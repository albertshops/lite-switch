# Lite Switch for i3

> Preserved legacy implementation. Active Linux development has moved to
> [Xfce/X11](../xfce/README.md). This workspace and its existing behaviour/tests
> remain available independently; new Linux features target Xfce.

An i3/X11-only Linux implementation of Lite Switch. This directory is an
independent Cargo workspace; it does not depend on Swift or the macOS build.

## Current status

The first graphical launcher is implemented:

- A single-instance resident GTK4 panel, toggled through the CLI/session bus.
- Installed-application discovery using GIO desktop entries (including entries
  exported through XDG application directories), application icons, and safe
  desktop-entry launching rather than shell-string execution.
- Running-window search/activation via i3 IPC, including hidden scratchpad windows.
- Event-driven window/title/workspace/output updates and GIO application discovery
  updates while the panel is open, preserving the selected result when possible.
- Conservative app/window identity matching and application-window cycling across
  workspaces. Application rows show their running-window count.
- Exact/prefix/word-prefix/fuzzy app-name ranking with local usage history,
  case/diacritic normalization, and deterministic tie breaking.
- Keyboard selection, Enter to activate, Escape to restore the previous window,
  outside-click dismissal, and a floating panel that preserves tiled geometry.

Direct app/window assignments, configurable shortcut chords/modifiers, and
opt-in managed binding installation/update/removal with backups are implemented.
Window actions, Vivaldi tabs, audio outputs, graphical settings, autostart, and
complete desktop packaging remain to be implemented.
Application rows cycle existing matching windows or launch if none are found;
window rows focus the selected window. Result lists are capped at 30 windows and 40
apps. There are no automatically installed shortcuts yet. Assignment changes save the
book only; adding/removing globally grabbed keys requires reviewing and explicitly
approving a managed sync. Normal launching, assignments, settings, export, and
checks do not modify your i3 configuration. Only approved binding lifecycle
commands change it.

## Build and check

Install a current stable Rust toolchain (1.88+) and GTK4 development libraries,
including GLib/GIO 2.80 or newer. On
Debian/Ubuntu, the usual system prerequisite is `libgtk-4-dev` plus `pkg-config`
and a C toolchain. The UI targets i3 on X11 and needs a session D-Bus connection.

When sudo is unavailable, an optional development fallback extracts APT packages
into a private SDK without installing system packages or running package scripts:

```sh
./scripts/bootstrap-gtk-sdk.sh --with-desktop-tests
source scripts/environment.sh
```

The fallback was tested on Ubuntu 24.04 x86_64. It uses the configured APT
repositories and requires current package indexes. Other distributions should
use their normal GTK4 development packages. The SDK defaults to
`~/.local/share/lite-switch/gtk4-sdk`; override with `LITE_SWITCH_GTK_SDK`.

From this directory:

```sh
source scripts/environment.sh  # Optional; auto-detects the private SDK
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
cargo run -- show
```

The first `show`/`toggle` invocation remains running; later invocations communicate
with it over the session bus. `lite-switch quit` stops it. Building/installing the
app does not install bindings, services, or autostart entries. On hosts without GTK, diagnostic-only builds
remain available with `cargo build --no-default-features`.

## Install and run

```sh
./scripts/install.sh  # Default prefix: ~/.local
~/.local/bin/lite-switch show
# In another terminal:
~/.local/bin/lite-switch toggle
~/.local/bin/lite-switch quit
```

Use `PREFIX=/your/prefix ./scripts/install.sh` for a custom prefix. The installer
places a wrapper in `bin/lite-switch` and the binary/environment helper under
`libexec/lite-switch/`. The wrapper enables X11 and loads the optional private SDK
when necessary. Ensure the prefix's `bin` directory is on PATH. Updating does not
stop an already running instance: quit it, install, then reopen the panel.

Before removing the binary, remove any managed i3 integration using the approved
binding-uninstall command below. Then quit the app and remove
`~/.local/bin/lite-switch` and `~/.local/libexec/lite-switch/`. History is retained at
`${XDG_STATE_HOME:-~/.local/state}/lite-switch/usage.json`. No telemetry or network
service is used. Corrupt history is reported rather than silently overwritten.

## Diagnostic commands

```sh
lite-switch doctor
lite-switch windows
lite-switch windows vivaldi
lite-switch outputs
lite-switch cycle vivaldi-stable.desktop  # Exact installed desktop-entry ID
lite-switch focus 123456  # Use an actual container ID from `windows`
```

