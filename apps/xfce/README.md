# Lite Switch for Xfce/X11

Active Linux target. This independent Rust/GTK4 workspace ports the useful
launcher/search/assignment logic from `apps/i3` without moving the macOS app or
deleting the preserved legacy i3 build. No i3 IPC or generic desktop adapter is
used here: the target is Xfce on X11 with xfwm4.

## Current implementation

- Single-instance, resident GTK4 launcher with app icons and keyboard selection.
- Installed-app discovery and safe desktop-entry launching through GIO, not shell
  strings; case/diacritic-normalized exact/prefix/word-prefix/fuzzy app ranking and
  private local usage history.
- X11/EWMH discovery: Unicode/legacy titles, ICCCM class/instance, role/PID,
  workspace names, RandR monitors, outer-frame geometry and window state. Panels,
  desktop surfaces and other non-application surfaces are excluded.
- Verified workspace-aware activation and minimized-window restoration.
- A centered, undecorated floating panel; Escape restores previous focus, while
  outside focus dismisses without stealing focus back. Existing geometry is not
  changed. The panel skips the taskbar/pager.
- Event-driven window/title/workspace/output updates and GIO app monitoring;
  unchanged targets do not rebuild GTK rows during mouse/key interaction.
- Conservative app matching with persistent class/instance/role overrides,
  stable cross-workspace cycling, assigned-window exclusion and configurable
  launch-if-needed preferences. Ambiguity refuses activation/duplicate launching.
- GTK key capture/removal and CLI app/window assignments. App assignments persist;
  window assignments last only for the resident process. Destruction/reused XIDs
  invalidate old generations even if snapshots never observe a gap. Lost backend
  or WM continuity clears session targets before reconnecting.
- Saved launcher/direct-modifier preferences and review-only Xfce shortcut JSON.
- Approved install/sync/uninstall through typed Xfconf D-Bus properties, with
  conflict priority, private recovery snapshots, restoration and grab checks.

Global shortcuts are **opt-in**. Approved lifecycle commands apply them; an
additional automatic-sync opt-in makes subsequent assignment/settings edits live.
Close/fill/half-screen/center/output actions and persistent action assignments are also
implemented. Vivaldi tabs, audio controls, graphical settings, app autostart and
complete desktop packaging remain to be implemented. Real-session installation
is a user-approved deployment choice, never a side effect of installing the app.

## Build

Rust 1.88+, GTK4 development libraries with GLib/GIO 2.80+, Xfce/xfwm4 on X11,
a session D-Bus and RandR 1.5+ are required. On SDK-only hosts, source the helper
to load the existing private GTK SDK. The copied helper is independent of the
legacy workspace; no i3 build is required.

```sh
source scripts/environment.sh  # Optional private SDK fallback
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
cargo clippy --locked --no-default-features --all-targets -- -D warnings
cargo test --locked --no-default-features
```

Diagnostic-only builds without GTK remain available using `--no-default-features`.
Window listings can contain private titles; redact before sharing. Diagnostic
identity numbers are process-local, never persistent X11/application identifiers.

## Install and use

```sh
./scripts/install.sh                 # PREFIX defaults to ~/.local
~/.local/bin/lite-switch-xfce show    # First show/toggle stays in the foreground
# From another terminal:
~/.local/bin/lite-switch-xfce toggle
~/.local/bin/lite-switch-xfce quit
```

First-use assignment/settings/cycling commands automatically start a hidden
resident server and return, so session-only assignments survive without hanging
the caller's terminal. Startup errors are reported; background diagnostics are
private at `${XDG_STATE_HOME:-~/.local/state}/lite-switch/application.log`.

The wrapper/binary are installed separately at `bin/lite-switch-xfce` and
`libexec/lite-switch-xfce/`. The old i3 executable is not replaced or automatically
started. Updates replace files atomically, but an existing resident keeps its
old executable: quit it, install, then reopen. Before removing those installation paths, uninstall any managed shortcuts with
the approved lifecycle command below, then quit the resident. Local
history/preferences and recovery snapshots are retained.

