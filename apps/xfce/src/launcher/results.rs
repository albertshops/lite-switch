//! Avoid presenting a running app twice while preserving explicit window search.
use crate::search;

pub(super) fn window_query(query: &str) -> Option<&str> {
    let query = query.trim_start();
    query
        .strip_prefix("windows:")
        .or_else(|| query.strip_prefix("window:"))
        .map(str::trim_start)
}

pub(super) fn show_window(
    query: &str,
    title: &str,
    class: &str,
    app_name: Option<&str>,
    pinned: bool,
) -> bool {
    let explicit = window_query(query);
    let text = explicit.unwrap_or(query);
    let matches =
        search::match_name(text, title).is_some() || search::match_name(text, class).is_some();
    matches
        && (explicit.is_some()
            || pinned
            || app_name.is_none()
            || (!search::normalized(query).is_empty()
                && app_name.is_some_and(|name| search::match_name(query, name).is_none())))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_and_app_queries_do_not_duplicate_known_apps() {
        for name in ["Vivaldi", "Terminal", "WhatsApp"] {
            assert!(!show_window("", name, name, Some(name), false));
            assert!(!show_window("  ", name, name, Some(name), false));
            assert!(!show_window(name, name, name, Some(name), false));
        }
        assert!(show_window("", "Unknown Window", "Unmatched", None, false));
        assert!(show_window(
            "",
            "Pinned Window",
            "Vivaldi",
            Some("Vivaldi"),
            true
        ));
    }
    #[test]
    fn specific_titles_and_explicit_mode_keep_window_assignment_available() {
        assert!(show_window(
            "Project Notes",
            "Project Notes — Vivaldi",
            "Vivaldi",
            Some("Vivaldi"),
            false
        ));
        assert!(show_window(
            "windows:",
            "Terminal",
            "Xfce4-terminal",
            Some("Terminal"),
            false
        ));
        assert!(show_window(
            "window: Terminal",
            "Terminal",
            "Xfce4-terminal",
            Some("Terminal"),
            false
        ));
        assert!(!show_window(
            "windows: Missing",
            "Terminal",
            "Xfce4-terminal",
            Some("Terminal"),
            false
        ));
        assert_eq!(window_query("  windows:  Vivaldi"), Some("Vivaldi"));
    }
}