Window JSON includes container/X11 IDs, title, class, instance, role, workspace,
output, focus, urgency, and scratchpad status. Dock areas such as i3bar are
excluded. The diagnostic CLI filter is case-insensitive substring matching across
title/class/instance, with all query words required; GUI app ranking is separate.

Socket discovery uses `I3SOCK`, then `i3 --get-socketpath`. For shells without
session environment, explicitly set `DISPLAY` and/or the appropriate `I3SOCK`.
Do not hard-code a socket filename in permanent configuration: it changes when
i3 restarts. IPC reads/writes have three-second timeouts and bounded responses.
Focus verifies the target and matches both numeric container and X11 IDs. These
are session identities, not persistent application identifiers. Diagnostic output
can contain sensitive titles; redact it before sharing.

## App/window matching

Matching prefers a desktop entry's `StartupWMClass`, then its desktop ID, its
reverse-domain ID suffix, and its executable basename. Within each tier, the
X11 class outranks the instance (important for custom terminals inheriting an
`xterm` instance). Ambiguous best matches are left unmatched rather than guessed.
Explicit per-app matching overrides are not implemented yet.

Cycling uses stable container-ID ordering, advances from the previously active
window, and wraps across workspaces, including scratchpad windows. Activating an
app through the panel or `cycle` records local usage. Individually assigned windows are excluded from application cycling. If all of
an app's windows are individually assigned, cycling reports that no eligible
windows remain rather than launching a duplicate.

Window changes arrive over an i3 subscription integrated into GTK's file-descriptor
main loop, not periodic tree polling. Bursts are coalesced into one refresh, and
lost subscriptions are retried. App-directory changes use GIO's app monitor.

## Shortcut assignments (global installation is separate)

Click **+ key** beside a result, then press an ASCII letter, number, or arrow.
Click an existing key label to change it; Backspace/Delete removes it and Escape
cancels recording. Each key has one target, and assigning an existing target to
a new key moves its binding. Assignment capture does not install global keys.

- App assignments persist in `${XDG_CONFIG_HOME:-~/.config}/lite-switch/assignments.json`.
- Window assignments stay in the resident process only and disappear on close,
  restart, or lost i3 event continuity. Both container/X11 IDs are checked.
- Assigned apps expose a **Launch** checkbox. It defaults off for direct app
  shortcuts; enable it to launch when no app windows exist.
- Assignments and history use private, atomic JSON writes. Invalid/corrupt files
  are reported rather than silently replaced.

Equivalent CLI operations (use exact installed desktop-entry IDs):

```sh
lite-switch assignments
lite-switch assign-app a vivaldi-stable.desktop
lite-switch assign-window w 123456
lite-switch activate a
lite-switch launch-if-needed a on
lite-switch unassign w
lite-switch bindings          # Print a proposed include, without writing it
lite-switch bindings-check    # Validate against current config; no edits/reload
```

The default proposal binds **Alt+Space** and only currently assigned Alt keys.
It invokes `lite-switch activate <key>`, so window assignments remain session-only.
The executable is quoted for both i3 and the shell. An installed wrapper exports
its own path so custom installation prefixes work.

## Configurable shortcuts and managed i3 integration

Settings are saved privately in `lite-switch/settings.json` under XDG config.
These commands save app preferences only; they do not install or change i3 keys:

```sh
lite-switch settings
lite-switch set-launcher Super+space
lite-switch set-modifier Super
lite-switch bindings
lite-switch bindings-check
lite-switch bindings-status
```

Launcher chords accept Alt/Mod1, Super/Mod4, Control/Ctrl, and Shift, plus a
letter, number, arrow, Space, Return, Tab, Escape, or F1–F12. At least a non-Shift
modifier is required. Direct assignments use Alt/Mod1 or Super/Mod4. In your live
i3 config Alt+Space is already bound. Lite Switch takes priority: approved
installation backs up and temporarily comments out the old binding, reporting
its path, line, chord, and original command. Choosing a different chord is optional.

**The following commands change your current i3 configuration and reload i3.
Run them only after explicitly approving that operation:**

