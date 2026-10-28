//! Conservative desktop-entry/X11 matching. Never guess when the best rank ties.
use crate::{applications::Application, matching::Overrides, x11::Window};

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

fn best_candidates<'a>(
    window: &Window,
    apps: impl IntoIterator<Item = &'a Application>,
) -> Vec<&'a Application> {
    let mut best = Vec::new();
    let mut best_rank = u8::MAX;
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
            best.clear();
            best_rank = rank;
        }
        if rank == best_rank {
            best.push(app);
        }
    }
    best
}

fn matching_candidates<'a>(
    window: &Window,
    apps: impl IntoIterator<Item = &'a Application>,
) -> Option<&'a Application> {
    let mut candidates = best_candidates(window, apps);
    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}

fn matching_app<'a>(window: &Window, apps: &'a [Application]) -> Option<&'a Application> {
    matching_candidates(window, apps)
}

pub fn matched_app<'a>(
    window: &Window,
    apps: &'a [Application],
    overrides: &Overrides,
) -> Option<&'a Application> {
    if overrides.rules.is_empty() {
        return matching_app(window, apps);
    }
    let mut pinned = overrides
        .rules
        .iter()
        .filter(|(_, rule)| rule.matches(window));
    if let Some((id, _)) = pinned.next() {
        if pinned.next().is_some() {
            return None;
        }
        // A pin to an uninstalled entry remains unmatched, not claimed by an
        // unrelated heuristic. Explicit rules take precedence over all ranks.
        return apps.iter().find(|app| &app.id == id);
    }
    matching_candidates(
        window,
        apps.iter()
            .filter(|app| !overrides.rules.contains_key(&app.id)),
    )
}

pub fn ambiguous_for(
    window: &Window,
    apps: &[Application],
    overrides: &Overrides,
    id: &str,
) -> bool {
    let pinned: Vec<_> = overrides
        .rules
        .iter()
        .filter(|(_, rule)| rule.matches(window))
        .map(|(id, _)| id.as_str())
        .collect();
    if !pinned.is_empty() {
        return pinned.len() > 1 && pinned.contains(&id);
    }
    let best = best_candidates(
        window,
        apps.iter()
            .filter(|app| !overrides.rules.contains_key(&app.id)),
    );
    best.len() > 1 && best.iter().any(|app| app.id == id)
}

pub fn next_window(mut windows: Vec<&Window>, current: Option<u32>) -> Option<&Window> {
    windows.sort_by_key(|window| window.x11_id);
    let next = current
        .and_then(|id| windows.iter().position(|window| window.x11_id == id))
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

    fn window(id: u32, class: &str) -> Window {
        Window {
            x11_id: id,
            identity: u64::from(id),
            class: Some(class.into()),
            ..Window::default()
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
    fn explicit_overrides_win_and_ambiguous_or_narrowed_rules_do_not_guess() {
        let mut target = window(1, "Editor");
        target.instance = Some("project".into());
        let apps = vec![
            app("a.desktop", Some("Editor")),
            app("b.desktop", Some("Editor")),
        ];
        let mut rules = Overrides::default();
        assert!(matched_app(&target, &apps, &rules).is_none());
        assert!(ambiguous_for(&target, &apps, &rules, "a.desktop"));
        rules.rules.insert(
            "a.desktop".into(),
            crate::matching::parse(&["--class", "Editor"]).unwrap(),
        );
        assert_eq!(matched_app(&target, &apps, &rules).unwrap().id, "a.desktop");
        rules.rules.insert(
            "b.desktop".into(),
            crate::matching::parse(&["--instance", "project"]).unwrap(),
        );
        assert!(matched_app(&target, &apps, &rules).is_none());
        rules.rules.remove("b.desktop");
        rules.rules.get_mut("a.desktop").unwrap().instance = Some("other".into());
        assert_eq!(matched_app(&target, &apps, &rules).unwrap().id, "b.desktop");
        rules.rules.remove("a.desktop");
        rules.rules.insert(
            "gone.desktop".into(),
            crate::matching::parse(&["--class", "Editor"]).unwrap(),
        );
        assert!(matched_app(&target, &apps, &rules).is_none());
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
        assert_eq!(next_window(vec![&b, &a], None).unwrap().x11_id, 10);
        assert_eq!(next_window(vec![&a, &b], Some(10)).unwrap().x11_id, 20);
        assert_eq!(next_window(vec![&a, &b], Some(20)).unwrap().x11_id, 10);
        assert_eq!(next_window(vec![&a], Some(10)).unwrap().x11_id, 10);
    }
}
