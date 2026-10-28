use super::*;
use crate::actions::{
    Action,
    geometry::{self, SizeHints, Strut},
};
use x11rb::protocol::xproto::MapState;

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub action: Action,
    pub x11_id: Xid,
    pub requested: Rect,
    pub actual: Rect,
    pub constrained: bool,
}

/// Close acknowledges only that the request was sent, not that the app exited.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ActionOutcome {
    Geometry(Outcome),
    Close {
        action: Action,
        x11_id: Xid,
        close_requested: bool,
    },
}

impl Desktop {
    pub fn perform_action(&mut self, expected: &Window, action: Action) -> Result<ActionOutcome> {
        if action == Action::Close {
            let target = self.live_target(expected)?;
            // Let xfwm4 perform the normal WM_DELETE_WINDOW protocol, preserving
            // client save/cancel dialogs. Never kill a client or wait for exit.
            self.message(target.x11_id, self.atoms._NET_CLOSE_WINDOW, [0, 2, 0, 0, 0])?;
            return Ok(ActionOutcome::Close {
                action,
                x11_id: target.x11_id,
                close_requested: true,
            });
        }
        self.apply_action(expected, action)
            .map(ActionOutcome::Geometry)
    }
    fn live_target(&mut self, expected: &Window) -> Result<Window> {
        self.windows()?
            .into_iter()
            .find(|window| window.x11_id == expected.x11_id && window.identity == expected.identity)
            .ok_or_else(|| "action target closed or its X11 identity changed".into())
    }
    fn wait_target(
        &mut self,
        expected: &Window,
        ready: impl Fn(&Window) -> bool,
    ) -> Result<Window> {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let window = self.live_target(expected)?;
            if ready(&window) {
                return Ok(window);
            }
            if Instant::now() >= deadline {
                return Err("xfwm4 did not apply the requested window state/geometry".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn usable_area(&self, workspace: Option<u32>, output: Rect) -> Result<Rect> {
        let root_geometry = self.connection.get_geometry(self.root)?.reply()?;
        let root = Rect {
            x: 0,
            y: 0,
            width: u32::from(root_geometry.width),
            height: u32::from(root_geometry.height),
        };
        let workspace = workspace.unwrap_or(
            self.numbers(
                self.root,
                self.atoms._NET_CURRENT_DESKTOP,
                AtomEnum::CARDINAL.into(),
            )?
            .first()
            .copied()
            .unwrap_or(0),
        );
        let mut struts = Vec::new();
        for id in self.numbers(
            self.root,
            self.atoms._NET_CLIENT_LIST,
            AtomEnum::WINDOW.into(),
        )? {
            // Closing clients are harmless; malformed surviving reservations are
            // not silently treated as free space.
            let partial = match self.numbers(
                id,
                self.atoms._NET_WM_STRUT_PARTIAL,
                AtomEnum::CARDINAL.into(),
            ) {
                Ok(values) => values,
                Err(_) => continue,
            };
            let legacy = if partial.is_empty() {
                match self.numbers(id, self.atoms._NET_WM_STRUT, AtomEnum::CARDINAL.into()) {
                    Ok(v) => v,
                    Err(_) => continue,
                }
            } else {
                Vec::new()
            };
            if partial.is_empty() && legacy.is_empty() {
                continue;
            }
            let desktop =
                match self.numbers(id, self.atoms._NET_WM_DESKTOP, AtomEnum::CARDINAL.into()) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
            if desktop
                .first()
                .is_some_and(|desktop| *desktop != u32::MAX && *desktop != workspace)
            {
                continue;
            }
            let attributes = match self.connection.get_window_attributes(id)?.reply() {
                Ok(v) => v,
                Err(_) => continue,
            };
            if attributes.map_state != MapState::VIEWABLE {
                continue;
            }
            let size = match self.connection.get_geometry(id)?.reply() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let position = match self
                .connection
                .translate_coordinates(id, self.root, 0, 0)?
                .reply()
            {
                Ok(v) => v,
                Err(_) => continue,
            };
            let panel = Rect {
                x: i32::from(position.dst_x),
                y: i32::from(position.dst_y),
                width: u32::from(size.width),
                height: u32::from(size.height),
            };
            if let Some(strut) = Strut::from_wire(&partial, &legacy, root, panel)? {
                struts.push(strut);
            }
        }
        if !struts.is_empty() {
            return geometry::usable(output, root, &struts);
        }
        // Legacy/no-strut WMs may still publish explicit workarea margins.
        let areas = self.numbers(
            self.root,
            self.atoms._NET_WORKAREA,
            AtomEnum::CARDINAL.into(),
        )?;
        if let Some(area) = areas.as_chunks::<4>().0.get(workspace as usize) {
            return geometry::intersection(
                output,
                Rect {
                    x: area[0] as i32,
                    y: area[1] as i32,
                    width: area[2],
                    height: area[3],
                },
            )
            .ok_or_else(|| "no usable area on this output".into());
        }
        Ok(output)
    }
    fn fit_action(&self, id: Xid, area: Rect) -> Result<([u32; 2], [u32; 2])> {
        let [left, right, top, bottom] = frame_extents(&self.numbers(
            id,
            self.atoms._NET_FRAME_EXTENTS,
            AtomEnum::CARDINAL.into(),
        )?)?;
        let width = area
            .width
            .checked_sub(left + right)
            .filter(|v| *v > 0)
            .ok_or("window decorations cannot fit action area")?;
        let height = area
            .height
            .checked_sub(top + bottom)
            .filter(|v| *v > 0)
            .ok_or("window decorations cannot fit action area")?;
        let hints = SizeHints::from_wire(&self.numbers(
            id,
            AtomEnum::WM_NORMAL_HINTS.into(),
            AtomEnum::WM_SIZE_HINTS.into(),
        )?)?;
        let client = hints.fit(width, height)?;
        Ok((client, [client[0] + left + right, client[1] + top + bottom]))
    }
    pub fn apply_action(&mut self, expected: &Window, action: Action) -> Result<Outcome> {
        if action == Action::Close {
            return Err("close is not a geometry action".into());
        }
        let before = self.live_target(expected)?;
        let mut outputs = self.outputs()?;
        outputs.sort_by(|a, b| (a.rect.x, a.rect.y, &a.name).cmp(&(b.rect.x, b.rect.y, &b.name)));
        let source_index = outputs
            .iter()
            .position(|o| Some(&o.name) == before.output.as_ref())
            .ok_or("action target is not on an active output")?;
        let switching = matches!(action, Action::NextOutput | Action::PreviousOutput);
        if switching && outputs.len() < 2 {
            return Err("no other active output to move to".into());
        }
        let destination_index = match action {
            Action::NextOutput => (source_index + 1) % outputs.len(),
            Action::PreviousOutput => (source_index + outputs.len() - 1) % outputs.len(),
            _ => source_index,
        };
        self.focus(&before)?;
        let source = self.usable_area(before.workspace, outputs[source_index].rect)?;
        let destination = self.usable_area(before.workspace, outputs[destination_index].rect)?;
        let rough = if switching {
            Rect {
                width: before.rect.width.min(destination.width),
                height: before.rect.height.min(destination.height),
                ..destination
            }
        } else {
            action.area(destination, before.rect)?
        };
        self.fit_action(before.x11_id, rough)?; // reject impossible minima before changing state
        self.live_target(expected)?;
        self.message(
            before.x11_id,
            self.atoms._NET_WM_STATE,
            [0, self.atoms._NET_WM_STATE_FULLSCREEN, 0, 2, 0],
        )?;
        self.live_target(expected)?;
        self.message(
            before.x11_id,
            self.atoms._NET_WM_STATE,
            [
                0,
                self.atoms._NET_WM_STATE_MAXIMIZED_HORZ,
                self.atoms._NET_WM_STATE_MAXIMIZED_VERT,
                2,
                0,
            ],
        )?;
        let normal = self.wait_target(expected, |w| {
            !w.fullscreen && !w.maximized_horz && !w.maximized_vert
        })?;
        let requested = if switching {
            geometry::map_between(
                source,
                destination,
                before.rect,
                normal.rect.width.min(destination.width),
                normal.rect.height.min(destination.height),
            )?
        } else {
            action.area(destination, normal.rect)?
        };
        let (client, outer) = self.fit_action(normal.x11_id, requested)?;
        let wanted = if switching {
            geometry::map_between(source, destination, before.rect, outer[0], outer[1])?
        } else {
            action.anchor(requested, outer[0], outer[1])?
        };
        self.live_target(expected)?;
        // NorthWest gravity: x/y anchor the frame; width/height are client size.
        self.message(
            normal.x11_id,
            self.atoms._NET_MOVERESIZE_WINDOW,
            [
                1 | (15 << 8) | (2 << 12),
                wanted.x as u32,
                wanted.y as u32,
                client[0],
                client[1],
            ],
        )?;
        let actual = self.wait_target(expected, |w| {
            w.rect == wanted && !w.fullscreen && !w.maximized_horz && !w.maximized_vert
        })?;
        self.focus(&actual)?;
        Ok(Outcome {
            action,
            x11_id: actual.x11_id,
            requested,
            actual: actual.rect,
            constrained: actual.rect != requested,
        })
    }
}
