#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
# shellcheck source=environment.sh
source "$ROOT_DIR/scripts/environment.sh"

cd "$ROOT_DIR"
cargo build --locked --release --target-dir "$ROOT_DIR/target"
install -d "$PREFIX/bin" "$PREFIX/libexec/lite-switch"
STAGED="$(mktemp -d "$PREFIX/libexec/lite-switch/.install.XXXXXX")"
trap 'rm -rf "$STAGED"' EXIT
install -m 755 target/release/lite-switch "$STAGED/lite-switch"
install -m 644 scripts/environment.sh "$STAGED/environment.sh"
cat > "$STAGED/launcher" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
BIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$BIN_DIR/../libexec/lite-switch"
source "$APP_DIR/environment.sh"
export GDK_BACKEND=x11
export LITE_SWITCH_LAUNCHER="$BIN_DIR/lite-switch"
exec "$APP_DIR/lite-switch" "$@"
SH
chmod 755 "$STAGED/launcher"
# Replace executable files atomically; existing processes retain their old inode.
mv -f "$STAGED/lite-switch" "$PREFIX/libexec/lite-switch/lite-switch"
mv -f "$STAGED/environment.sh" "$PREFIX/libexec/lite-switch/environment.sh"
mv -f "$STAGED/launcher" "$PREFIX/bin/lite-switch"
printf 'Installed %s\nNo i3 configuration or autostart files were changed.\n' "$PREFIX/bin/lite-switch"
printf 'Quit any old resident instance, then run: lite-switch show\n'
