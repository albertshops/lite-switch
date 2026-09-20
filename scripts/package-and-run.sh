#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
APP_PATH="$DIST_DIR/Lite Switch.app"
TEMP_APP_PATH="$DIST_DIR/.Lite Switch.app.tmp"
EXECUTABLE_PATH="$TEMP_APP_PATH/Contents/MacOS/LiteSwitch"

cd "$ROOT_DIR"
echo "Building Lite Switch..."
swift build -c release --product LiteSwitch

rm -rf "$TEMP_APP_PATH"
mkdir -p "$TEMP_APP_PATH/Contents/MacOS" "$TEMP_APP_PATH/Contents/Resources"
trap 'rm -rf "$TEMP_APP_PATH"' EXIT
cp "$ROOT_DIR/.build/release/LiteSwitch" "$EXECUTABLE_PATH"

python3 - "$ROOT_DIR/Sources/LiteSwitch/Resources/Info.plist" "$TEMP_APP_PATH/Contents/Info.plist" <<'PY'
from pathlib import Path
import sys
source, destination = map(Path, sys.argv[1:])
contents = source.read_text()
for placeholder, value in {
    "$(DEVELOPMENT_LANGUAGE)": "en",
    "$(EXECUTABLE_NAME)": "LiteSwitch",
    "$(PRODUCT_BUNDLE_IDENTIFIER)": "com.albertshops.liteswitch",
    "$(PRODUCT_NAME)": "Lite Switch",
    "$(PRODUCT_BUNDLE_PACKAGE_TYPE)": "APPL",
    "$(MARKETING_VERSION)": "0.1.0",
    "$(CURRENT_PROJECT_VERSION)": "1",
    "$(MACOSX_DEPLOYMENT_TARGET)": "14.0",
}.items():
    contents = contents.replace(placeholder, value)
destination.write_text(contents)
PY

chmod +x "$EXECUTABLE_PATH"
codesign --force --deep --sign - "$TEMP_APP_PATH"
codesign --verify --deep --strict "$TEMP_APP_PATH"

if pgrep -x LiteSwitch >/dev/null 2>&1; then
    echo "Stopping the current Lite Switch process..."
    killall LiteSwitch
    for _ in {1..50}; do
        pgrep -x LiteSwitch >/dev/null 2>&1 || break
        sleep 0.1
    done
fi

rm -rf "$APP_PATH"
mv "$TEMP_APP_PATH" "$APP_PATH"
trap - EXIT
open "$APP_PATH"
echo "Lite Switch was packaged and launched successfully."
