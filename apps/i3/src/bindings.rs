//! i3 include generation and proposal validation.
pub mod managed;
mod priority;
pub(crate) mod source;
mod transaction;
use crate::{
    assignments::Book,
    ipc::{self, Result},
    settings::Settings,
};
use std::{
    env, fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const HEADER: &str = "# Lite Switch managed bindings v1\n";

pub fn launcher_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("LITE_SWITCH_LAUNCHER") {
        return Ok(path.into());
    }
    if let Some(home) = env::var_os("HOME") {
        let installed = PathBuf::from(home).join(".local/bin/lite-switch");
        if installed.is_file() {
            return Ok(installed);
        }
    }
    Ok(env::current_exe()?)
}

pub(crate) fn shell_quote(path: &Path) -> Result<String> {
    let path = path.to_str().ok_or("path must be UTF-8")?;
    if !Path::new(path).is_absolute() || path.contains(['\0', '\n', '\r']) {
        return Err("path must be absolute and contain no control characters".into());
    }
    Ok(format!("'{}'", path.replace('\'', "'\\''")))
}

fn command(path: &Path, arguments: &str) -> Result<String> {
    let shell = format!("{} {arguments}", shell_quote(path)?);
    // Protect commas and semicolons from i3 as well as quoting for the shell.
    Ok(format!(
        "\"{}\"",
        shell.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

pub fn render(book: &Book, settings: &Settings, launcher: &Path) -> Result<String> {
    let mut lines = vec![
        HEADER.trim_end().to_owned(),
        "# Generated; review/approve configuration integration separately.".to_owned(),
        format!(
            "bindsym {} exec --no-startup-id {}",
            settings.launcher_shortcut.value(),
            command(launcher, "toggle")?
        ),
    ];
    for key in book.targets.keys() {
        let chord = format!("{}+{}", settings.assignment_modifier.value(), key.value());
        if chord == settings.launcher_shortcut.value() {
            return Err("launcher shortcut conflicts with an assigned target".into());
        }
        lines.push(format!(
            "bindsym {chord} exec --no-startup-id {}",
            command(launcher, &format!("activate {}", key.value()))?
        ));
    }
    Ok(format!("{}\n", lines.join("\n")))
}

pub(crate) fn config_path(socket: &Path) -> Result<PathBuf> {
    let version: serde_json::Value = ipc::request(socket, ipc::GET_VERSION, "")?;
    let path = version["loaded_config_file_name"]
        .as_str()
        .ok_or("i3 did not report its config path")?;
    let path = PathBuf::from(path);
    shell_quote(&path)?;
    Ok(path)
}

pub(crate) struct PrivateDir(pub PathBuf);
impl PrivateDir {
    pub fn new(prefix: &str) -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = env::temp_dir().join(format!(
            "lite-switch-{prefix}-{}-{nonce}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        Ok(Self(directory))
    }
}
impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn validate_config(config: &Path) -> Result<()> {
    let output = std::process::Command::new("i3")
        .args(["-C", "-c"])
        .arg(config)
        .output()?;
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() || diagnostics.to_lowercase().contains("duplicate keybinding") {
        return Err(format!("i3 rejected the configuration:\n{diagnostics}").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        assignments::{Assignment, Key},
        settings::{Modifier, Shortcut},
    };
    #[test]
    fn limited_keys_configurable_shortcuts_and_executable_quoting() {
        let mut book = Book::default();
        book.targets.insert(
            Key::try_from("A".to_owned()).unwrap(),
            Assignment::Application {
                desktop_id: "editor.desktop".into(),
                launch_if_needed: false,
            },
        );
        let output = render(
            &book,
            &Settings::default(),
            Path::new("/tmp/test ' quote;comma,/lite-switch"),
        )
        .unwrap();
        assert!(output.contains("bindsym Mod1+space"));
        assert!(output.contains("bindsym Mod1+a"));
        assert!(!output.contains("Mod1+b"));
        assert!(output.contains("'\\\\''"));
        assert!(render(&book, &Settings::default(), Path::new("relative")).is_err());
        let mut settings = Settings {
            launcher_shortcut: Shortcut::try_from("Control+Alt+space".to_owned()).unwrap(),
            assignment_modifier: Modifier::Mod4,
            ..Settings::default()
        };
        let output = render(&book, &settings, Path::new("/tmp/launcher")).unwrap();
        assert!(output.contains("bindsym Control+Mod1+space"));
        assert!(output.contains("bindsym Mod4+a"));
        settings.launcher_shortcut = Shortcut::try_from("Super+a".to_owned()).unwrap();
        assert!(render(&book, &settings, Path::new("/tmp/launcher")).is_err());
    }
}
