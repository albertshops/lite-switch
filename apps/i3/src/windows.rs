use crate::ipc::{self, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Deserialize)]
pub struct Node {
    pub id: u64,
    pub name: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: String,
    pub window: Option<u64>,
    pub window_properties: Option<WindowProperties>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub urgent: bool,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub floating_nodes: Vec<Node>,
}

#[derive(Debug, Default, Deserialize)]
pub struct WindowProperties {
    pub class: Option<String>,
    pub instance: Option<String>,
    pub title: Option<String>,
    pub window_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Window {
    pub id: u64,
    pub x11_id: u64,
    pub title: String,
    pub class: Option<String>,
    pub instance: Option<String>,
    pub role: Option<String>,
    pub workspace: Option<String>,
    pub output: Option<String>,
    pub focused: bool,
    pub urgent: bool,
    pub scratchpad: bool,
}

pub fn windows(root: &Node) -> Vec<Window> {
    let mut result = Vec::new();
    collect(root, None, None, &mut result);
    result
}

fn collect(node: &Node, workspace: Option<&str>, output: Option<&str>, result: &mut Vec<Window>) {
    // Bars and other docks are X11 windows, but never launcher targets.
    if node.kind == "dockarea" {
        return;
    }
    let workspace = if node.kind == "workspace" {
        node.name.as_deref()
    } else {
        workspace
    };
    let output = if node.kind == "output" {
        node.name.as_deref()
    } else {
        output
    };
    if let Some(x11_id) = node.window.filter(|id| *id != 0) {
        let properties = node.window_properties.as_ref();
        result.push(Window {
            id: node.id,
            x11_id,
            title: node
                .name
                .as_ref()
                .or_else(|| properties.and_then(|p| p.title.as_ref()))
                .cloned()
                .unwrap_or_default(),
            class: properties.and_then(|p| p.class.clone()),
            instance: properties.and_then(|p| p.instance.clone()),
            role: properties.and_then(|p| p.window_role.clone()),
            workspace: workspace.map(str::to_owned),
            output: output.map(str::to_owned),
            focused: node.focused,
            urgent: node.urgent,
            scratchpad: workspace == Some("__i3_scratch"),
        });
    }
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        collect(child, workspace, output, result);
    }
}

/// All words must match the title, class, or instance. Keep i3 traversal order.
/// App ranking/usage history is intentionally separate from this diagnostic filter.
pub fn filter_windows(windows: Vec<Window>, query: &str) -> Vec<Window> {
    let query = query.to_lowercase();
    let words: Vec<_> = query.split_whitespace().collect();
    windows
        .into_iter()
        .filter(|window| {
            let text = format!(
                "{} {} {}",
                window.title,
                window.class.as_deref().unwrap_or(""),
                window.instance.as_deref().unwrap_or("")
            )
            .to_lowercase();
            words.iter().all(|word| text.contains(word))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct CommandReply {
    success: bool,
    error: Option<String>,
}

pub fn focus(path: &Path, id: u64) -> Result<()> {
    focus_target(path, id, None)
}

#[cfg(feature = "gui")]
pub fn focus_window(path: &Path, window: &Window) -> Result<()> {
    focus_target(path, window.id, Some(window.x11_id))
}

fn focus_target(path: &Path, id: u64, expected_x11_id: Option<u64>) -> Result<()> {
    let root: Node = ipc::request(path, ipc::GET_TREE, "")?;
    let target = windows(&root)
        .into_iter()
        .find(|window| window.id == id)
        .ok_or_else(|| format!("window container {id} no longer exists"))?;
    if expected_x11_id.is_some_and(|expected| expected != target.x11_id) {
        return Err(format!("window container {id} changed identity").into());
    }
    // Both IDs are numeric. Matching both protects against a container being
    // replaced between the tree query and command execution.
    let criteria = format!("[con_id={id} id={}]", target.x11_id);
    let command = if target.scratchpad {
        format!("{criteria} scratchpad show; {criteria} focus")
    } else {
        format!("{criteria} focus")
    };
    let replies: Vec<CommandReply> = ipc::request(path, ipc::COMMAND, &command)?;
    if replies.is_empty() {
        return Err("i3 returned an empty command response".into());
    }
    for reply in replies {
        if !reply.success {
            return Err(reply
                .error
                .unwrap_or_else(|| "i3 rejected focus command".into())
                .into());
        }
    }
    // Criteria can match nothing without i3 reporting a command failure.
    let root: Node = ipc::request(path, ipc::GET_TREE, "")?;
    if !windows(&root)
        .iter()
        .any(|window| window.id == id && window.x11_id == target.x11_id && window.focused)
    {
        return Err(
            format!("i3 did not focus window {id}; it may have closed or focus changed").into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Node {
        serde_json::from_str(include_str!("../tests/fixtures/tree.json")).unwrap()
    }

    #[test]
    fn traverses_nested_tiled_and_floating_windows() {
        let windows = windows(&tree());
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].id, 11);
        assert_eq!(windows[0].title, "Editor — project");
        assert_eq!(windows[0].workspace.as_deref(), Some("2: code"));
        assert_eq!(windows[0].output.as_deref(), Some("DP-1"));
        assert!(windows[0].focused);
        assert_eq!(windows[1].id, 12);
        assert!(windows[1].urgent);
        assert_eq!(windows[1].title, "Fallback title");
        assert!(windows[2].scratchpad);
    }

    #[test]
    fn filters_case_insensitively_across_title_and_properties() {
        assert_eq!(filter_windows(windows(&tree()), "EDITOR PROJECT")[0].id, 11);
        assert_eq!(
            filter_windows(windows(&tree()), "vivaldi fallback")[0].id,
            12
        );
        assert!(filter_windows(windows(&tree()), "no match").is_empty());
        assert_eq!(filter_windows(windows(&tree()), " ").len(), 3);
    }

    #[test]
    fn tolerates_missing_properties_and_empty_containers() {
        let node: Node =
            serde_json::from_str(r#"{"id":1,"nodes":[{"id":2,"window":42},{"id":3,"window":0}]}"#)
                .unwrap();
        assert_eq!(windows(&node).len(), 1);
        assert_eq!(windows(&node)[0].title, "");
        assert_eq!(windows(&node)[0].class, None);
    }
}
