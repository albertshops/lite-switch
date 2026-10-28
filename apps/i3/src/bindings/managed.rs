//! Explicitly approved, reversible integration with the current i3 config.
use super::{
    self as bindings, priority,
    transaction::{self, Change, Image},
};
use crate::{
    assignments::{self, Book},
    ipc::{self, Result},
    settings::Settings,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const BEGIN: &str = "# BEGIN LITE SWITCH MANAGED BINDINGS";
const END: &str = "# END LITE SWITCH MANAGED BINDINGS";

struct Paths {
    directory: PathBuf,
    include: PathBuf,
    manifest: PathBuf,
}
impl Paths {
    fn new() -> Result<Self> {
        let directory = assignments::path()?
            .parent()
            .ok_or("missing config directory")?
            .to_owned();
        Ok(Self {
            include: directory.join("i3-bindings.conf"),
            manifest: directory.join("binding-installation.json"),
            directory,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    root_config: PathBuf,
    root_target: PathBuf,
    include: PathBuf,
    contents: String,
    #[serde(default)]
    displaced: Vec<priority::Displaced>,
}

fn load(paths: &Paths) -> Result<Option<Manifest>> {
    parse_manifest(paths, Image::read(&paths.manifest)?.as_ref())
}

fn parse_manifest(paths: &Paths, image: Option<&Image>) -> Result<Option<Manifest>> {
    let Some(image) = image else {
        return Ok(None);
    };
    let manifest: Manifest = serde_json::from_slice(&image.bytes)?;
    if manifest.version != 1
        || manifest.include != paths.include
        || !manifest.contents.starts_with(bindings::HEADER)
    {
        return Err("invalid binding-installation manifest; refusing to modify files".into());
    }
    bindings::shell_quote(&manifest.root_config)?;
    bindings::shell_quote(&manifest.root_target)?;
    Ok(Some(manifest))
}

fn block(include: &Path) -> Result<String> {
    Ok(format!(
        "\n{BEGIN}\ninclude {}\n{END}\n",
        bindings::shell_quote(include)?
    ))
}

fn verify_owned(manifest: &Manifest, socket: &Path) -> Result<String> {
    if bindings::config_path(socket)? != manifest.root_config
        || fs::canonicalize(&manifest.root_config)? != manifest.root_target
    {
        return Err(
            "the active i3 config target changed since installation; refusing to edit it".into(),
        );
    }
    let root = fs::read_to_string(&manifest.root_target)?;
    let expected = block(&manifest.include)?;
    if root.matches(BEGIN).count() != 1
        || root.matches(END).count() != 1
        || root.matches(&expected).count() != 1
    {
        return Err("managed block was removed/edited; refusing to overwrite user changes".into());
    }
    if Image::read(&manifest.include)?
        .is_none_or(|image| image.bytes != manifest.contents.as_bytes())
    {
        return Err(
            "managed include was edited or removed; refusing to overwrite user changes".into(),
        );
    }
    let mut sources = BTreeMap::new();
    for entry in &manifest.displaced {
        bindings::shell_quote(&entry.path)?;
        sources
            .entry(entry.path.clone())
            .or_insert(fs::read_to_string(&entry.path)?);
    }
    priority::restore(&mut sources, &manifest.displaced)?;
    Ok(root)
}

fn proposal(
    paths: &Paths,
    previous: Option<&Manifest>,
    proposed: &str,
    socket: &Path,
) -> Result<(PathBuf, PathBuf, priority::Plan)> {
    let (root_config, root_target, observed) = if let Some(manifest) = previous {
        (
            manifest.root_config.clone(),
            manifest.root_target.clone(),
            verify_owned(manifest, socket)?,
        )
    } else {
        let config = bindings::config_path(socket)?;
        let target = fs::canonicalize(&config)?;
        let raw = fs::read_to_string(&target)?;
        if raw.contains(BEGIN) || raw.contains(END) || Image::read(&paths.include)?.is_some() {
            return Err(
                "an unmanaged block/include already exists; refusing to overwrite it".into(),
            );
        }
        (config, target, raw)
    };
    let clean = if previous.is_some() {
        observed.replacen(&block(&paths.include)?, "", 1)
    } else {
        observed.clone()
    };
    let plan = priority::build(
        &root_config,
        &clean,
        previous.map_or(&[], |m| m.displaced.as_slice()),
        proposed,
        &paths.include,
    )?;
    if plan.root_before.bytes != observed.as_bytes()
        || fs::canonicalize(&root_config)? != root_target
    {
        return Err("root config changed during validation; refusing to overwrite it".into());
    }
    Ok((root_config, root_target, plan))
}

pub fn check(book: &Book, settings: &Settings, launcher: &Path, socket: &Path) -> Result<String> {
    let paths = Paths::new()?;
    let previous = load(&paths)?;
    let (_, _, plan) = proposal(
        &paths,
        previous.as_ref(),
        &bindings::render(book, settings, launcher)?,
        socket,
    )?;
    Ok(format!(
        "Proposed priority bindings passed i3 validation. No existing config was changed.\n{}",
        plan.report()
    ))
}

pub fn status(book: &Book, settings: &Settings, launcher: &Path, socket: &Path) -> Result<String> {
    let paths = Paths::new()?;
    let value = match load(&paths)? {
        None => serde_json::json!({"installed": false, "include": paths.include}),
        Some(manifest) => {
            let intact = verify_owned(&manifest, socket).is_ok();
            let pending = bindings::render(book, settings, launcher)? != manifest.contents;
            serde_json::json!({"installed": true, "intact": intact, "pending_changes": pending, "priority": "lite-switch-first", "displaced": manifest.displaced, "root_config": manifest.root_config,
                "root_target": manifest.root_target, "include": manifest.include})
        }
    };
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}

struct Lock(PathBuf);
impl Lock {
    fn acquire(paths: &Paths) -> Result<Self> {
        fs::create_dir_all(&paths.directory)?;
        fs::set_permissions(&paths.directory, fs::Permissions::from_mode(0o700))?;
        let path = paths.directory.join(".bindings.lock");
        let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)
            .map_err(|e| format!("cannot lock binding installation (remove a stale lock only after checking no installer is running): {}: {e}", path.display()))?;
        let lock = Self(path);
        writeln!(file, "{}", std::process::id())?;
        Ok(lock)
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn reload(socket: &Path, expected: Option<&Manifest>, include: &Path) -> Result<()> {
    let replies: serde_json::Value = ipc::request(socket, ipc::COMMAND, "reload")?;
    if !replies
        .as_array()
        .is_some_and(|items| !items.is_empty() && items.iter().all(|item| item["success"] == true))
    {
        return Err("i3 rejected reload".into());
    }
    let config: serde_json::Value = ipc::request(socket, 9, "")?; // GET_CONFIG
    let files = config["included_configs"]
        .as_array()
        .ok_or("i3 does not expose included config contents")?;
    let loaded = files.iter().find(|file| {
        file["path"].as_str().is_some_and(|path| {
            Path::new(path) == include
                || fs::canonicalize(path)
                    .ok()
                    .zip(fs::canonicalize(include).ok())
                    .is_some_and(|(a, b)| a == b)
        })
    });
    match (expected, loaded) {
        (Some(manifest), Some(file))
            if file["raw_contents"].as_str() == Some(manifest.contents.as_str()) =>
        {
            Ok(())
        }
        (None, None) => Ok(()),
        _ => Err("i3 did not load the expected managed include state".into()),
    }
}

fn backup_path(paths: &Paths) -> Result<PathBuf> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(paths
        .directory
        .join("backups")
        .join(format!("bindings-{}-{nonce}.json", std::process::id())))
}

pub fn install(
    book: &Book,
    settings: &Settings,
    launcher: &Path,
    socket: &Path,
    approved: bool,
) -> Result<String> {
    if !approved {
        return Err("binding installation requires explicit --approve-i3-config".into());
    }
    let paths = Paths::new()?;
    let _lock = Lock::acquire(&paths)?;
    let manifest_before = Image::read(&paths.manifest)?;
    let previous = parse_manifest(&paths, manifest_before.as_ref())?;
    let contents = bindings::render(book, settings, launcher)?;
    let (root_config, root_target, plan) = proposal(&paths, previous.as_ref(), &contents, socket)?;
    let report = plan.report();
    let root_after = format!("{}{}", plan.root_after, block(&paths.include)?);
    let manifest = Manifest {
        version: 1,
        root_config,
        root_target: root_target.clone(),
        include: paths.include.clone(),
        contents: contents.clone(),
        displaced: plan.displaced,
    };
    let include_before = if let Some(old) = &previous {
        let image = Image::read(&paths.include)?.ok_or("managed include disappeared")?;
        if image.bytes != old.contents.as_bytes() {
            return Err("managed include changed during validation".into());
        }
        Some(image)
    } else {
        None
    };
    let mut changes = plan.changes;
    changes.push(Change {
        path: root_target,
        after: Some(Image::text(&root_after, plan.root_before.mode)),
        before: Some(plan.root_before),
    });
    changes.push(Change {
        path: paths.include.clone(),
        before: include_before,
        after: Some(Image::text(&contents, 0o600)),
    });
    changes.push(Change {
        path: paths.manifest.clone(),
        before: manifest_before,
        after: Some(Image {
            bytes: serde_json::to_vec_pretty(&manifest)?,
            mode: 0o600,
        }),
    });
    if changes.iter().all(|change| change.before == change.after) {
        return Ok(format!(
            "Managed bindings are already up to date; no reload needed.\n{report}"
        ));
    }
    let backup = backup_path(&paths)?;
    transaction::commit(
        &changes,
        &backup,
        || bindings::validate_config(&manifest.root_config),
        |restoring| {
            reload(
                socket,
                if restoring {
                    previous.as_ref()
                } else {
                    Some(&manifest)
                },
                &paths.include,
            )
        },
    )?;
    Ok(format!(
        "Managed priority bindings installed/reloaded. Backup: {}\n{report}",
        backup.display()
    ))
}

pub fn uninstall(socket: &Path, approved: bool) -> Result<String> {
    if !approved {
        return Err("binding removal requires explicit --approve-i3-config".into());
    }
    let paths = Paths::new()?;
    let _lock = Lock::acquire(&paths)?;
    let manifest_before = Image::read(&paths.manifest)?;
    let manifest = parse_manifest(&paths, manifest_before.as_ref())?
        .ok_or("managed bindings are not installed")?;
    let root = verify_owned(&manifest, socket)?;
    let mut before = BTreeMap::new();
    let mut contents = BTreeMap::new();
    for path in
        std::iter::once(&manifest.root_target).chain(manifest.displaced.iter().map(|d| &d.path))
    {
        if before.contains_key(path) {
            continue;
        }
        let image = Image::read(path)?.ok_or("displaced source disappeared")?;
        contents.insert(path.clone(), String::from_utf8(image.bytes.clone())?);
        before.insert(path.clone(), image);
    }
    if contents[&manifest.root_target] != root {
        return Err("root changed during validation".into());
    }
    contents.insert(
        manifest.root_target.clone(),
        root.replacen(&block(&paths.include)?, "", 1),
    );
    priority::restore(&mut contents, &manifest.displaced)?;
    let mut changes = Vec::new();
    for (path, text) in contents {
        let image = before.remove(&path).unwrap();
        changes.push(Change {
            path,
            after: Some(Image::text(&text, image.mode)),
            before: Some(image),
        });
    }
    let include_image = Image::read(&paths.include)?.ok_or("managed include disappeared")?;
    if include_image.bytes != manifest.contents.as_bytes()
        || fs::canonicalize(&manifest.root_config)? != manifest.root_target
    {
        return Err("managed files changed during validation; refusing to overwrite them".into());
    }
    changes.push(Change {
        path: paths.include.clone(),
        before: Some(include_image),
        after: None,
    });
    changes.push(Change {
        path: paths.manifest.clone(),
        before: manifest_before,
        after: None,
    });
    let backup = backup_path(&paths)?;
    transaction::commit(
        &changes,
        &backup,
        || bindings::validate_config(&manifest.root_config),
        |restoring| {
            reload(
                socket,
                if restoring { Some(&manifest) } else { None },
                &paths.include,
            )
        },
    )?;
    Ok(format!(
        "Managed bindings removed; restored {} displaced binding(s). Assignments/settings retained. Backup: {}\n",
        manifest.displaced.len(),
        backup.display()
    ))
}
