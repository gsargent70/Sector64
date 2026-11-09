#!/usr/bin/env sh
# Remove a per-user Sector64 install (the inverse of install.sh).
# Honours the same PREFIX and XDG_DATA_HOME variables.
set -eu

CLI="s64ls s64extract s64val s64write s64pack s64arc s64lha s64wr3 s64cvt s64tap"

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS="$DATA/applications"
ICONS="$DATA/icons/hicolor/scalable/apps"
DOC="$DATA/doc/sector64"

echo "Removing Sector64 from $BIN and $DATA …"
rm -f "$BIN/sector64-gui"
for t in $CLI; do
    rm -f "$BIN/$t"
done
rm -f "$APPS/sector64.desktop"
rm -f "$ICONS/sector64.svg"
rm -rf "$DOC"

command -v update-desktop-database >/dev/null 2>&1 && \
    update-desktop-database "$APPS" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -q -t -f "$DATA/icons/hicolor" >/dev/null 2>&1 || true

echo "Done. (The window-state/output-folder settings in"
echo "~/.local/share/sector64 and your own disk images are left untouched.)"
