# Lite Switch behaviour contract

The macOS implementation remains the behavioural reference. Linux is a separate
implementation, not a shared runtime or a Swift port. These are implementation
requirements, not claims that the i3 scaffold already implements them.

## Shared behaviour

- One keyboard-first panel searches installed applications, running applications
  and windows, Vivaldi tabs, and window actions.
- Enter activates a result; Escape dismisses the panel.
- App search tiers are exact, prefix, word-prefix, then fuzzy subsequence.
  Usage history must never promote a weaker match tier above a stronger one.
  Within a tier, preserve the reference implementation's gap, usage, length,
  and alphabetical ordering.
- App usage history is local and influences empty-query ordering.
- Assignable keys include ASCII letters, numbers, and arrow keys.
- A key has one target; assigning an already assigned target moves its binding.
- App and action assignments persist. Window and tab assignments last only for
  the current Lite Switch session and must not activate stale/reused identities.
- App shortcuts cycle through eligible windows, excluding individually assigned
  windows. Preserve the reference implementation's launch-if-needed preference.
- Window actions target the window active before opening the panel, not the panel.
- Available audio outputs and the active default are visible. Distinguish changing
  the default output from moving existing playback streams.
- No telemetry or external network service. Keep history and assignments local.

## Xfce-specific decisions (active Linux target)

- Target Xfce on X11 with xfwm4, using standard X11/EWMH properties and requests;
  do not add Wayland/Sway or a generic multi-desktop abstraction.
- Preserve the useful i3 launcher/search/assignment logic by porting it into an
  independent Xfce workspace. Keep the i3 implementation available as legacy.
- Exclude panels, desktop surfaces and other non-application windows from targets.
- Track XID generations through destruction events; invalidate session targets on
  lost X11/event continuity or window-manager replacement. XIDs alone are not
  persistent identities, even if a replacement has identical title/class/PID.
- Explicit class/instance/role rules are conjunctive exact comparisons and
  override heuristics. Overlapping explicit rules remain unmatched; apps with
  rules cannot fall back heuristically when their own rule fails. Refuse
  duplicate launching when matching is ambiguous and explain the override path.
- Preserve previous-window focus and floating desktop geometry. Fill and
  half-screen placement use usable monitor geometry, distinct from fullscreen.
  The i3-specific return-to-tiling requirement does not apply to Xfce.
- Use conflict-aware Xfce shortcut integration, not i3 config includes. On approved
  installation/sync Lite Switch wins conflicts, with private backups, displacement
  reporting, restoration and concurrent-edit protection. Do not silently disable
  unrelated desktop controls or enable app autostart without approval.
- Audio/lock acceptance requiring user participation may remain pending while
  development continues; never claim audible success from device enumeration or
  trigger an unexpected lock/suspend.

## i3-specific decisions (preserved legacy implementation)

- Target i3/X11 only, using i3 IPC and container identities.
- Match running applications to desktop-entry IDs using X11 properties, with
  explicit overrides for ambiguous applications.
- Use a floating launcher that does not disrupt the tiled layout.
- Half-screen actions explicitly enable floating. Provide a return-to-tiling
  action. Fill usable output space; do not equate fill with fullscreen.
- Move-to-output behaviour must account for output geometry and reserved bars.
- Let i3 own global keybindings. Keep generated bindings in a separate include;
  detect conflicts and obtain approval before changing user configuration.
  Approved installation gives Lite Switch priority over conflicting default-mode
  bindings, with private backups and explicit replacement reporting. Restore
  displaced bindings when their Lite Switch chord moves or is removed; preserve
  unrelated keys and named modes.
- Replace macOS browser automation with a Vivaldi extension/native-messaging
  bridge. Browser window IDs are not i3 container IDs; validate their mapping.
- Store configuration/state under XDG directories.
- Audio controls must remain accessible without a tray.

## Verification

[Acceptance scenarios](test-cases/acceptance.md) are shared specifications.
They are not yet an executable cross-platform test suite. Each implementation
owns its native tests, dependencies, packaging, and release process.
