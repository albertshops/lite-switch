#!/usr/bin/env bash
# Optional Debian/Ubuntu development fallback when sudo is unavailable.
# Downloads/extracts trusted APT packages privately; runs no package scripts.
set -euo pipefail
WITH_TESTS=false
if [[ "${1:-}" == "--with-desktop-tests" && $# -eq 1 ]]; then
    WITH_TESTS=true
elif [[ $# -ne 0 ]]; then
    echo "Usage: $0 [--with-desktop-tests]" >&2
    exit 2
fi
SDK="${LITE_SWITCH_GTK_SDK:-$HOME/.local/share/lite-switch/gtk4-sdk}"
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/lite-switch/gtk4-debs"
mkdir -p "$SDK" "$CACHE"
PACKAGES="$(mktemp)"
trap 'rm -f "$PACKAGES"' EXIT
apt-get --simulate install --no-install-recommends libgtk-4-dev |
    awk '/^Inst / { print $2 }' > "$PACKAGES"
# Already-installed runtimes must also be extracted, otherwise private dev
# symlinks can dangle or cause accidental static linking.
printf '%s\n' libgtk-4-dev libgtk-4-1 libgtk-4-common libpango-1.0-0 \
    libpangocairo-1.0-0 libpangoft2-1.0-0 libharfbuzz0b libgdk-pixbuf-2.0-0 \
    libcairo2 libvulkan1 libx11-6 >> "$PACKAGES"
if $WITH_TESTS; then
    printf '%s\n' xvfb libxfont2 xserver-common xdotool libxdo3 >> "$PACKAGES"
fi
sort -u -o "$PACKAGES" "$PACKAGES"
cd "$CACHE"
xargs -a "$PACKAGES" -n 12 apt-get download
while IFS= read -r -d '' package; do
    dpkg-deb -x "$package" "$SDK"
done < <(find "$CACHE" -maxdepth 1 -name '*.deb' -print0)
echo "GTK SDK extracted to $SDK"
echo "Source apps/i3/scripts/environment.sh before cargo commands."
