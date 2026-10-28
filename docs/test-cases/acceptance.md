# Acceptance scenarios

These scenarios guide native unit tests and manual desktop checks. Platform-only
cases are labelled. A checked-in scenario is not evidence that it has passed.

## Search

- An exact app-name match ranks above prefix, word-prefix, and fuzzy matches,
  even if the weaker match has much more usage history.
- Case and diacritic differences do not prevent an otherwise matching result.
- With an empty query, recent/frequent apps outrank never-used apps.
- Repeated queries against unchanged data produce stable ordering.

## Assignments and cycling

- Assigning the same key to a new target replaces the previous target.
- Assigning the same target to a new key removes its previous binding.
- Restarting Lite Switch preserves app/action assignments but clears window/tab
  assignments.
- An app shortcut cycles through eligible windows, excluding directly assigned
  windows; launch-if-needed follows its stored preference.
- Closing a window/tab makes its session assignment harmless, including if an
  underlying identifier is subsequently reused.

## Focus and window actions

- Opening then cancelling the panel restores the previous window's focus.
- Activating a window on another workspace/output focuses that exact window.
- Opening the panel then choosing a window action changes the previous window,
  not the panel.
- [i3] The panel floats without changing tiled layout geometry.
- [i3] A half-screen action floats only the target window and respects usable
  output geometry; return-to-tiling restores tiling participation.
- [i3] Moving to another output handles different resolutions and reserved bars.

## Xfce/X11 integration

- [Xfce] Refuse incompatible window managers rather than silently using i3 or
  assuming arbitrary X11 desktops implement the expected behaviour.
- [Xfce] Discover Unicode titles, ICCCM instance/class, workspace/output and outer
  frame geometry. Exclude panels and desktop surfaces; retain minimized windows.
- [Xfce] Activation changes to the correct workspace and restores minimized
  windows, then verifies the requested window became active.
- [Xfce] Destroying/recreating an identical window with exactly the same XID must
  invalidate its old session identity even if snapshots never observe a gap.
- [Xfce] Known app windows do not duplicate default/app-name results. Specific
  title search and `windows:` retain individual-window activation/assignment;
  known windows use app icons. Directly assigned and unmatched windows remain visible.
- [Xfce] Explicit class/instance/role rules resolve ambiguous applications, persist
  across restarts and update cycling immediately. Overlap stays ambiguous,
  identical criteria are rejected and ambiguity never launches a duplicate.
- [Xfce] Approved shortcuts take priority over conflicts, with backups/reporting
  and exact restoration when removed or moved; unrelated keys remain intact.
- [Xfce] Automatic sync requires a separate explicit opt-in. Once enabled, GUI
  and CLI assignment/move/removal and shortcut preference edits apply immediately.
  Session closure/restart releases expired chords; uninstall is not undone.
  Sync failures retain the preference but report failure, preserving outside edits.

## Browser, audio, and lifecycle

- Selecting a Vivaldi tab activates the correct tab and containing browser window,
  including with multiple windows/profiles.
- Browser restarts invalidate old tab assignments without breaking app shortcuts.
- Output hotplugging refreshes the audio list and active-default indicator.
- Changing the default audio output reports failure accurately and does not
  promise that existing playback moved unless it actually did.
- Relaunching the app does not create duplicate resident processes.
- [i3] Approved installation gives Lite Switch priority over existing default-mode
  shortcuts, reporting each displaced binding and keeping private backups.
- [i3] Priority replacement handles variables, nested/glob includes, continuations,
  physical bindcodes and release bindings without changing unrelated named modes.
- [i3] Moving/removing a chord restores its displaced bindings exactly. Checks do
  not mutate user configuration; edited displacement blocks and concurrent edits
  are protected rather than overwritten.
