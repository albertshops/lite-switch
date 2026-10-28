use crate::{ipc::Result, storage, windows::Window};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Key(String);

impl TryFrom<String> for Key {
    type Error = String;
    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        let lower = value.to_ascii_lowercase();
        if lower.len() == 1 && lower.as_bytes()[0].is_ascii_alphanumeric() {
            return Ok(Self(lower));
        }
        let key = match lower.as_str() {
            "left" | "arrowleft" => "Left",
            "right" | "arrowright" => "Right",
            "up" | "arrowup" => "Up",
            "down" | "arrowdown" => "Down",
            _ => return Err("use an ASCII letter, number, Left, Right, Up, or Down".into()),
        };
        Ok(Self(key.to_owned()))
    }
}

impl From<Key> for String {
    fn from(key: Key) -> Self {
        key.0
    }
}

impl Key {
    pub fn value(&self) -> &str {
        &self.0
    }
    pub fn label(&self, modifier: &str) -> String {
        format!("{modifier}+{}", self.0.to_uppercase())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Assignment {
    Application {
        desktop_id: String,
        #[serde(default)]
        launch_if_needed: bool,
    },
    Window {
        container_id: u64,
        x11_id: u64,
    },
}

impl Assignment {
    pub fn window(window: &Window) -> Self {
        Self::Window {
            container_id: window.id,
            x11_id: window.x11_id,
        }
    }

    pub fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Application { desktop_id: a, .. }, Self::Application { desktop_id: b, .. }) => {
                a == b
            }
            _ => self == other,
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Application { desktop_id, .. }
                if desktop_id.is_empty() || desktop_id.contains(['\n', '\r', '\0']) =>
            {
                Err("invalid desktop-entry ID in assignments".into())
            }
            Self::Window {
                container_id,
                x11_id,
            } if *container_id == 0 || *x11_id == 0 => Err("window IDs must be positive".into()),
            _ => Ok(()),
        }
    }

    fn persistent(&self) -> bool {
        matches!(self, Self::Application { .. })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    assignments: BTreeMap<Key, Assignment>,
}

#[derive(Debug, Clone, Default)]
pub struct Book {
    pub targets: BTreeMap<Key, Assignment>,
}

pub fn path() -> Result<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or("HOME is not set")?;
    Ok(base.join("lite-switch/assignments.json"))
}

