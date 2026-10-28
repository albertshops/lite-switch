//! File transaction with private recovery snapshots and optimistic edit checks.
use crate::{ipc::Result, storage};
use serde::Serialize;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct Image {
    pub bytes: Vec<u8>,
    pub mode: u32,
}

impl Image {
    pub fn read(path: &Path) -> Result<Option<Self>> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if !metadata.is_file() {
            return Err(format!("refusing non-regular managed file: {}", path.display()).into());
        }
        Ok(Some(Self {
            bytes: fs::read(path)?,
            mode: metadata.permissions().mode() & 0o777,
        }))
    }
    pub fn text(text: &str, mode: u32) -> Self {
        Self {
            bytes: text.as_bytes().to_vec(),
            mode,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct Change {
    pub path: PathBuf,
    pub before: Option<Image>,
    pub after: Option<Image>,
}

impl Change {
    #[cfg(test)]
    pub fn capture(path: PathBuf, after: Option<Image>) -> Result<Self> {
        Ok(Self {
            before: Image::read(&path)?,
            path,
            after,
        })
    }
}

fn write(path: &Path, image: Option<&Image>) -> Result<()> {
    match image {
        Some(image) => storage::write_file(path, &image.bytes, image.mode),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        },
    }
}

fn rollback(changes: &[Change]) -> Result<()> {
    // Check *all* files before restoring any. Do not overwrite an outside edit.
    for change in changes {
        if Image::read(&change.path)? != change.after {
            return Err(format!("concurrent edit at {}; automatic rollback refused; recover from the backup manually", change.path.display()).into());
        }
    }
    for change in changes.iter().rev() {
        if change.before != change.after {
            write(&change.path, change.before.as_ref())?;
        }
    }
    Ok(())
}

pub(super) fn commit(
    changes: &[Change],
    backup: &Path,
    mut validate: impl FnMut() -> Result<()>,
    mut activate: impl FnMut(bool) -> Result<()>,
) -> Result<()> {
    #[derive(Serialize)]
    struct Backup<'a> {
        version: u32,
        changes: &'a [Change],
    }
    storage::write_json(
        backup,
        &Backup {
            version: 1,
            changes,
        },
    )?;
    let mut applied = 0;
    let mut activation_attempted = false;
    let result = (|| -> Result<()> {
        for change in changes {
            if Image::read(&change.path)? != change.before {
                return Err(format!(
                    "concurrent edit at {}; refusing to overwrite it",
                    change.path.display()
                )
                .into());
            }
            if change.before != change.after {
                write(&change.path, change.after.as_ref())?;
            }
            applied += 1;
        }
        validate()?;
        activation_attempted = true;
        activate(false)?;
        Ok(())
    })();
    if let Err(error) = result {
        let rollback_result = rollback(&changes[..applied]);
        if let Err(rollback_error) = rollback_result {
            return Err(format!(
                "{error}\nRollback failed: {rollback_error}\nRecovery backup: {}",
                backup.display()
            )
            .into());
        }
        if activation_attempted && let Err(reload_error) = activate(true) {
            return Err(format!("{error}\nFiles restored but reloading old bindings failed: {reload_error}\nRecovery backup: {}", backup.display()).into());
        }
        return Err(format!(
            "{error}\nChanges rolled back. Recovery backup: {}",
            backup.display()
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::PrivateDir;

    #[test]
    fn restores_files_if_validation_or_reload_fails() {
        for reload_fails in [false, true] {
            let dir = PrivateDir::new("transaction").unwrap();
            let root = dir.0.join("config");
            let include = dir.0.join("include");
            fs::write(&root, "original").unwrap();
            let changes = vec![
                Change::capture(root.clone(), Some(Image::text("new", 0o644))).unwrap(),
                Change::capture(include.clone(), Some(Image::text("new include", 0o600))).unwrap(),
            ];
            let backup = dir.0.join("backups/transaction.json");
            let mut activations = Vec::new();
            let result = commit(
                &changes,
                &backup,
                || {
                    if reload_fails {
                        Ok(())
                    } else {
                        Err("validation failed".into())
                    }
                },
                |restoring| {
                    activations.push(restoring);
                    if restoring {
                        Ok(())
                    } else {
                        Err("reload failed".into())
                    }
                },
            );
            assert!(result.is_err());
            assert_eq!(fs::read_to_string(root).unwrap(), "original");
            assert!(!include.exists());
            assert!(backup.exists());
            assert_eq!(
                fs::metadata(backup).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                activations,
                if reload_fails {
                    vec![false, true]
                } else {
                    vec![]
                }
            );
        }
    }

    #[test]
    fn rollback_does_not_rewrite_unchanged_source_guards() {
        use std::os::unix::fs::MetadataExt;
        let dir = PrivateDir::new("guard-rollback").unwrap();
        let guard = dir.0.join("unchanged-source");
        fs::write(&guard, "unchanged").unwrap();
        let inode = fs::metadata(&guard).unwrap().ino();
        let image = Image::read(&guard).unwrap().unwrap();
        let change = Change {
            path: guard.clone(),
            before: Some(image.clone()),
            after: Some(image),
        };
        assert!(
            commit(
                &[change],
                &dir.0.join("backup.json"),
                || Err("validation".into()),
                |_| panic!("must not reload")
            )
            .is_err()
        );
        assert_eq!(fs::metadata(guard).unwrap().ino(), inode);
    }

    #[test]
    fn restores_partial_apply_but_never_overwrites_a_concurrent_edit() {
        let dir = PrivateDir::new("transaction").unwrap();
        let root = dir.0.join("config");
        fs::write(&root, "original").unwrap();
        let changes = vec![
            Change::capture(root.clone(), Some(Image::text("new", 0o600))).unwrap(),
            Change::capture(
                dir.0.join("missing/child"),
                Some(Image::text("unwritable", 0o600)),
            )
            .unwrap(),
        ];
        assert!(
            commit(
                &changes,
                &dir.0.join("backup.json"),
                || panic!("must not validate"),
                |_| panic!("must not reload")
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&root).unwrap(), "original");
        let changes = &changes[..1];
        let result = commit(
            changes,
            &dir.0.join("backup2.json"),
            || {
                fs::write(&root, "outside edit")?;
                Err("failed after concurrent edit".into())
            },
            |_| panic!("must not reload"),
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("automatic rollback refused")
        );
        assert_eq!(fs::read_to_string(root).unwrap(), "outside edit");
    }
}
