# Lite Switch

Lite Switch launches apps, switches windows and Vivaldi tabs, manages windows, and selects sound outputs.

This monorepo contains independent native implementations:

| Platform | Location | Status |
| --- | --- | --- |
| macOS 14+ | `Package.swift`, `Sources/`, `scripts/` | Existing menu-bar application |
| Xfce/X11 | [`apps/xfce/`](apps/xfce/README.md) | Active Rust/GTK4 launcher with X11 discovery/focus, search, cycling and assignments |
| i3/X11 | [`apps/i3/`](apps/i3/README.md) | Preserved legacy GTK4 implementation; superseded as the active target |

The macOS files remain in their original locations. Each app owns its dependencies,
tests, and packaging. Shared [behaviour](docs/behaviour.md) and
[acceptance scenarios](docs/test-cases/acceptance.md) describe the intended parity;
runtime code is not shared. The Vivaldi extension will live under
`extensions/vivaldi/` when implemented.

For Linux build instructions and implementation order, see
[`apps/xfce/README.md`](apps/xfce/README.md). The instructions below apply to macOS.

## macOS usage

Press **⌥ Space** to open one searchable panel containing:

- installed applications (ranked using local launch history)
- running applications and their windows
- open Vivaldi tabs
- actions to fill, move, or snap the previously active window

Select a result and press **Return** to activate it. Click **+ key** beside any result, then press a letter, number, or arrow key to assign a direct **Option-key** shortcut. Application shortcuts cycle through unassigned windows and launch the app when needed. Window and Vivaldi-tab assignments last for the current app session; application and window-action assignments persist.

## Build and run

```sh
./scripts/package-and-run.sh
```

The packaged app is written to `dist/Lite Switch.app`. Lite Switch targets macOS 14 or later and uses only Apple frameworks.

Grant Lite Switch access in **System Settings → Privacy & Security → Accessibility** to search, focus, and manage windows. Vivaldi tab support also requires allowing Lite Switch to automate Vivaldi when macOS asks.

Open the menu-bar item to see available sound outputs, identify the active output by its checkmark, or switch devices. Use **Settings…** to change the launcher shortcut or enable Launch at Login.

## Checks

```sh
swift run LiteSwitchCoreChecks
```

## Privacy

Lite Switch makes no runtime network requests and includes no telemetry. The macOS application uses only Apple frameworks; Linux builds have their own Rust/native dependencies. Application usage history and shortcut assignments remain local.

## License

MIT. See [LICENSE](LICENSE).
