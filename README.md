# Lite Switch

Lite Switch combines Pinpoint’s application launcher with Twitcher’s app, window, Vivaldi-tab, window-management, and sound-output switching tools in one native macOS menu-bar app.

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

Lite Switch makes no network requests and includes no telemetry or third-party dependencies. Application usage history and shortcut assignments remain in local user defaults.

## License

MIT. See [LICENSE](LICENSE).
