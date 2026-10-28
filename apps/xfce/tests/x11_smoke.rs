//! Run only through desktop_smoke.py on its private X server/window manager.
#[allow(dead_code)]
#[path = "../src/actions.rs"]
mod actions;
#[allow(dead_code, unused_imports)]
#[path = "../src/x11.rs"]
mod x11;
use std::{
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, PropMode,
        WindowClass,
    },
    wrapper::ConnectionExt as _,
};

fn wait(mut check: impl FnMut() -> bool, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !check() {
        assert!(Instant::now() < deadline, "{message}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires the private Xvfb/xfwm4 session from desktop_smoke.py"]
fn real_ewmh_discovery_focus_and_reused_xid() {
    assert_eq!(
        std::env::var("LITE_SWITCH_ISOLATED_X11").as_deref(),
        Ok("1")
    );
    let (client, screen) = x11rb::connect(None).unwrap();
    let root = client.setup().roots[screen].root;
    let atom = |name: &str| {
        client
            .intern_atom(false, name.as_bytes())
            .unwrap()
            .reply()
            .unwrap()
            .atom
    };
    let create = |id, title: &str, workspace: u32, kind: &str| {
        client
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                id,
                root,
                20,
                30,
                400,
                250,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
            )
            .unwrap()
            .check()
            .unwrap();
        client
            .change_property8(
                PropMode::REPLACE,
                id,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                b"test-instance\0LiteSwitchTest\0",
            )
            .unwrap()
            .check()
            .unwrap();
        client
            .change_property8(
                PropMode::REPLACE,
                id,
                atom("_NET_WM_NAME"),
                atom("UTF8_STRING"),
                title.as_bytes(),
            )
            .unwrap()
            .check()
            .unwrap();
        client
            .change_property32(
                PropMode::REPLACE,
                id,
                atom("_NET_WM_DESKTOP"),
                AtomEnum::CARDINAL,
                &[workspace],
            )
            .unwrap()
            .check()
            .unwrap();
        client
            .change_property32(
                PropMode::REPLACE,
                id,
                atom("_NET_WM_WINDOW_TYPE"),
                AtomEnum::ATOM,
                &[atom(kind)],
            )
            .unwrap()
            .check()
            .unwrap();
        client.map_window(id).unwrap().check().unwrap();
        client.flush().unwrap();
    };
    let id = client.generate_id().unwrap();
    create(id, "Éditeur — test", 0, "_NET_WM_WINDOW_TYPE_NORMAL");
    let mut desktop = x11::Desktop::connect().unwrap();
    wait(
        || desktop.windows().unwrap().iter().any(|w| w.x11_id == id),
        "window not discovered",
    );
    let original = desktop
        .windows()
        .unwrap()
        .into_iter()
        .find(|w| w.x11_id == id)
        .unwrap();
    assert_eq!(original.title, "Éditeur — test");
    client
        .change_property8(
            PropMode::REPLACE,
            id,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            b"caf\xe9",
        )
        .unwrap()
        .check()
        .unwrap();
    client
        .change_property8(
            PropMode::REPLACE,
            id,
            atom("_NET_WM_NAME"),
            AtomEnum::STRING,
            b"wrong-type",
        )
        .unwrap()
        .check()
        .unwrap();
    assert_eq!(
        desktop
            .windows()
            .unwrap()
            .into_iter()
            .find(|w| w.x11_id == id)
            .unwrap()
            .title,
        "café"
    );
    client
        .change_property8(
            PropMode::REPLACE,
            id,
            atom("_NET_WM_NAME"),
            atom("UTF8_STRING"),
            "Éditeur — test".as_bytes(),
        )
        .unwrap()
        .check()
        .unwrap();
    assert_eq!(original.class.as_deref(), Some("LiteSwitchTest"));
    assert_eq!(original.instance.as_deref(), Some("test-instance"));
    assert!(original.rect.width >= 400 && original.rect.height >= 250);
    assert!(!desktop.outputs().unwrap().is_empty());
    // ConfigureRequest-compatible coordinates: NorthWest gravity anchors the
    // outer frame, while the supplied width/height are client dimensions.
    let extents: Vec<_> = client
        .get_property(
            false,
            id,
            atom("_NET_FRAME_EXTENTS"),
            AtomEnum::CARDINAL,
            0,
            4,
        )
        .unwrap()
        .reply()
        .unwrap()
        .value32()
        .unwrap()
        .collect();
    assert_eq!(extents.len(), 4);
    let move_resize = ClientMessageEvent::new(
        32,
        id,
        atom("_NET_MOVERESIZE_WINDOW"),
        [1 | (15 << 8) | (2 << 12), 100, 120, 500, 400],
    );
    client
        .send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            move_resize,
        )
        .unwrap()
        .check()
        .unwrap();
    wait(
        || {
            desktop
                .windows()
                .unwrap()
                .iter()
                .any(|w| w.x11_id == id && w.rect.width == 500 + extents[0] + extents[1])
        },
        "frame resize request not applied",
    );
    let resized = desktop
        .windows()
        .unwrap()
        .into_iter()
        .find(|w| w.x11_id == id)
        .unwrap();
    assert_eq!(
        resized.rect,
        x11::Rect {
            x: 100,
            y: 120,
            width: 500 + extents[0] + extents[1],
            height: 400 + extents[2] + extents[3]
        }
    );
    #[cfg(feature = "gui")]
    {
        use actions::Action;
        let neighbor = client.generate_id().unwrap();
        create(
            neighbor,
            "Unchanged Neighbor",
            0,
            "_NET_WM_WINDOW_TYPE_NORMAL",
        );
        wait(
            || {
                desktop
                    .windows()
                    .unwrap()
                    .iter()
                    .any(|w| w.x11_id == neighbor)
            },
            "neighbor missing",
        );
        let neighbor_rect = desktop
            .windows()
            .unwrap()
            .into_iter()
            .find(|w| w.x11_id == neighbor)
            .unwrap()
            .rect;
        let panel = client.generate_id().unwrap();
        client
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                panel,
                root,
                0,
                0,
                1280,
                40,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new(),
            )
            .unwrap()
            .check()
            .unwrap();
        client
            .change_property32(
                PropMode::REPLACE,
                panel,
                atom("_NET_WM_WINDOW_TYPE"),
                AtomEnum::ATOM,
                &[atom("_NET_WM_WINDOW_TYPE_DOCK")],
            )
            .unwrap()
            .check()
            .unwrap();
        client
            .change_property32(
                PropMode::REPLACE,
                panel,
                atom("_NET_WM_DESKTOP"),
                AtomEnum::CARDINAL,
                &[u32::MAX],
            )
            .unwrap()
            .check()
            .unwrap();
        client
            .change_property32(
                PropMode::REPLACE,
                panel,
                atom("_NET_WM_STRUT_PARTIAL"),
                AtomEnum::CARDINAL,
                &[0, 0, 40, 0, 0, 0, 0, 0, 0, 1279, 0, 0],
            )
            .unwrap()
            .check()
            .unwrap();
        client.map_window(panel).unwrap().check().unwrap();
        let output = desktop.outputs().unwrap()[0].rect;
        wait(
            || desktop.usable_area(Some(0), output).unwrap().y == 40,
            "panel reservation missing",
        );
        let usable = desktop.usable_area(Some(0), output).unwrap();
        for action in [
            Action::Fill,
            Action::LeftHalf,
            Action::RightHalf,
            Action::TopHalf,
            Action::BottomHalf,
        ] {
            let outcome = desktop.apply_action(&original, action).unwrap();
            assert_eq!(outcome.actual, action.area(usable, original.rect).unwrap());
            assert!(!outcome.constrained);
            assert_eq!(
                desktop
                    .windows()
                    .unwrap()
                    .into_iter()
                    .find(|w| w.x11_id == neighbor)
                    .unwrap()
                    .rect,
                neighbor_rect
            );
        }
        let before_failed = desktop
            .windows()
            .unwrap()
            .into_iter()
            .find(|w| w.x11_id == id)
            .unwrap()
            .rect;
        assert!(desktop.apply_action(&original, Action::NextOutput).is_err());
        assert_eq!(
            desktop
                .windows()
                .unwrap()
                .into_iter()
                .find(|w| w.x11_id == id)
                .unwrap()
                .rect,
            before_failed
        );
        let fullscreen = ClientMessageEvent::new(
            32,
            id,
            atom("_NET_WM_STATE"),
            [1, atom("_NET_WM_STATE_FULLSCREEN"), 0, 2, 0],
        );
        client
            .send_event(
                false,
                root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                fullscreen,
            )
            .unwrap()
            .check()
            .unwrap();
        wait(
            || {
                desktop
                    .windows()
                    .unwrap()
                    .iter()
                    .any(|w| w.x11_id == id && w.fullscreen)
            },
            "fullscreen test setup",
        );
        let result = desktop.apply_action(&original, Action::Fill).unwrap();
        assert_eq!(result.actual, usable);
        assert!(
            !desktop
                .windows()
                .unwrap()
                .into_iter()
                .find(|w| w.x11_id == id)
                .unwrap()
                .fullscreen
        );
        client.destroy_window(panel).unwrap().check().unwrap();
        wait(
            || desktop.usable_area(Some(0), output).unwrap() == output,
            "panel cleanup",
        );
    }
    #[cfg(feature = "gui")]
    {
        use actions::Action;
        use x11rb::protocol::randr::{ConnectionExt as _, MonitorInfo};
        let initial = client
            .randr_get_monitors(root, true)
            .unwrap()
            .reply()
            .unwrap()
            .monitors;
        assert_eq!(initial.len(), 1);
        let left = MonitorInfo {
            name: atom("TestLeft"),
            primary: true,
            automatic: false,
            x: 0,
            y: 0,
            width: 640,
            height: 900,
            width_in_millimeters: 160,
            height_in_millimeters: 225,
            outputs: initial[0].outputs.clone(),
        };
        let right = MonitorInfo {
            name: atom("TestRight"),
            primary: false,
            automatic: false,
            x: 640,
            y: 120,
            width: 640,
            height: 780,
            width_in_millimeters: 160,
            height_in_millimeters: 195,
            outputs: vec![],
        };
        client
            .randr_set_monitor(root, left.clone())
            .unwrap()
            .check()
            .unwrap();
        client
            .randr_set_monitor(root, right.clone())
            .unwrap()
            .check()
            .unwrap();
        wait(
            || desktop.outputs().unwrap().len() == 2,
            "private logical monitors unavailable",
        );
        thread::sleep(Duration::from_millis(100));
        let panel = client.generate_id().unwrap();
        client
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                panel,
                root,
                0,
                0,
                640,
                40,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new(),
            )
            .unwrap()
            .check()
            .unwrap();
        client
            .change_property32(
                PropMode::REPLACE,
                panel,
                atom("_NET_WM_WINDOW_TYPE"),
                AtomEnum::ATOM,
                &[atom("_NET_WM_WINDOW_TYPE_DOCK")],
            )
            .unwrap()
            .check()
            .unwrap();
        client
            .change_property32(
                PropMode::REPLACE,
                panel,
                atom("_NET_WM_DESKTOP"),
                AtomEnum::CARDINAL,
                &[u32::MAX],
            )
            .unwrap()
            .check()
            .unwrap();
        client
            .change_property32(
                PropMode::REPLACE,
                panel,
                atom("_NET_WM_STRUT_PARTIAL"),
                AtomEnum::CARDINAL,
                &[0, 0, 40, 0, 0, 0, 0, 0, 0, 639, 0, 0],
            )
            .unwrap()
            .check()
            .unwrap();
        client.map_window(panel).unwrap().check().unwrap();
        wait(
            || {
                desktop
                    .usable_area(
                        Some(0),
                        x11::Rect {
                            x: 0,
                            y: 0,
                            width: 640,
                            height: 900,
                        },
                    )
                    .unwrap()
                    .y
                    == 40
            },
            "left-output panel missing",
        );
        desktop.apply_action(&original, Action::LeftHalf).unwrap();
        let next = desktop.apply_action(&original, Action::NextOutput).unwrap();
        let moved = desktop
            .windows()
            .unwrap()
            .into_iter()
            .find(|w| w.x11_id == id)
            .unwrap();
        assert_eq!(moved.output.as_deref(), Some("TestRight"));
        assert_eq!(
            actions::geometry::intersection(
                next.actual,
                x11::Rect {
                    x: 640,
                    y: 120,
                    width: 640,
                    height: 780
                }
            ),
            Some(next.actual)
        );
        let previous = desktop
            .apply_action(&original, Action::PreviousOutput)
            .unwrap();
        let moved = desktop
            .windows()
            .unwrap()
            .into_iter()
            .find(|w| w.x11_id == id)
            .unwrap();
        assert_eq!(moved.output.as_deref(), Some("TestLeft"));
        assert_eq!(
            actions::geometry::intersection(
                previous.actual,
                x11::Rect {
                    x: 0,
                    y: 40,
                    width: 640,
                    height: 860
                }
            ),
            Some(previous.actual)
        );
        client.destroy_window(panel).unwrap().check().unwrap();
        client
            .randr_delete_monitor(root, left.name)
            .unwrap()
            .check()
            .unwrap();
        client
            .randr_delete_monitor(root, right.name)
            .unwrap()
            .check()
            .unwrap();
        wait(|| desktop.outputs().unwrap().len() == 1, "monitor cleanup");
        thread::sleep(Duration::from_millis(100));
    }
    let other = client.generate_id().unwrap();
    create(other, "Other workspace", 1, "_NET_WM_WINDOW_TYPE_NORMAL");
    wait(
        || desktop.windows().unwrap().iter().any(|w| w.x11_id == other),
        "other-workspace window not discovered",
    );
    let target = desktop
        .windows()
        .unwrap()
        .into_iter()
        .find(|w| w.x11_id == other)
        .unwrap();
    assert_eq!(target.workspace, Some(1));
    desktop.focus(&target).unwrap();
    assert_eq!(desktop.active_id().unwrap(), Some(other));
    let current = client
        .get_property(
            false,
            root,
            atom("_NET_CURRENT_DESKTOP"),
            AtomEnum::CARDINAL,
            0,
            1,
        )
        .unwrap()
        .reply()
        .unwrap();
    assert_eq!(current.value32().unwrap().next(), Some(1));
    let minimize = ClientMessageEvent::new(32, other, atom("WM_CHANGE_STATE"), [3, 0, 0, 0, 0]);
    client
        .send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            minimize,
        )
        .unwrap()
        .check()
        .unwrap();
    wait(
        || {
            desktop
                .windows()
                .unwrap()
                .iter()
                .any(|w| w.x11_id == other && w.minimized)
        },
        "window did not minimize",
    );
    desktop.focus(&target).unwrap();
    wait(
        || {
            desktop
                .windows()
                .unwrap()
                .iter()
                .any(|w| w.x11_id == other && w.focused && !w.minimized)
        },
        "activation did not restore minimized window",
    );
    let dock = client.generate_id().unwrap();
    create(dock, "Not an application", 1, "_NET_WM_WINDOW_TYPE_DOCK");
    assert!(!desktop.windows().unwrap().iter().any(|w| w.x11_id == dock));
    client.destroy_window(id).unwrap().check().unwrap();
    // Reuse precisely the same XID, class and title in the same client. This must
    // invalidate the old generation even if client-list snapshots never saw a gap.
    create(id, "Éditeur — test", 0, "_NET_WM_WINDOW_TYPE_NORMAL");
    assert!(
        desktop.focus(&original).is_err(),
        "stale snapshot activated reused XID"
    );
    wait(
        || desktop.windows().unwrap().iter().any(|w| w.x11_id == id),
        "reused window not discovered",
    );
    #[cfg(feature = "gui")]
    assert!(
        desktop
            .apply_action(&original, actions::Action::Fill)
            .is_err()
    );
    let replacement = desktop
        .windows()
        .unwrap()
        .into_iter()
        .find(|w| w.x11_id == id)
        .unwrap();
    assert_ne!(original.identity, replacement.identity);
    desktop.focus(&replacement).unwrap();
    assert_eq!(desktop.active_id().unwrap(), Some(id));
    client.destroy_window(id).unwrap().check().unwrap();
    assert!(desktop.focus(&replacement).is_err());

    // Once replacement is observed, restoring a former root property must not
    // revive session continuity on the old Desktop connection.
    let property = atom("_NET_SUPPORTING_WM_CHECK");
    let old_manager = client
        .get_property(false, root, property, AtomEnum::WINDOW, 0, 1)
        .unwrap()
        .reply()
        .unwrap()
        .value32()
        .unwrap()
        .next()
        .unwrap();
    let fake_manager = client.generate_id().unwrap();
    client
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            fake_manager,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new(),
        )
        .unwrap()
        .check()
        .unwrap();
    client
        .change_property32(
            PropMode::REPLACE,
            fake_manager,
            property,
            AtomEnum::WINDOW,
            &[fake_manager],
        )
        .unwrap()
        .check()
        .unwrap();
    client
        .change_property32(
            PropMode::REPLACE,
            root,
            property,
            AtomEnum::WINDOW,
            &[fake_manager],
        )
        .unwrap()
        .check()
        .unwrap();
    assert!(desktop.windows().is_err());
    client
        .change_property32(
            PropMode::REPLACE,
            root,
            property,
            AtomEnum::WINDOW,
            &[old_manager],
        )
        .unwrap()
        .check()
        .unwrap();
    assert!(
        desktop.windows().is_err(),
        "lost continuity must remain invalid"
    );
    assert!(x11::Desktop::connect().unwrap().windows().is_ok());
}