Use the correct `DISPLAY` and Xauthority for the graphical session. A remote
shell may need `DISPLAY=:0`; do not assume a display number on other machines.

```sh
lite-switch-xfce doctor
lite-switch-xfce windows editor
lite-switch-xfce outputs
lite-switch-xfce focus 123456  # Actual X11 window ID, not an i3 container ID
lite-switch-xfce cycle application.desktop
lite-switch-xfce assignments
lite-switch-xfce assign-app a application.desktop [--launch-if-needed]
lite-switch-xfce assign-window w 123456
lite-switch-xfce activate w
lite-switch-xfce launch-if-needed a on
lite-switch-xfce unassign w
lite-switch-xfce settings
lite-switch-xfce set-launcher Super+space
lite-switch-xfce set-modifier Super
lite-switch-xfce bindings       # Export only
lite-switch-xfce bindings-check # Read-only priority/policy analysis
lite-switch-xfce bindings-status
```

Click **+ key** beside a result and press an ASCII letter, number or arrow.
Backspace/Delete removes the assignment; Escape cancels capture. An assigned
application has a **Launch** checkbox for its direct-key launch preference.
A key has one target; assigning an already assigned target moves its key.

Persistent preferences/assignments use private atomic JSON under XDG config
`lite-switch/`; usage history is under XDG state `lite-switch/usage.json`.
Existing compatible app/history data from the legacy implementation is retained.
Corrupt files are reported rather than silently overwritten. No telemetry or
external runtime network service is used.

Search **Close Window** or run `lite-switch-xfce action close` to request a normal
close of the previous/active application window. Assign it with
`lite-switch-xfce assign-action c close`. Save/cancel dialogs remain under the
application's control; a successful request does not guarantee the window closed.

## Explicit application identity overrides

When multiple desktop entries claim the same class, cycling refuses to guess or
launch a duplicate. Inspect `windows` JSON and configure exact criteria:

```sh
lite-switch-xfce matching
lite-switch-xfce match-app application.desktop --class AppClass
lite-switch-xfce match-app application.desktop --class AppClass --instance profile --role main
lite-switch-xfce unmatch-app application.desktop
```

At least one class/instance/role criterion is required. Supplied fields are
conjunctive, exact and ASCII-case-insensitive, not regexes. Explicit matches beat
all heuristics; apps with an explicit rule do not use heuristic fallback when
that rule fails. Overlapping explicit rules leave the window unmatched, and
identical criteria cannot be assigned to different apps. A pin to an unavailable
desktop entry does not let an unrelated heuristic claim its windows.

Rules persist privately in `lite-switch/matching.json` under XDG config and take
effect immediately for counts/cycling; no desktop shortcuts change or need sync.
Invalid/corrupt files and detected external edits are refused rather than
silently overwritten. Restart the resident after manual file edits to reload
its rule state. Graphical identity-rule editing remains a later settings feature.

## App and window results

Known running apps appear once by default rather than as both window and app
rows. Searching an app name keeps its cycling/application result. Specific window
titles still find individual windows, using their app icons when available.
Type `windows:` (optionally followed by a title/class query) to explicitly list
all windows, including windows titled identically to their app. Directly assigned
windows remain visible so their keys can be changed/removed; unmatched windows
remain accessible even without a desktop entry.

## Approved global shortcut lifecycle

After reviewing `bindings-check`, these commands change Xfce shortcuts:

```sh
lite-switch-xfce bindings-install --approve-xfce-shortcuts
# After assignment/shortcut preference changes, review again, then:
lite-switch-xfce bindings-sync --approve-xfce-shortcuts
lite-switch-xfce bindings-uninstall --approve-xfce-shortcuts
# Optional ongoing approval: future assignment/settings edits apply immediately.
lite-switch-xfce set-shortcut-sync on --approve-xfce-shortcuts
lite-switch-xfce set-shortcut-sync off
```

Without the flag they refuse before changing shortcut properties. Lite Switch
wins over conflicting custom bindings across registered Xfce providers, including
modifier aliases and overlapping physical grabs in the current layout/group.
Displaced paths/chords/commands are reported. Original values and types are
restored when keys move or are removed, including startup-notification metadata.
Unrelated keys/default-provider trees are not changed. Ctrl+Alt+F1–F12 are rejected
as Xorg VT-switch combinations; controls outside Xfce cannot be forcibly stolen.

