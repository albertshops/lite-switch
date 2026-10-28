#!/usr/bin/env bash
# Source this file before building/running on hosts without system GTK4 packages.
# Ordinary installations use the distribution's libgtk-4-dev package instead.
SDK="${LITE_SWITCH_GTK_SDK:-$HOME/.local/share/lite-switch/gtk4-sdk}"
if ! pkg-config --exists gtk4 && [[ -f "$SDK/usr/lib/x86_64-linux-gnu/pkgconfig/gtk4.pc" ]]; then
    export PKG_CONFIG_SYSROOT_DIR="$SDK"
    export PKG_CONFIG_PATH="$SDK/usr/lib/x86_64-linux-gnu/pkgconfig:$SDK/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    export LIBRARY_PATH="$SDK/usr/lib/x86_64-linux-gnu${LIBRARY_PATH:+:$LIBRARY_PATH}"
fi
# Runtime libraries are also needed by the installed application on SDK-only hosts.
if [[ -n "${PKG_CONFIG_SYSROOT_DIR:-}" && "${PKG_CONFIG_SYSROOT_DIR:-}" == "$SDK" ]]; then
    export LD_LIBRARY_PATH="$SDK/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export XDG_DATA_DIRS="$SDK/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
fi
unset SDK
