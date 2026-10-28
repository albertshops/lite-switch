//! Managed Xfce shortcut lifecycle. Never edit xfconfd's XML behind its back.
use super::{
    keyboard::Keyboard,
    plan::{self, Manifest, Plan},
    proposal, transaction,
    xfconf::{Store, Xfconf},
};
use crate::{
    assignments::{self, Book},
    settings::Settings,
    storage,
    x11::Result,
};
use serde::Serialize;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

struct Paths {
    manifest: PathBuf,
    lock: PathBuf,
    backups: PathBuf,
}
impl Paths {
    fn new() -> Result<Self> {
        let base = assignments::path()?
            .parent()
            .ok_or("configuration path has no parent")?
            .to_owned();
        Ok(Self {
            manifest: base.join("xfce-binding-installation.json"),
            lock: base.join(".xfce-bindings.lock"),
            backups: base.join("backups"),
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Image {
    bytes: Vec<u8>,
    mode: u32,
}
fn image(path: &Path) -> Result<Option<Image>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() {
        return Err(format!("refusing non-regular manifest {}", path.display()).into());
    }
    if metadata.len() > 1024 * 1024 {
        return Err("binding manifest exceeds safe size".into());
    }
    Ok(Some(Image {
        bytes: {
            let mut bytes = Vec::new();
            fs::File::open(path)?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 {
                return Err("binding manifest exceeds safe size".into());
            }
            bytes
        },
        mode: metadata.permissions().mode() & 0o777,
    }))
}
fn load(image: Option<&Image>) -> Result<Option<Manifest>> {
    image
        .map(|image| serde_json::from_slice(&image.bytes).map_err(Into::into))
        .transpose()
}
fn nonce() -> Result<String> {
    Ok(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ))
}
struct Lock {
    path: PathBuf,
    contents: String,
}
impl Lock {
    fn acquire(paths: &Paths) -> Result<Self> {
        let parent = paths.lock.parent().ok_or("lock has no parent")?;
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        let contents = nonce()?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&paths.lock)
            .map_err(|error| {
                format!(
                    "cannot lock shortcut installation: {error}; check {} for a stale lock",
                    paths.lock.display()
                )
            })?;
        let lock = Self {
            path: paths.lock.clone(),
            contents,
        };
        file.write_all(lock.contents.as_bytes())?;
        file.sync_all()?;
        Ok(lock)
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        if fs::read(&self.path).ok().as_deref() == Some(self.contents.as_bytes()) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn report(plan: &Plan) -> String {
    let mut text = format!(
        "Lite Switch takes priority; {} existing binding(s) displaced.\n",
        plan.displaced.len()
    );
    for binding in &plan.displaced {
        text.push_str(&format!(
            "  {} [{}] {}\n",
            binding.property, binding.shortcut, binding.command
        ));
    }
    text
}
fn build(
    store: &mut Xfconf,
    keyboard: &Keyboard,
    previous: Option<&Manifest>,
    book: &Book,
    settings: &Settings,
    launcher: &Path,
) -> Result<Plan> {
    let plan = plan::build(
        store.snapshot()?,
        previous,
        proposal(book, settings, launcher)?,
        |chord| keyboard.resolve(chord),
        |chord| keyboard.required(chord),
    )?;
    keyboard.unchanged()?;
    Ok(plan)
}

pub fn check(book: &Book, settings: &Settings, launcher: &Path) -> Result<String> {
    let paths = Paths::new()?;
    let before = image(&paths.manifest)?;
    let previous = load(before.as_ref())?;
    let mut store = Xfconf::connect()?;
    let keyboard = Keyboard::connect()?;
    let plan = build(
        &mut store,
        &keyboard,
        previous.as_ref(),
        book,
        settings,
        launcher,
    )?;
    transaction::preflight(&mut store, &plan)?;
    Ok(format!(
        "Xfce shortcut proposal analyzed. No shortcut properties/binding state changed; no grab probes performed.\n{}",
        report(&plan)
    ))
}

pub fn status(book: &Book, settings: &Settings, launcher: &Path) -> Result<String> {
    let paths = Paths::new()?;
    let before = image(&paths.manifest)?;
    let previous = load(before.as_ref())?;
    let mut store = Xfconf::connect()?;
    let current = store.snapshot()?;
    let intact = plan::restored(&current, previous.as_ref());
    let (valid, error) = match intact {
        Ok(_) => (true, None),
        Err(error) => (false, Some(error.to_string())),
    };
    let desired = proposal(book, settings, launcher)?;
    let mut pending = previous
        .as_ref()
        .is_some_and(|manifest| manifest.shortcuts != desired);
    if valid && let Some(previous) = &previous {
        let keyboard = Keyboard::connect()?;
        let planned = plan::build(
            current.clone(),
            Some(previous),
            desired,
            |chord| keyboard.resolve(chord),
            |chord| keyboard.required(chord),
        )?;
        pending |= planned.after != current || planned.manifest != *previous;
    }
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({
            "installed": previous.is_some(), "intact": valid, "error": error,
            "pending_changes": pending,
            "priority": "lite-switch-first", "manifest": paths.manifest,
            "managed_properties": previous.as_ref().map(|manifest| &manifest.entries),
        }))?
    ))
}

