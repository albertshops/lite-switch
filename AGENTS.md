# Development Instructions

## Repository layout

- macOS: `Package.swift`, `Sources/`, and `scripts/package-and-run.sh` (kept in
  their original locations).
- Xfce/X11: active Linux target, independent Rust workspace at `apps/xfce/`;
  follow its `AGENTS.md`.
- i3/X11: preserved legacy workspace at `apps/i3/`; keep it independently
  buildable, but do not develop new i3 features unless explicitly requested.
- Shared behaviour and acceptance scenarios: `docs/`.
- Future Vivaldi extension: `extensions/vivaldi/` when implemented.

Keep platform dependencies, builds, and packaging independent. Share behaviour
specifications and test scenarios rather than runtime code. Linux-only changes
must not require a macOS build. Shared behaviour changes require validation on
both platforms where available; report unavailable checks explicitly.

## macOS changes

After every macOS code or configuration change:

1. Run `swift run LiteSwitchCoreChecks` from the repository root.
2. Repackage and relaunch with `./scripts/package-and-run.sh`.

Do not consider a macOS change complete until both commands succeed and the
relaunched app is running. Do not relocate the existing macOS app as part of
Linux feature work.
