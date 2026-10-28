//! Xfce shortcut proposals and approval-gated property transactions.
mod keyboard;
pub mod managed;
mod plan;
mod transaction;
mod xfconf;

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Shortcut {
    pub shortcut: String,
    pub command: String,
}
use crate::{assignments::Book, settings::Settings, x11::Result};
use std::path::{Path, PathBuf};

pub fn launcher_path() -> Result<PathBuf> {
    Ok(std::env::var_os("LITE_SWITCH_LAUNCHER")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_exe()?))
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn xfce_chord(chord: &str) -> String {
    let mut parts: Vec<_> = chord.split('+').collect();
    let key = parts.pop().unwrap_or("");
    let modifiers: String = parts
        .into_iter()
        .map(|part| match part {
            "Mod1" => "<Alt>",
            "Mod4" => "<Super>",
            "Control" => "<Primary>",
            "Shift" => "<Shift>",
            _ => "",
        })
        .collect();
    format!("{modifiers}{key}")
}
fn proposal(book: &Book, settings: &Settings, launcher: &Path) -> Result<Vec<Shortcut>> {
    let launcher = launcher.to_str().ok_or("launcher path must be UTF-8")?;
    if !Path::new(launcher).is_absolute() || launcher.contains(['\n', '\r', '\0']) {
        return Err("launcher must be an absolute safe path".into());
    }
    let parts: Vec<_> = settings.launcher_shortcut.value().split('+').collect();
    if parts.contains(&"Control")
        && parts.contains(&"Mod1")
        && parts.last().is_some_and(|key| {
            key.strip_prefix('F')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (1..=12).contains(&n))
        })
    {
        return Err(
            "Ctrl+Alt+F1–F12 are Xorg VT-switch combinations, not supported launcher shortcuts"
                .into(),
        );
    }
    let launcher_chord = xfce_chord(settings.launcher_shortcut.value());
    let mut proposal = vec![Shortcut {
        shortcut: launcher_chord.clone(),
        command: format!("{} toggle", quote(launcher)),
    }];
    for key in book.targets.keys() {
        let chord = xfce_chord(&format!(
            "{}+{}",
            settings.assignment_modifier.value(),
            key.value()
        ));
        if chord == launcher_chord {
            return Err("launcher chord conflicts with a direct assignment".into());
        }
        proposal.push(Shortcut {
            shortcut: chord,
            command: format!("{} activate {}", quote(launcher), key.value()),
        });
    }
    Ok(proposal)
}

pub fn render(book: &Book, settings: &Settings, launcher: &Path) -> Result<String> {
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&proposal(book, settings, launcher)?)?
    ))
}

#[cfg(test)]
pub struct PrivateDir(pub PathBuf);
#[cfg(test)]
impl PrivateDir {
    pub fn new(label: &str) -> Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "lite-switch-xfce-{label}-{}-{now}",
            std::process::id()
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}
#[cfg(test)]
impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_exports_xfce_chords_and_quotes_without_installing() {
        let text = render(
            &Book::default(),
            &Settings::default(),
            Path::new("/tmp/a ' launcher"),
        )
        .unwrap();
        let entries: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(entries[0]["shortcut"], "<Alt>space");
        assert_eq!(entries[0]["command"], "'/tmp/a '\\'' launcher' toggle");
        assert!(
            render(
                &Book::default(),
                &Settings::default(),
                Path::new("bad\npath")
            )
            .is_err()
        );
        let settings = Settings {
            launcher_shortcut: crate::settings::Shortcut::try_from("Control+Alt+F1".to_owned())
                .unwrap(),
            ..Settings::default()
        };
        assert!(
            render(&Book::default(), &settings, Path::new("/bin/launcher"))
                .unwrap_err()
                .to_string()
                .contains("VT-switch")
        );
    }
}