Changes go through xfconfd, not direct XML edits. A private lock and versioned
before/after snapshots live under XDG config `lite-switch/`, with the manifest at
`xfce-binding-installation.json` and snapshots under `backups/xfce-bindings-*.json`.
Those snapshots include the channel and previous/proposed manifest images; they
may contain private commands, so never publish them unredacted. Files use mode
0600 and directories 0700. A stale `.xfce-bindings.lock` requires checking that no
installer is running before manually removing it.

The installer releases affected native grabs before adding replacements, waits
for availability and checks occupancy afterward without sending live keys. A
settling interval avoids racing asynchronous providers. Grab occupancy does not
identify the owning process; native tests additionally press the actual keys.
A remaining external grab causes failure/rollback instead of false success.
Policy locks, changed service/keyboard/WM identity and detected outside edits are
refused. Hand-edited managed properties are not silently overwritten. Identical
syncs do not rewrite state or create extra backups.

Xfconf has no cross-client compare-and-swap or transaction API. Guards are
optimistic snapshots around each observable write, not a promise of atomicity
against an edit in the tiny check/write race. Failures restore safe properties
and preserve detected outside edits; incomplete rollback names the recovery
snapshot. Restoration is of typed properties: xfconfd owns XML serialization and
may reorder its file.

Automatic sync is off by default, including when loading older preferences.
After explicit opt-in, GUI/CLI assignment, reassignment, removal and shortcut
preference edits use the same locked transaction, backup, conflict reporting,
restoration and grab checks. Session-window closure or lost continuity releases
its chord; restarting removes expired session chords. Approval is kept privately
in `xfce-shortcut-sync.json`, separate from the legacy-compatible shared settings.
Automatic sync never creates an installation, including after uninstall. Explicit
installation is still required. Turning it off leaves current bindings intact.
On sync failure the saved preference is retained, but CLI exits with an error and
the panel shows a warning; hover for full backup/conflict/error details. Outside
edits remain protected. With auto sync off, use approved `bindings-sync` manually.

## Tests

```sh
# Needs Xvfb, xfwm4, xfsettingsd, xfconf, xterm, xdotool and dbus-run-session.
source scripts/environment.sh
cargo build --locked --release
python3 tests/desktop_smoke.py
```

The harness uses its own X server, xfwm4 and service-directory-free session bus,
starts only private xfconf/xfsettingsd, isolates XDG directories and does not contact the
system bus. Native tests cover discovery, legacy/Unicode titles, geometry,
outputs, cross-workspace activation, minimized restoration, dock exclusion,
exact XID reuse and latched WM-continuity invalidation.

GTK tests cover centering, single instance, unchanged geometry, focus/toggle/
outside dismissal, live title/app updates, GIO launch/history, app cycling,
assignment exclusion/lifetime, actual key-button capture/removal, cold-CLI hidden
startup, corrupt-file protection and reconnection across real private WM
replacement. Shortcut tests perform approved install/update/remove on this
private desktop, press actual global keys, cover quoted launcher paths, alias
collisions, metadata restoration, idempotency, ownership protection and rollback
when an external client holds a grab. Identity tests introduce genuinely
ambiguous desktop entries, resolve them with explicit rules, exercise overlap/
duplicate safety and persistence, and preserve external file edits without
changing desktop shortcuts. Ordinary `cargo test` needs no display; the ignored native desktop
test is activated explicitly by the private harness. For diagnostic-only builds,
set `LITE_SWITCH_NO_GUI_TEST=1` when running that harness.

## Desktop setup acceptance

Xfce is installed with one panel, no desktop icons and existing scaling. Automatic
xss-lock/i3lock startup is configured and its sleep inhibitor was checked. Actual
audio playback and a user-confirmed lock/unlock/suspend cycle remain pending;
these checks do not block port development and must not be claimed as passed
from device enumeration or process checks alone.
