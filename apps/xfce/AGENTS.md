# Xfce Development

Target Xfce/X11 with xfwm4, standard X11/EWMH and Xfce settings. Do not add
Wayland, Sway or a generic desktop abstraction. Preserve apps/i3 as a legacy
independent workspace; do not delete it or move the macOS implementation.

Run from this directory after changes:

1. `cargo fmt --check`
2. `cargo clippy --locked --all-targets -- -D warnings`
3. `cargo test --locked`
4. `cargo build --locked --release`
5. Run the isolated Xvfb/xfwm4 tests for X11/UI/focus changes.

Reuse applicable Rust/GTK logic by porting it into this independent build, not by
introducing a cross-platform runtime. Keep state local/private and commands safe
against stale/reused XIDs. Preserve identity continuity only while X11 event
continuity is maintained. Exclude panels and desktop surfaces from target lists.

Install and execute/reopen implemented native features in the actual Xfce
session. Do not claim audible playback or real lock/unlock success without user
confirmation. Those acceptance checks may remain pending while port work proceeds.
Do not unexpectedly lock, suspend or log out the user. Real shortcut changes
require approval, backup, displacement reporting and safe restoration. Lite
Switch wins conflicts on approved install/sync, not by silently deleting keys.