```sh
lite-switch bindings-install --approve-i3-config
# After changing assignments or shortcut settings, review/check again, then:
lite-switch bindings-sync --approve-i3-config
# To remove only Lite Switch's managed integration:
lite-switch bindings-uninstall --approve-i3-config
```

Without the approval flag these operations fail before editing any existing
configuration. Installation appends a marked block to the active root config and
maintains an isolated `lite-switch/i3-bindings.conf` include. Symlinked root
configs retain their symlink; the target is updated atomically with its existing
permissions. Conflicting default-mode bindings, including release bindings and
matching physical bindcodes, are disabled in their original source files.
Other keys and named modes remain unchanged. Moving/removing a Lite Switch chord
restores its displaced bindings; uninstall restores all remaining displaced
bindings and removes the exact owned block/include/manifest. Other user edits,
assignments, settings, and backups are preserved. Identical syncs
are no-ops rather than unnecessary reloads.

Changes are protected by a private lock and recovery snapshots under
`lite-switch/backups/`. Backups are mode 0600 inside a private directory and store
versioned before/after byte images, modes, and paths for every changed file.
Validation/apply/reload failures trigger rollback; after a reload attempt the old
configuration is reloaded too. Optimistic content checks refuse to overwrite
concurrent edits. If outside edits prevent safe rollback, the error names the
backup for manual recovery. A stale `.bindings.lock` is not removed automatically;
check that no installer is running before removing one.

Priority checks use i3's variable-expanded parser output and the current X11
keymap, preserving relative/glob include semantics. Private shadow files validate
the proposed replacement configuration with `i3 -C`; checks report displaced
bindings without changing existing config or reloading it. Include expansion
uses an isolated helper that rejects command substitutions. Unsupported parser
output/syntax fails safely rather than guessing. Affected source files must be
writable; recheck/sync after changing the keyboard layout. The applied configuration is checked
again before reload, and i3's reported included contents are verified afterward.
Hand-edited managed includes/displacement blocks, missing source files, or a
changed root target are refused rather than overwritten. Reloading has normal i3 side effects, including rerunning
`exec_always` commands; rollback may require another reload. Config diagnostics
and backups can contain private commands/secrets, so do not publish them unredacted.

There is not yet automatic sync after changing keys. A removed session target is
safe to activate (it reports unassigned), but its old globally grabbed chord stays
reserved until an approved sync removes it. `bindings-status` reports pending
changes. **The agent has not run any approved installation/sync/removal command
in your real session; all mutating lifecycle tests use an isolated i3 server.**

## Tests

Unit tests cover search ranking, usage persistence/permissions, protocol framing,
argument validation, and tree extraction. Integration tests launch the compiled
CLI against isolated fake i3 sockets, exercising stale targets, command failures,
scratchpad focus, malformed replies, and post-command focus verification.
`cargo test` does not require a display.

An opt-in desktop test starts its own Xvfb, i3, and private session bus. It does
not use your display, bindings, config, state, or resident app instance:

```sh
# Needs Xvfb, i3, xterm, xdotool, and dbus-run-session.
source scripts/environment.sh
cargo build --release
python3 tests/desktop_smoke.py
```

It verifies floating geometry, preservation of a tiled window's size, single
instance behaviour, Escape/previous focus, toggle, window activation, desktop-entry
launching, private usage-history persistence, live window-title/app-directory
updates, cross-workspace app cycling without duplicate launches, GTK shortcut
capture/removal, actual exported global keys, priority replacement across
variables, nested/glob includes, continued lines, bindcodes and release bindings,
named-mode preservation, exact restoration,
assigned-window exclusion, launch policy, assignment lifetime/persistence,
configurable chords/modifiers, approved install/update/removal, idempotency,
conflicting-update replacement, invalid-config rejection, edited-include
protection, private backups, and
symlink/permission preservation. Transaction tests also inject validation, partial
write, reload, and concurrent-edit failures to exercise rollback. Multi-monitor
behaviour still needs dedicated checks.

## Next implementation steps

1. Add explicit identity overrides for ambiguous applications.
2. Add graphical shortcut settings/review/application controls (CLI integration
   is implemented; no real-session binding changes without approval).
3. Add previous-window actions with explicit floating/tiling semantics.
4. Add Vivaldi tabs through extension native messaging.
5. Add audio outputs, settings, opt-in autostart, and complete packaging.

No Sway or generic desktop adapter is planned. See the
[shared behaviour contract](../../docs/behaviour.md).
