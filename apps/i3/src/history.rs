use crate::{ipc::Result, storage};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Record {
    pub count: u64,
    pub latest: u64,
}

#[derive(Default, Deserialize, Serialize)]
pub struct History {
    pub apps: BTreeMap<String, Record>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn path() -> Result<PathBuf> {
    let base = match env::var_os("XDG_STATE_HOME").filter(|p| Path::new(p).is_absolute()) {
        Some(base) => PathBuf::from(base),
        None => PathBuf::from(env::var_os("HOME").ok_or("HOME is not set")?).join(".local/state"),
    };
    Ok(base.join("lite-switch/usage.json"))
}

impl History {
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)
                .map_err(|e| format!("cannot read usage history {}: {e}", path.display()))?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn record(&mut self, id: &str, path: &Path) -> Result<()> {
        let previous = self.apps.remove(id);
        let next = Record {
            count: previous.as_ref().map_or(1, |r| r.count.saturating_add(1)),
            latest: now(),
        };
        self.apps.insert(id.to_owned(), next);
        if let Err(e) = self.save(path) {
            self.apps.remove(id);
            if let Some(previous) = previous {
                self.apps.insert(id.to_owned(), previous);
            }
            return Err(e);
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<()> {
        storage::write_json(path, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn history_round_trip_and_private_permissions() {
        let dir = env::temp_dir().join(format!("lite-switch-history-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("usage.json");
        let mut history = History::load(&path).unwrap();
        history.record("editor.desktop", &path).unwrap();
        history.record("editor.desktop", &path).unwrap();
        assert_eq!(
            History::load(&path).unwrap().apps["editor.desktop"].count,
            2
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::write(&path, b"invalid").unwrap();
        assert!(History::load(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"invalid");
        fs::remove_dir_all(dir).unwrap();
    }
}
