#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
source "$ROOT/scripts/environment.sh"
cd "$ROOT"
cargo build --locked --release --target-dir "$ROOT/target"
install -d "$PREFIX/bin" "$PREFIX/libexec/lite-switch-xfce"
staged=$(mktemp -d "$PREFIX/libexec/lite-switch-xfce/.install.XXXXXXXX")
trap 'rm -rf "$staged"' EXIT
install -m 755 target/release/lite-switch-xfce "$staged/lite-switch-xfce"
install -m 644 scripts/environment.sh "$staged/environment.sh"
cat > "$staged/launcher" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
BIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$BIN_DIR/../libexec/lite-switch-xfce"
source "$APP_DIR/environment.sh"
export GDK_BACKEND=x11
export LITE_SWITCH_LAUNCHER="$BIN_DIR/lite-switch-xfce"
exec "$APP_DIR/lite-switch-xfce" "$@"
SH
chmod 755 "$staged/launcher"
mv -f "$staged/lite-switch-xfce" "$PREFIX/libexec/lite-switch-xfce/lite-switch-xfce"
mv -f "$staged/environment.sh" "$PREFIX/libexec/lite-switch-xfce/environment.sh"
mv -f "$staged/launcher" "$PREFIX/bin/lite-switch-xfce"
printf 'Installed %s/bin/lite-switch-xfce\nNo shortcuts/autostart changed. Existing i3 binaries remain untouched.\n' "$PREFIX"
