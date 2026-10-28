//! Conservative desktop-entry/X11 matching. Never guess when the best rank ties.
use crate::{applications::Application, windows::Window};

fn rank(
    id: &str,
    wm_class: Option<&str>,
    executable: Option<&str>,
    class: Option<&str>,
    instance: Option<&str>,
) -> Option<u8> {
    let matches = |value: &str| {
        if value.is_empty() {
            None
        } else if class.is_some_and(|candidate| candidate.eq_ignore_ascii_case(value)) {
            Some(0)
        } else if instance.is_some_and(|candidate| candidate.eq_ignore_ascii_case(value)) {
            Some(1)
        } else {
            None
        }
    };
    if let Some(score) = wm_class.and_then(matches) {
        return Some(score);
    }
    let stem = id.strip_suffix(".desktop").unwrap_or(id);
    if let Some(score) = matches(stem) {
        return Some(2 + score);
    }
    if let Some(score) = stem.rsplit('.').next().and_then(matches) {
        return Some(4 + score);
    }
    executable.and_then(matches).map(|score| 6 + score)
}

pub fn matching_app<'a>(window: &Window, apps: &'a [Application]) -> Option<&'a Application> {
    let mut best = None;
    let mut best_rank = u8::MAX;
    let mut tied = false;
    for app in apps {
        let Some(rank) = rank(
            &app.id,
            app.wm_class.as_deref(),
            app.executable.as_deref(),
            window.class.as_deref(),
            window.instance.as_deref(),
        ) else {
            continue;
        };
        if rank < best_rank {
            best = Some(app);
            best_rank = rank;
            tied = false;
        } else if rank == best_rank {
            tied = true;
        }
    }
    if tied { None } else { best }
}

pub fn next_window(mut windows: Vec<&Window>, current: Option<u64>) -> Option<&Window> {
    windows.sort_by_key(|window| window.id);
    let next = current
        .and_then(|id| windows.iter().position(|window| window.id == id))
        .map_or(0, |index| (index + 1) % windows.len());
    windows.get(next).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gio;

    fn app(id: &str, wm: Option<&str>) -> Application {
        Application {
            id: id.into(),
            name: id.into(),
            wm_class: wm.map(str::to_owned),
            executable: None,
            info: gio::AppInfo::create_from_commandline(
                "/bin/true",
                Some(id),
                gio::AppInfoCreateFlags::NONE,
            )
            .unwrap(),
        }
    }

    fn window(id: u64, class: &str) -> Window {
        Window {
            id,
            x11_id: id + 100,
            title: String::new(),
            class: Some(class.into()),
            instance: None,
            role: None,
            workspace: None,
            output: None,
            focused: false,
            urgent: false,
            scratchpad: false,
        }
    }

    #[test]
    fn matching_prefers_startup_wm_class_and_refuses_ambiguous_matches() {
        let target = window(1, "Editor");
        let apps = vec![
            app("Editor.desktop", None),
            app("custom.desktop", Some("editor")),
        ];
        assert_eq!(matching_app(&target, &apps).unwrap().id, "custom.desktop");
        let apps = vec![
            app("org.one.Editor.desktop", None),
            app("org.two.Editor.desktop", None),
        ];
        assert!(matching_app(&target, &apps).is_none());
        assert!(matching_app(&window(1, "unknown"), &apps).is_none());
    }

    #[test]
    fn desktop_id_and_executable_fallbacks_are_case_insensitive() {
        assert_eq!(
            rank(
                "org.gnome.Nautilus.desktop",
                None,
                None,
                Some("Org.Gnome.Nautilus"),
                None
            ),
            Some(2)
        );
        assert_eq!(
            rank(
                "org.gnome.Nautilus.desktop",
                None,
                None,
                Some("Nautilus"),
                None
            ),
            Some(4)
        );
        assert_eq!(
            rank(
                "other.desktop",
                None,
                Some("vivaldi-stable"),
                None,
                Some("Vivaldi-stable")
            ),
            Some(7)
        );
        assert!(rank("other.desktop", Some(""), Some(""), Some(""), None).is_none());
    }

    #[test]
    fn explicit_class_beats_inherited_instance() {
        let mut target = window(1, "CustomTerminal");
        target.instance = Some("xterm".into());
        let apps = vec![
            app("xterm.desktop", Some("XTerm")),
            app("custom.desktop", Some("CustomTerminal")),
        ];
        assert_eq!(matching_app(&target, &apps).unwrap().id, "custom.desktop");
    }

    #[test]
    fn cycles_stably_and_wraps_without_dividing_by_zero() {
        let a = window(10, "App");
        let b = window(20, "App");
        assert!(next_window(vec![], None).is_none());
        assert!(next_window(vec![], Some(10)).is_none());
        assert_eq!(next_window(vec![&b, &a], None).unwrap().id, 10);
        assert_eq!(next_window(vec![&a, &b], Some(10)).unwrap().id, 20);
        assert_eq!(next_window(vec![&a, &b], Some(20)).unwrap().id, 10);
        assert_eq!(next_window(vec![&a], Some(10)).unwrap().id, 10);
    }
}
