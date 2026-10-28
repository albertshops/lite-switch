use crate::{assignments, ipc::Result, storage};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modifier {
    Mod1,
    Mod4,
}

impl Modifier {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "mod1" | "alt" => Ok(Self::Mod1),
            "mod4" | "super" | "win" => Ok(Self::Mod4),
            _ => Err("assignment modifier must be Alt/Mod1 or Super/Mod4".into()),
        }
    }
    pub fn value(self) -> &'static str {
        match self {
            Self::Mod1 => "Mod1",
            Self::Mod4 => "Mod4",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Mod1 => "Alt",
            Self::Mod4 => "Super",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Shortcut(String);

impl TryFrom<String> for Shortcut {
    type Error = String;
    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        let mut parts: Vec<_> = value.split('+').collect();
        if parts.len() < 2 {
            return Err("launcher shortcut requires a modifier and key".into());
        }
        let key = parts.pop().unwrap().to_ascii_lowercase();
        let key = match key.as_str() {
            "space" => "space".to_owned(),
            "return" | "enter" => "Return".to_owned(),
            "tab" => "Tab".to_owned(),
            "escape" | "esc" => "Escape".to_owned(),
            value
                if value.starts_with('f')
                    && value[1..]
                        .parse::<u8>()
                        .is_ok_and(|n| (1..=12).contains(&n)) =>
            {
                format!("F{}", value[1..].parse::<u8>().unwrap())
            }
            _ => assignments::Key::try_from(key)?.value().to_owned(),
        };
        let mut modifiers = Vec::new();
        for part in parts {
            let modifier = match part.to_ascii_lowercase().as_str() {
                "alt" | "mod1" => "Mod1",
                "super" | "win" | "mod4" => "Mod4",
                "ctrl" | "control" => "Control",
                "shift" => "Shift",
                _ => return Err("use Alt/Mod1, Super/Mod4, Control, and/or Shift".into()),
            };
            if modifiers.contains(&modifier) {
                return Err("duplicate shortcut modifier".into());
            }
            modifiers.push(modifier);
        }
        if modifiers == ["Shift"] {
            return Err("Shift alone is not a safe launcher modifier".into());
        }
        modifiers.sort_unstable();
        Ok(Self(format!("{}+{key}", modifiers.join("+"))))
    }
}

impl From<Shortcut> for String {
    fn from(shortcut: Shortcut) -> Self {
        shortcut.0
    }
}
impl Shortcut {
    pub fn value(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub launcher_shortcut: Shortcut,
    pub assignment_modifier: Modifier,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            launcher_shortcut: Shortcut::try_from("Mod1+space".to_owned()).unwrap(),
            assignment_modifier: Modifier::Mod1,
        }
    }
}

pub fn path() -> Result<PathBuf> {
    Ok(assignments::path()?.with_file_name("settings.json"))
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(bytes) => {
                let settings: Self = serde_json::from_slice(&bytes)?;
                if settings.version != 1 {
                    return Err("unsupported settings file version".into());
                }
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        storage::write_json(path, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_round_trip_and_invalid_settings_are_not_overwritten() {
        use std::os::unix::fs::PermissionsExt;
        let directory = crate::bindings::PrivateDir::new("settings-test").unwrap();
        let path = directory.0.join("settings.json");
        assert_eq!(
            Settings::load(&path).unwrap().launcher_shortcut.value(),
            "Mod1+space"
        );
        let settings = Settings {
            launcher_shortcut: Shortcut::try_from("Super+space".to_owned()).unwrap(),
            assignment_modifier: Modifier::Mod4,
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(
            Settings::load(&path).unwrap().launcher_shortcut.value(),
            "Mod4+space"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        for invalid in [
            "invalid",
            r#"{"version":2,"launcher_shortcut":"Mod1+space","assignment_modifier":"Mod1"}"#,
            r#"{"version":1,"launcher_shortcut":"plain","assignment_modifier":"Mod1"}"#,
        ] {
            fs::write(&path, invalid).unwrap();
            assert!(Settings::load(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
        }
    }

    #[test]
    fn shortcut_validation_and_canonical_order() {
        assert_eq!(
            Shortcut::try_from("Shift+Super+Space".to_owned())
                .unwrap()
                .value(),
            "Mod4+Shift+space"
        );
        assert_eq!(
            Shortcut::try_from("Alt+Ctrl+Left".to_owned())
                .unwrap()
                .value(),
            "Control+Mod1+Left"
        );
        assert_eq!(
            Shortcut::try_from("Super+F12".to_owned()).unwrap().value(),
            "Mod4+F12"
        );
        assert_eq!(
            Shortcut::try_from("Super+F01".to_owned()).unwrap().value(),
            "Mod4+F1"
        );
        for value in [
            "space",
            "Shift+space",
            "Alt+Alt+a",
            "Alt+Super+;kill",
            "Alt+F13",
            "Alt+",
            "Hyper+a",
            "Alt+é",
        ] {
            assert!(Shortcut::try_from(value.to_owned()).is_err(), "{value}");
        }
        assert_eq!(Modifier::parse("super").unwrap(), Modifier::Mod4);
        assert!(Modifier::parse("Control").is_err());
    }
}
