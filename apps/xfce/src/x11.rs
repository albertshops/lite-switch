//! Xfce/xfwm4-specific X11 integration. No shell commands or i3 IPC.
#[cfg(feature = "gui")]
#[path = "x11/actions.rs"]
mod actions;
#[cfg(feature = "gui")]
pub use actions::ActionOutcome;
use serde::Serialize;
use std::{
    cell::Cell,
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    atom_manager,
    connection::Connection,
    protocol::{
        Event,
        randr::ConnectionExt as _,
        xproto::{
            Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window as Xid,
        },
    },
    rust_connection::RustConnection,
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const MAX_PROPERTY: u32 = 128 * 1024;
static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);

atom_manager! {
    Atoms: AtomsCookie {
        UTF8_STRING,
        _NET_SUPPORTING_WM_CHECK,
        _NET_WM_NAME,
        _NET_CLIENT_LIST,
        _NET_ACTIVE_WINDOW,
        _NET_CLOSE_WINDOW,
        _NET_CURRENT_DESKTOP,
        _NET_DESKTOP_NAMES,
        _NET_WM_DESKTOP,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_WINDOW_TYPE_NOTIFICATION,
        _NET_WM_WINDOW_TYPE_TOOLTIP,
        _NET_WM_WINDOW_TYPE_DND,
        _NET_WM_WINDOW_TYPE_MENU,
        _NET_WM_WINDOW_TYPE_POPUP_MENU,
        _NET_WM_WINDOW_TYPE_DROPDOWN_MENU,
        _NET_WM_STATE,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_MOVERESIZE_WINDOW,
        _NET_WORKAREA,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_STATE_MAXIMIZED_HORZ,
        _NET_WM_STATE_MAXIMIZED_VERT,
        _NET_WM_STRUT_PARTIAL,
        _NET_WM_STRUT,
        _NET_WM_STATE_DEMANDS_ATTENTION,
        _NET_FRAME_EXTENTS,
        _NET_WM_PID,
        WM_WINDOW_ROLE,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    fn overlap(self, other: Self) -> u64 {
        let x = (i64::from(self.x) + i64::from(self.width))
            .min(i64::from(other.x) + i64::from(other.width))
            - i64::from(self.x.max(other.x));
        let y = (i64::from(self.y) + i64::from(self.height))
            .min(i64::from(other.y) + i64::from(other.height))
            - i64::from(self.y.max(other.y));
        x.max(0) as u64 * y.max(0) as u64
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Output {
    pub name: String,
    pub primary: bool,
    pub rect: Rect,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Window {
    pub x11_id: Xid,
    /// Process-local generation. Never persist or compare across CLI invocations.
    pub identity: u64,
    pub title: String,
    pub class: Option<String>,
    pub instance: Option<String>,
    pub role: Option<String>,
    pub pid: Option<u32>,
    pub workspace: Option<u32>,
    pub workspace_name: Option<String>,
    pub output: Option<String>,
    pub rect: Rect,
    pub focused: bool,
    pub minimized: bool,
    pub fullscreen: bool,
    pub maximized_horz: bool,
    pub maximized_vert: bool,
    pub urgent: bool,
}

#[derive(Default)]
struct Identities(BTreeMap<Xid, u64>);
impl Identities {
    fn observe(&mut self, xid: Xid) -> u64 {
        *self
            .0
            .entry(xid)
            .or_insert_with(|| NEXT_IDENTITY.fetch_add(1, Ordering::Relaxed))
    }
    fn forget(&mut self, xid: Xid) {
        self.0.remove(&xid);
    }
    fn retain(&mut self, live: &[Xid]) {
        self.0.retain(|xid, _| live.contains(xid));
    }
}

pub struct Desktop {
    connection: RustConnection,
    root: Xid,
    manager: Xid,
    atoms: Atoms,
    identities: Identities,
    invalidated: Cell<bool>,
    pending_refresh: bool,
}

impl Desktop {
    pub fn connect() -> Result<Self> {
        let (connection, screen) = x11rb::connect(None).map_err(|e| {
            format!("cannot connect to X11; run inside Xfce/X11 and check DISPLAY: {e}")
        })?;
        let root = connection.setup().roots[screen].root;
        let atoms = Atoms::new(&connection)?.reply()?;
        let mut desktop = Self {
            connection,
            root,
            manager: 0,
            atoms,
            identities: Identities::default(),
            invalidated: Cell::new(false),
            pending_refresh: false,
        };
        desktop.manager = desktop.manager_id()?;
        let name = desktop.text(
            desktop.manager,
            desktop.atoms._NET_WM_NAME,
            desktop.atoms.UTF8_STRING,
        )?;
        if !name.eq_ignore_ascii_case("xfwm4") {
            return Err(
                format!("expected Xfce/xfwm4 on X11, found window manager {name:?}").into(),
            );
        }
        desktop
            .connection
            .change_window_attributes(
                root,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::PROPERTY_CHANGE | EventMask::SUBSTRUCTURE_NOTIFY),
            )?
            .check()?;
        desktop
            .connection
            .change_window_attributes(
                desktop.manager,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::STRUCTURE_NOTIFY),
            )?
            .check()?;
        let version = desktop.connection.randr_query_version(1, 5)?.reply()?;
        if (version.major_version, version.minor_version) < (1, 5) {
            return Err("Xfce output discovery requires RandR 1.5 or newer".into());
        }
        desktop
            .connection
            .randr_select_input(
                root,
                x11rb::protocol::randr::NotifyMask::SCREEN_CHANGE
                    | x11rb::protocol::randr::NotifyMask::CRTC_CHANGE
                    | x11rb::protocol::randr::NotifyMask::OUTPUT_CHANGE
                    | x11rb::protocol::randr::NotifyMask::RESOURCE_CHANGE,
            )?
            .check()?;
        Ok(desktop)
    }

    fn manager_id(&self) -> Result<Xid> {
        let manager = self
            .numbers(
                self.root,
                self.atoms._NET_SUPPORTING_WM_CHECK,
                AtomEnum::WINDOW.into(),
            )?
            .first()
            .copied()
            .ok_or("X11 has no EWMH window manager; expected Xfce/xfwm4")?;
        if self
            .numbers(
                manager,
                self.atoms._NET_SUPPORTING_WM_CHECK,
                AtomEnum::WINDOW.into(),
            )?
            .first()
            != Some(&manager)
        {
            return Err("window-manager self reference is invalid".into());
        }
        Ok(manager)
    }

    fn check_manager(&self) -> Result<()> {
        if self.invalidated.get() {
            return Err(
                "window-manager continuity lost; reconnect and clear session targets".into(),
            );
        }
        let manager = match self.manager_id() {
            Ok(manager) => manager,
            Err(error) => {
                self.invalidated.set(true);
                return Err(error);
            }
        };
        if manager != self.manager {
            self.invalidated.set(true);
            return Err("window manager changed; reconnect and clear session targets".into());
        }
        Ok(())
    }

    fn numbers(&self, window: Xid, property: Atom, kind: Atom) -> Result<Vec<u32>> {
        let reply = self
            .connection
            .get_property(false, window, property, kind, 0, MAX_PROPERTY / 4)?
            .reply()?;
        if kind != u32::from(AtomEnum::ANY) && reply.type_ != 0 && reply.type_ != kind {
            return Ok(Vec::new());
        }
        if reply.bytes_after != 0 {
            return Err("X11 property exceeds safe size limit".into());
        }
        if reply.format == 0 {
            return Ok(Vec::new());
        }
        Ok(reply
            .value32()
            .ok_or("expected a 32-bit X11 property")?
            .collect())
    }

    fn bytes(&self, window: Xid, property: Atom, kind: Atom) -> Result<Vec<u8>> {
        let reply = self
            .connection
            .get_property(false, window, property, kind, 0, MAX_PROPERTY / 4)?
            .reply()?;
        // Some legacy tools incorrectly label _NET_WM_NAME as STRING. A
        // type mismatch reports bytes_after too, but is not an oversized value;
        // treat it as absent so callers can fall back to ICCCM WM_NAME.
        if kind != u32::from(AtomEnum::ANY) && reply.type_ != 0 && reply.type_ != kind {
            return Ok(Vec::new());
        }
        if reply.bytes_after != 0 {
            return Err("X11 property exceeds safe size limit".into());
        }
        if reply.format != 0 && reply.format != 8 {
            return Err("expected an 8-bit X11 property".into());
        }
        Ok(reply.value)
    }

    fn text(&self, window: Xid, property: Atom, kind: Atom) -> Result<String> {
        Ok(
            String::from_utf8_lossy(&self.bytes(window, property, kind)?)
                .trim_end_matches('\0')
                .to_owned(),
        )
    }

    /// Drain destruction events before resolving identities, including XID reuse
    /// between snapshots. A lost connection is an error, never continuity.
    fn drain(&mut self) -> Result<bool> {
        let mut changed = false;
        while let Some(event) = self.connection.poll_for_event()? {
            changed |= match &event {
                Event::PropertyNotify(event) => {
                    let root_properties = [
                        self.atoms._NET_CLIENT_LIST,
                        self.atoms._NET_ACTIVE_WINDOW,
                        self.atoms._NET_CURRENT_DESKTOP,
                        self.atoms._NET_DESKTOP_NAMES,
                        self.atoms._NET_WORKAREA,
                        self.atoms._NET_SUPPORTING_WM_CHECK,
                    ];
                    let client_properties = [
                        self.atoms._NET_WM_NAME,
                        u32::from(AtomEnum::WM_NAME),
                        u32::from(AtomEnum::WM_CLASS),
                        self.atoms.WM_WINDOW_ROLE,
                        self.atoms._NET_WM_DESKTOP,
                        self.atoms._NET_WM_STATE,
                        self.atoms._NET_FRAME_EXTENTS,
                        self.atoms._NET_WM_PID,
                        u32::from(AtomEnum::WM_HINTS),
                    ];
                    if event.window == self.root {
                        root_properties.contains(&event.atom)
                    } else {
                        client_properties.contains(&event.atom)
                    }
                }
                Event::ConfigureNotify(_)
                | Event::MapNotify(_)
                | Event::UnmapNotify(_)
                | Event::DestroyNotify(_)
                | Event::ReparentNotify(_)
                | Event::RandrNotify(_)
                | Event::RandrScreenChangeNotify(_) => true,
                _ => false,
            };
            if let Event::DestroyNotify(event) = event {
                self.identities.forget(event.window);
                if event.window == self.manager {
                    self.invalidated.set(true);
                }
            }
        }
        if self.invalidated.get() {
            return Err(
                "window-manager continuity lost; reconnect and clear session targets".into(),
            );
        }
        Ok(changed)
    }

    #[cfg(feature = "gui")]
    pub fn event_fd(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.connection.stream().as_raw_fd()
    }

    #[cfg(feature = "gui")]
    pub fn process_events(&mut self) -> Result<bool> {
        self.check_manager()?;
        self.drain()
    }

    #[cfg(feature = "gui")]
    pub fn take_pending_refresh(&mut self) -> bool {
        std::mem::take(&mut self.pending_refresh)
    }

    #[cfg(feature = "gui")]
    pub fn prepare_panel(
        &mut self,
        id: Xid,
        width: u32,
        height: u32,
        output: Option<&str>,
    ) -> Result<bool> {
        if !self.windows()?.iter().any(|w| w.x11_id == id) {
            return Ok(false);
        }
        let outputs = self.outputs()?;
        let output = output
            .and_then(|name| outputs.iter().find(|o| o.name == name))
            .or_else(|| outputs.iter().find(|o| o.primary))
            .or_else(|| outputs.first())
            .ok_or("no active X11 monitor")?;
        let mut usable = output.rect;
        let current = self
            .numbers(
                self.root,
                self.atoms._NET_CURRENT_DESKTOP,
                AtomEnum::CARDINAL.into(),
            )?
            .first()
            .copied()
            .unwrap_or(0);
        let areas = self.numbers(
            self.root,
            self.atoms._NET_WORKAREA,
            AtomEnum::CARDINAL.into(),
        )?;
        if let Some(area) = areas.as_chunks::<4>().0.get(current as usize) {
            let x = i64::from(usable.x).max(i64::from(area[0] as i32));
            let y = i64::from(usable.y).max(i64::from(area[1] as i32));
            let right = (i64::from(usable.x) + i64::from(usable.width))
                .min(i64::from(area[0] as i32) + i64::from(area[2]));
            let bottom = (i64::from(usable.y) + i64::from(usable.height))
                .min(i64::from(area[1] as i32) + i64::from(area[3]));
            if right > x && bottom > y {
                usable = Rect {
                    x: x.try_into()?,
                    y: y.try_into()?,
                    width: (right - x).try_into()?,
                    height: (bottom - y).try_into()?,
                };
            }
        }
        let width = width.min(usable.width);
        let height = height.min(usable.height);
        let x = usable.x + ((usable.width - width) / 2) as i32;
        let y = usable.y + ((usable.height - height) / 2) as i32;
        self.message(
            id,
            self.atoms._NET_WM_STATE,
            [
                1,
                self.atoms._NET_WM_STATE_SKIP_TASKBAR,
                self.atoms._NET_WM_STATE_SKIP_PAGER,
                2,
                0,
            ],
        )?;
        self.message(
            id,
            self.atoms._NET_MOVERESIZE_WINDOW,
            [(15 << 8) | (2 << 12), x as u32, y as u32, width, height],
        )?;
        Ok(self.windows()?.iter().any(|w| {
            w.x11_id == id
                && w.rect
                    == Rect {
                        x,
                        y,
                        width,
                        height,
                    }
        }))
    }

    pub fn active_id(&self) -> Result<Option<Xid>> {
        Ok(self
            .numbers(
                self.root,
                self.atoms._NET_ACTIVE_WINDOW,
                AtomEnum::WINDOW.into(),
            )?
            .first()
            .copied()
            .filter(|id| *id != 0))
    }

    pub fn outputs(&self) -> Result<Vec<Output>> {
        self.check_manager()?;
        let reply = self
            .connection
            .randr_get_monitors(self.root, true)?
            .reply()?;
        reply
            .monitors
            .into_iter()
            .map(|monitor| {
                let name = self.connection.get_atom_name(monitor.name)?.reply()?.name;
                Ok(Output {
                    name: String::from_utf8_lossy(&name).into_owned(),
                    primary: monitor.primary,
                    rect: Rect {
                        x: i32::from(monitor.x),
                        y: i32::from(monitor.y),
                        width: u32::from(monitor.width),
                        height: u32::from(monitor.height),
                    },
                })
            })
            .collect()
    }

    pub fn windows(&mut self) -> Result<Vec<Window>> {
        self.pending_refresh = false;
        self.check_manager()?;
        self.drain()?;
        let ids = self.numbers(
            self.root,
            self.atoms._NET_CLIENT_LIST,
            AtomEnum::WINDOW.into(),
        )?;
        self.identities.retain(&ids);
        let active = self.active_id()?;
        let names = self.bytes(
            self.root,
            self.atoms._NET_DESKTOP_NAMES,
            self.atoms.UTF8_STRING,
        )?;
        let names: Vec<_> = names
            .split(|byte| *byte == 0)
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect();
        let outputs = self.outputs()?;
        let mut windows = Vec::new();
        for id in ids {
            // Clients may close during discovery. Root/connection errors still
            // surface via check_manager/drain, rather than being swallowed.
            if let Ok(Some(window)) = self.window(id, active, &names, &outputs) {
                windows.push(window);
            }
        }
        self.pending_refresh = self.drain()?;
        windows.retain(|w| self.identities.0.get(&w.x11_id) == Some(&w.identity));
        Ok(windows)
    }

    fn window(
        &mut self,
        id: Xid,
        active: Option<Xid>,
        names: &[String],
        outputs: &[Output],
    ) -> Result<Option<Window>> {
        self.connection
            .change_window_attributes(
                id,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY),
            )?
            .check()?;
        let types = self.numbers(id, self.atoms._NET_WM_WINDOW_TYPE, AtomEnum::ATOM.into())?;
        let excluded = [
            self.atoms._NET_WM_WINDOW_TYPE_DOCK,
            self.atoms._NET_WM_WINDOW_TYPE_DESKTOP,
            self.atoms._NET_WM_WINDOW_TYPE_NOTIFICATION,
            self.atoms._NET_WM_WINDOW_TYPE_TOOLTIP,
            self.atoms._NET_WM_WINDOW_TYPE_DND,
            self.atoms._NET_WM_WINDOW_TYPE_MENU,
            self.atoms._NET_WM_WINDOW_TYPE_POPUP_MENU,
            self.atoms._NET_WM_WINDOW_TYPE_DROPDOWN_MENU,
        ];
        if types.iter().any(|kind| excluded.contains(kind)) {
            return Ok(None);
        }
        let mut title = self.text(id, self.atoms._NET_WM_NAME, self.atoms.UTF8_STRING)?;
        if title.is_empty() {
            title = self.text(id, AtomEnum::WM_NAME.into(), self.atoms.UTF8_STRING)?;
            if title.is_empty() {
                title =
                    latin1(&self.bytes(id, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into())?);
            }
        }
        let classes = self.bytes(id, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into())?;
        let (instance, class) = wm_class(&classes);
        let role = self.text(id, self.atoms.WM_WINDOW_ROLE, AtomEnum::ANY.into())?;
        let workspace = self
            .numbers(id, self.atoms._NET_WM_DESKTOP, AtomEnum::CARDINAL.into())?
            .first()
            .copied()
            .filter(|id| *id != u32::MAX);
        let states = self.numbers(id, self.atoms._NET_WM_STATE, AtomEnum::ATOM.into())?;
        let hints = self.numbers(id, AtomEnum::WM_HINTS.into(), AtomEnum::WM_HINTS.into())?;
        let geometry = self.connection.get_geometry(id)?.reply()?;
        let position = self
            .connection
            .translate_coordinates(id, self.root, 0, 0)?
            .reply()?;
        let extents = self.numbers(id, self.atoms._NET_FRAME_EXTENTS, AtomEnum::CARDINAL.into())?;
        let [left, right, top, bottom] = frame_extents(&extents)?;
        let rect = Rect {
            x: i32::from(position.dst_x) - left as i32,
            y: i32::from(position.dst_y) - top as i32,
            width: u32::from(geometry.width) + left + right,
            height: u32::from(geometry.height) + top + bottom,
        };
        let output = outputs
            .iter()
            .filter(|o| rect.overlap(o.rect) > 0)
            .max_by_key(|o| rect.overlap(o.rect))
            .map(|o| o.name.clone());
        let pid = self
            .numbers(id, self.atoms._NET_WM_PID, AtomEnum::CARDINAL.into())?
            .first()
            .copied();
        let identity = self.identities.observe(id);
        Ok(Some(Window {
            x11_id: id,
            identity,
            title,
            class,
            instance,
            role: (!role.is_empty()).then_some(role),
            pid,
            workspace,
            workspace_name: workspace.and_then(|id| names.get(id as usize).cloned()),
            output,
            rect,
            focused: active == Some(id),
            minimized: states.contains(&self.atoms._NET_WM_STATE_HIDDEN),
            fullscreen: states.contains(&self.atoms._NET_WM_STATE_FULLSCREEN),
            maximized_horz: states.contains(&self.atoms._NET_WM_STATE_MAXIMIZED_HORZ),
            maximized_vert: states.contains(&self.atoms._NET_WM_STATE_MAXIMIZED_VERT),
            urgent: states.contains(&self.atoms._NET_WM_STATE_DEMANDS_ATTENTION)
                || hints.first().is_some_and(|flags| flags & (1 << 8) != 0),
        }))
    }

    fn message(&self, window: Xid, kind: Atom, data: [u32; 5]) -> Result<()> {
        let event = ClientMessageEvent::new(32, window, kind, data);
        self.connection
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )?
            .check()?;
        self.connection.flush()?;
        Ok(())
    }

    pub fn focus(&mut self, expected: &Window) -> Result<()> {
        let live = self
            .windows()?
            .into_iter()
            .find(|w| w.x11_id == expected.x11_id && w.identity == expected.identity)
            .ok_or("window closed or its X11 identity was replaced")?;
        if let Some(workspace) = live.workspace {
            self.message(
                self.root,
                self.atoms._NET_CURRENT_DESKTOP,
                [workspace, 0, 0, 0, 0],
            )?;
        }
        self.message(
            live.x11_id,
            self.atoms._NET_ACTIVE_WINDOW,
            [2, 0, self.active_id()?.unwrap_or(0), 0, 0],
        )?;
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            self.check_manager()?;
            self.drain()?;
            if self.identities.0.get(&live.x11_id) != Some(&live.identity) {
                return Err("window was destroyed during activation".into());
            }
            if self.active_id()? == Some(live.x11_id) {
                // The round trip may have queued a destruction/reuse event.
                self.drain()?;
                if self.identities.0.get(&live.x11_id) == Some(&live.identity) {
                    return Ok(());
                }
                return Err("window was destroyed during activation".into());
            }
            if Instant::now() >= deadline {
                return Err("xfwm4 did not activate the requested window".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| char::from(*b))
        .collect()
}
fn wm_class(bytes: &[u8]) -> (Option<String>, Option<String>) {
    let mut parts = bytes.split(|byte| *byte == 0);
    let mut part = || parts.next().filter(|bytes| !bytes.is_empty()).map(latin1);
    (part(), part())
}
fn frame_extents(values: &[u32]) -> Result<[u32; 4]> {
    if values.is_empty() {
        return Ok([0; 4]);
    }
    let values: [u32; 4] = values.try_into().map_err(|_| "invalid X11 frame extents")?;
    if values.iter().any(|value| *value > 4096) {
        return Err("unreasonable X11 frame extents".into());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_stable_until_destroyed_or_removed() {
        let mut ids = Identities::default();
        let first = ids.observe(42);
        assert_eq!(ids.observe(42), first);
        ids.forget(42);
        assert_ne!(ids.observe(42), first);
        let second = ids.observe(42);
        ids.retain(&[]);
        assert_ne!(ids.observe(42), second);
        assert_ne!(Identities::default().observe(42), first);
    }
    #[test]
    fn rectangle_overlap_handles_negative_positions_and_wide_values() {
        let a = Rect {
            x: -100,
            y: -20,
            width: 200,
            height: 50,
        };
        assert_eq!(
            a.overlap(Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 200
            }),
            3000
        );
        assert_eq!(a.overlap(Rect { x: 100, ..a }), 0);
        let huge = Rect {
            x: i32::MAX,
            y: i32::MIN,
            width: u32::MAX,
            height: u32::MAX,
        };
        assert_eq!(huge.overlap(huge), u64::from(u32::MAX).pow(2));
    }
    #[test]
    fn properties_are_bounded_and_icccm_class_is_instance_then_class() {
        assert_eq!(
            wm_class(b"editor\0Editor\0"),
            (Some("editor".into()), Some("Editor".into()))
        );
        assert_eq!(wm_class(b"\0"), (None, None));
        assert_eq!(latin1(b"caf\xe9\0ignored"), "café");
        assert_eq!(frame_extents(&[]).unwrap(), [0; 4]);
        assert!(frame_extents(&[1, 2]).is_err());
        assert!(frame_extents(&[u32::MAX; 4]).is_err());
    }
}
