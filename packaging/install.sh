#!/usr/bin/env sh
# Build Sector64 from source and install it for the current user — no root.
#
# Installs the GUI and every command-line tool to ~/.local/bin, a desktop
# launcher and app icon, and the documentation. Honours PREFIX and
# XDG_DATA_HOME; everything lands under your home directory by default.
#
#   sh packaging/install.sh            # build + install
#   PREFIX=/tmp/s64 sh packaging/install.sh   # install elsewhere
#
# Uninstall with packaging/uninstall.sh (same env vars).
set -eu

cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: 'cargo' is not installed. Install Rust from https://rustup.rs" >&2
    exit 1
fi

# The command-line tools (the GUI is added separately).
CLI="s64ls s64extract s64val s64write s64pack s64arc s64lha s64wr3 s64cvt s64tap"

# Destinations (per-user, XDG-aware).
PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS="$DATA/applications"
ICONS="$DATA/icons/hicolor/scalable/apps"
DOC="$DATA/doc/sector64"

echo "Building (release)…"
cargo build --release -p sector64 -p sector64-gui

echo "Installing to $BIN …"
mkdir -p "$BIN" "$APPS" "$ICONS" "$DOC"

# GUI + CLI binaries.
install -m755 target/release/sector64-gui "$BIN/sector64-gui"
for t in $CLI; do
    install -m755 "target/release/$t" "$BIN/$t"
done

# Desktop launcher (point Exec at the installed binary) and icon.
install -m644 packaging/sector64.desktop "$APPS/sector64.desktop"
sed -i "s|^Exec=.*|Exec=$BIN/sector64-gui %f|" "$APPS/sector64.desktop"
install -m644 packaging/sector64.svg "$ICONS/sector64.svg"

# Documentation.
install -m644 README.md "$DOC/README.md"
install -m644 CLI.md "$DOC/CLI.md"
install -m644 gui/MANUAL.md "$DOC/MANUAL.md"

# Refresh the desktop/icon caches if the tools are present (non-fatal).
command -v update-desktop-database >/dev/null 2>&1 && \
    update-desktop-database "$APPS" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -q -t -f "$DATA/icons/hicolor" >/dev/null 2>&1 || true

echo
echo "Installed:"
echo "  GUI : $BIN/sector64-gui   (also in your app launcher as \"Sector64\")"
echo "  CLI : $CLI"
echo "  docs: $DOC"
echo

case ":$PATH:" in
    *":$BIN:"*) ;;
    *) echo "NOTE: $BIN is not on your PATH. Add it, e.g.:"
       echo "      echo 'export PATH=\"$BIN:\$PATH\"' >> ~/.profile" ;;
esac
