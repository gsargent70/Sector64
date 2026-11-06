#!/usr/bin/env sh
# Build the GUI in release and install it for the current user.
set -eu
cd "$(dirname "$0")/.."
cargo build --release -p sector64-gui
BIN="$HOME/.local/bin"
APPS="$HOME/.local/share/applications"
mkdir -p "$BIN" "$APPS"
install -m755 target/release/sector64-gui "$BIN/sector64-gui"
install -m644 packaging/sector64.desktop "$APPS/sector64.desktop"
# Point the launcher at the installed binary (absolute path is most robust).
sed -i "s|^Exec=.*|Exec=$BIN/sector64-gui %f|" "$APPS/sector64.desktop"
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APPS" >/dev/null 2>&1 || true
fi
echo "Installed sector64-gui to $BIN and a launcher to $APPS"
echo "Ensure $BIN is on your PATH."
