//! Start a hidden resident server for first-use CLI commands without hanging
//! their terminal or discarding session-only assignments when the command exits.
use super::*;
use std::{
    fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    process::{Command, Stdio},
    thread,
    time::Instant,
};

fn owner(bus: &gio::DBusConnection) -> Result<bool> {
    let result = bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&(APP_ID,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        1000,
        None::<&gio::Cancellable>,
    )?;
    Ok(result
        .get::<(bool,)>()
        .ok_or("invalid session-bus owner response")?
        .0)
}

/// true means quit was a no-op; all other commands proceed through GApplication.
pub(super) fn ensure(commands: &[String]) -> Result<bool> {
    let action = commands.first().map(String::as_str).unwrap_or("toggle");
    if ["show", "toggle", "__serve"].contains(&action) {
        return Ok(false);
    }
    let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;
    if owner(&bus)? {
        return Ok(false);
    }
    if action == "quit" {
        return Ok(true);
    }
    // Fail before spawning an invisible process on an incompatible/missing WM.
    Desktop::connect()?;
    let path = history::path()?.with_file_name("application.log");
    let directory = path.parent().ok_or("missing application log directory")?;
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("refusing a symlinked application log".into());
    }
    let log = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(&path)?;
    log.set_permissions(fs::Permissions::from_mode(0o600))?;
    let mut child = Command::new(std::env::current_exe()?)
        .arg("__serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if owner(&bus)? {
            return Ok(false);
        }
        if let Some(status) = child.try_wait()? {
            return Err(
                format!("resident server exited ({status}); see {}", path.display()).into(),
            );
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("resident startup timed out; see {}", path.display()).into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}