#[derive(Serialize)]
struct Backup<'a> {
    version: u32,
    channel: &'static str,
    before: &'a super::xfconf::Snapshot,
    after: &'a super::xfconf::Snapshot,
    manifest_path: &'a Path,
    manifest_before: &'a Option<Image>,
    manifest_after: &'a Option<Image>,
}
fn commit(
    paths: &Paths,
    before: Option<Image>,
    after: Option<Image>,
    store: &mut Xfconf,
    keyboard: &Keyboard,
    plan: &Plan,
) -> Result<PathBuf> {
    let backup = paths
        .backups
        .join(format!("xfce-bindings-{}.json", nonce()?));
    storage::write_json(
        &backup,
        &Backup {
            version: 1,
            channel: super::xfconf::CHANNEL,
            before: &plan.before,
            after: &plan.after,
            manifest_path: &paths.manifest,
            manifest_before: &before,
            manifest_after: &after,
        },
    )?;
    transaction::execute(
        store,
        plan,
        |claimed, grabs| keyboard.wait(grabs, claimed),
        || {
            if image(&paths.manifest)? != before {
                return Err(
                    "binding manifest changed concurrently; refusing to overwrite it".into(),
                );
            }
            if let Some(after) = &after {
                storage::write_file(&paths.manifest, &after.bytes, after.mode)?;
            } else {
                fs::remove_file(&paths.manifest)?;
            }
            Ok(())
        },
    )
    .map_err(|error| format!("{error}. Recovery snapshot: {}", backup.display()))?;
    Ok(backup)
}

pub fn install(
    book: &Book,
    settings: &Settings,
    launcher: &Path,
    approved: bool,
) -> Result<String> {
    if !approved {
        return Err("shortcut installation/sync requires explicit --approve-xfce-shortcuts".into());
    }
    apply(book, settings, launcher, false)?.ok_or_else(|| "shortcuts are not installed".into())
}

/// Only a persisted, explicit auto-sync opt-in may call this. Check installation
/// under the lifecycle lock so a concurrent uninstall cannot be undone.
pub fn sync_existing(book: &Book, settings: &Settings, launcher: &Path) -> Result<Option<String>> {
    if !settings.auto_sync_shortcuts {
        return Err("automatic shortcut sync is not approved".into());
    }
    apply(book, settings, launcher, true)
}

fn apply(
    book: &Book,
    settings: &Settings,
    launcher: &Path,
    existing_only: bool,
) -> Result<Option<String>> {
    let paths = Paths::new()?;
    let _lock = Lock::acquire(&paths)?;
    let before = image(&paths.manifest)?;
    let previous = load(before.as_ref())?;
    if existing_only && previous.is_none() {
        return Ok(None);
    }
    let mut store = Xfconf::connect()?;
    let keyboard = Keyboard::connect()?;
    let plan = build(
        &mut store,
        &keyboard,
        previous.as_ref(),
        book,
        settings,
        launcher,
    )?;
    let after = Some(Image {
        bytes: serde_json::to_vec_pretty(&plan.manifest)?,
        mode: 0o600,
    });
    if before == after && plan.before == plan.after {
        return Ok(Some(format!(
            "Managed Xfce shortcuts are already up to date; no changes.\n{}",
            report(&plan)
        )));
    }
    let backup = commit(&paths, before, after, &mut store, &keyboard, &plan)?;
    Ok(Some(format!(
        "Managed Xfce shortcut properties installed; passive-grab occupancy verified. Backup: {}\n{}",
        backup.display(),
        report(&plan)
    )))
}

pub fn uninstall(approved: bool) -> Result<String> {
    if !approved {
        return Err("shortcut removal requires explicit --approve-xfce-shortcuts".into());
    }
    let paths = Paths::new()?;
    let _lock = Lock::acquire(&paths)?;
    let before = image(&paths.manifest)?;
    let previous = load(before.as_ref())?.ok_or("managed Xfce shortcuts are not installed")?;
    let mut store = Xfconf::connect()?;
    let keyboard = Keyboard::connect()?;
    let plan = plan::build(
        store.snapshot()?,
        Some(&previous),
        vec![],
        |chord| keyboard.resolve(chord),
        |chord| keyboard.required(chord),
    )?;
    let count = previous
        .entries
        .iter()
        .filter(|entry| {
            plan::binding(&entry.property).is_some()
                && entry
                    .original
                    .as_ref()
                    .and_then(super::xfconf::Value::as_string)
                    .is_some_and(|value| !value.is_empty())
                && entry.original != entry.installed
        })
        .count();
    let backup = commit(&paths, before, None, &mut store, &keyboard, &plan)?;
    Ok(format!(
        "Managed Xfce shortcuts removed; restored {count} displaced binding(s). Preferences/history/backups retained. Backup: {}\n",
        backup.display()
    ))
}