impl Book {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let file: File = serde_json::from_slice(&bytes)?;
        if file.version != 1 {
            return Err("unsupported assignment file version".into());
        }
        if file.assignments.values().any(|target| !target.persistent()) {
            return Err(
                "window assignments must not be persisted; remove them from assignments.json"
                    .into(),
            );
        }
        let mut book = Self::default();
        for (key, target) in file.assignments {
            target.validate()?;
            if book.key_for(&target).is_some() {
                return Err("duplicate target in assignment file".into());
            }
            book.targets.insert(key, target);
        }
        Ok(book)
    }

    pub fn key_for(&self, target: &Assignment) -> Option<&Key> {
        self.targets
            .iter()
            .find(|(_, candidate)| candidate.matches(target))
            .map(|(key, _)| key)
    }

    pub fn assign(&mut self, path: &Path, key: Key, target: Assignment) -> Result<()> {
        target.validate()?;
        self.change(path, |book| {
            book.targets
                .retain(|_, candidate| !candidate.matches(&target));
            book.targets.insert(key, target);
        })
    }

    pub fn remove_key(&mut self, path: &Path, key: &Key) -> Result<()> {
        self.change(path, |book| {
            book.targets.remove(key);
        })
    }

    pub fn set_launch(&mut self, path: &Path, id: &str, enabled: bool) -> Result<()> {
        self.change(path, |book| {
            for target in book.targets.values_mut() {
                if let Assignment::Application {
                    desktop_id,
                    launch_if_needed,
                } = target
                    && desktop_id == id
                {
                    *launch_if_needed = enabled;
                }
            }
        })
    }

    fn change(&mut self, path: &Path, change: impl FnOnce(&mut Self)) -> Result<()> {
        let previous = self.clone();
        change(self);
        if let Err(e) = self.save(path) {
            *self = previous;
            return Err(e);
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<()> {
        let file = File {
            version: 1,
            assignments: self
                .targets
                .iter()
                .filter(|(_, target)| target.persistent())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        };
        storage::write_json(path, &file)
    }

    pub fn excludes(&self, window: &Window) -> bool {
        self.targets
            .values()
            .any(|target| target == &Assignment::window(window))
    }

    pub fn close_window(&mut self, id: u64, xid: u64) {
        self.targets.retain(|_, target| {
            target
                != &Assignment::Window {
                    container_id: id,
                    x11_id: xid,
                }
        });
    }

    pub fn clear_session(&mut self) {
        self.targets.retain(|_, target| target.persistent());
    }

    pub fn prune(&mut self, live: &[Window]) {
        self.targets.retain(|_, target| match target {
            Assignment::Window {
                container_id,
                x11_id,
            } => live
                .iter()
                .any(|w| w.id == *container_id && w.x11_id == *x11_id),
            _ => true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_normalizes_shortcut_keys() {
        for value in ["A", "z", "0", "9", "Left", "arrowRight", "Up", "Down"] {
            assert!(Key::try_from(value.to_owned()).is_ok());
        }
        for value in ["", "ab", "é", "space", "Return", "a;kill", "+"] {
            assert!(Key::try_from(value.to_owned()).is_err());
        }
        assert_eq!(Key::try_from("A".to_owned()).unwrap().value(), "a");
        assert!(
            serde_json::from_str::<BTreeMap<Key, Assignment>>(
                r#"{";kill":{"type":"window","container_id":1,"x11_id":2}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_invalid_targets() {
        assert!(
            Assignment::Application {
                desktop_id: "bad\nentry".into(),
                launch_if_needed: false
            }
            .validate()
            .is_err()
        );
        assert!(
            Assignment::Window {
                container_id: 0,
                x11_id: 1
            }
            .validate()
            .is_err()
        );
        assert!(
            Assignment::Window {
                container_id: 1,
                x11_id: 0
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn persistent_and_session_assignments_and_transactional_failure() {
        let dir = env::temp_dir().join(format!("lite-switch-assignments-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("assignments.json");
        let a = Key::try_from("a".to_owned()).unwrap();
        let b = Key::try_from("b".to_owned()).unwrap();
        let app = Assignment::Application {
            desktop_id: "editor.desktop".into(),
            launch_if_needed: false,
        };
        let window = Assignment::Window {
            container_id: 42,
            x11_id: 99,
        };
        let mut book = Book::load(&path).unwrap();
        book.assign(&path, a.clone(), app.clone()).unwrap();
        book.assign(&path, b.clone(), app.clone()).unwrap();
        assert!(!book.targets.contains_key(&a));
        book.set_launch(&path, "editor.desktop", true).unwrap();
        book.assign(&path, a.clone(), window.clone()).unwrap();
        assert_eq!(book.targets.len(), 2);
        let persisted = Book::load(&path).unwrap();
        assert_eq!(persisted.targets.len(), 1);
        assert!(matches!(
            persisted.targets[&b],
            Assignment::Application {
                launch_if_needed: true,
                ..
            }
        ));
        assert!(!fs::read_to_string(&path).unwrap().contains("window"));
        fs::write(dir.join("blocker"), b"not a directory").unwrap();
        assert!(
            book.assign(&dir.join("blocker/assignments.json"), a.clone(), app)
                .is_err()
        );
        assert_eq!(book.targets[&a], window);
        book.close_window(42, 100);
        assert!(book.targets.contains_key(&a));
        book.close_window(42, 99);
        assert!(!book.targets.contains_key(&a));
        book.assign(&path, a.clone(), window).unwrap();
        book.prune(&[]);
        assert!(!book.targets.contains_key(&a));
        book.remove_key(&path, &b).unwrap();
        assert!(book.targets.is_empty());
        for invalid in [
            r#"{"version":2,"assignments":{}}"#,
            r#"{"version":1,"assignments":{"a":{"type":"window","container_id":42,"x11_id":99}}}"#,
            r#"{"version":1,"assignments":{"a":{"type":"application","desktop_id":"same"},"b":{"type":"application","desktop_id":"same"}}}"#,
            "invalid",
        ] {
            fs::write(&path, invalid).unwrap();
            assert!(Book::load(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
