# i3 Development

This is an independent Rust workspace targeting i3 on X11 only.

On SDK-only hosts, first `source scripts/environment.sh`. After code or
configuration changes, run from this directory:

1. `cargo fmt --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test`
4. `cargo build --release`
5. For UI/focus changes, `python3 tests/desktop_smoke.py` (isolated Xvfb/i3/bus;
   needs the desktop test dependencies).

After implementing a runnable desktop feature, install/package as appropriate,
relaunch it in the target i3 session, and verify the running version includes the
change. Do not claim desktop verification from a headless shell.

Do not modify i3 bindings or autostart configuration without approval and backups.
Do not add a generic desktop abstraction or Sway support.
