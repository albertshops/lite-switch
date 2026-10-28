# Workspace-scoped floating experiment

The current experiment makes **new windows on workspace 9** float by default.
It does not make floating global, move existing windows, change shortcuts, or
install Lite Switch's managed bindings/autostart.

```i3
# BEGIN LITE SWITCH WORKSPACE 9 FLOATING TRIAL
# Scoped experiment: new windows on workspace 9 float; other workspaces unchanged.
# Alt+Shift+Space still toggles floating/tiling; Alt+left-drag moves floaters.
for_window [workspace="^9$"] floating enable
# END LITE SWITCH WORKSPACE 9 FLOATING TRIAL
```

The workspace criterion deliberately includes legacy X11 applications without a
`normal` window-type hint (xterm reports `unknown`). Docks have no workspace.
The exact name `9` is intentional; renaming it to `9: something` would require
updating the criterion. Existing tiling workspaces remain available.

## Try it

With the current `$mod Mod1` configuration:

- **Alt+9**: switch to workspace 9.
- **Alt+Enter**: open a terminal there; it starts floating.
- **Alt+left-drag**: move a floating window.
- **Alt+Shift+Space**: toggle the focused window between floating and tiling.
- **Alt+1**: return to workspace 1.

Toggling back to tiling joins i3's existing layout tree; this is not a separate
floating desktop mode. No fill/half-screen shortcuts have been added by this
experiment. Making floating the global default requires a separate decision
after trying the scoped setup.

## Backup and reversal

The original root configuration, proposed configuration, mode, checksum, and
validation logs are saved privately under:

```
~/.config/lite-switch/backups/floating-trial-<unique-id>/
```

To reverse the experiment while preserving subsequent configuration edits,
remove only the marked trial block from `~/.config/i3/config`, validate with
`i3 -C -c ~/.config/i3/config`, then run `i3-msg reload`.

If no other edits have occurred, compare the current file with that backup's
`proposed.conf` before restoring `original.conf`. Do not blindly restore a whole
backup over newer edits. Reloading reruns existing `exec_always` directives.
Removing the rule changes future matching only; it does not retile existing
floating windows. Use the existing floating toggle for those.

## Verification

`apps/i3/tests/floating_trial.py` starts a private Xvfb/i3 server and verifies
workspace scoping, legacy xterm matching, the existing keyboard toggle, and
unchanged geometry on another workspace:

```sh
cd apps/i3
source scripts/environment.sh
python3 tests/floating_trial.py
```

The live experiment was also validated by opening a temporary xterm on workspace
9, checking that it floated, toggling it to tiling, then closing only that test
window and restoring previous focus. The three pre-existing application windows
retained their geometry and floating state.
