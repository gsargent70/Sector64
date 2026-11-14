#!/usr/bin/env sh
# Cross-compile Sector64 for 64-bit Windows from Linux and package it as a
# portable zip: the GUI, every command-line tool, and the docs.
#
# Needs the Rust target and the MinGW-w64 linker:
#   rustup target add x86_64-pc-windows-gnu
#   sudo pacman -S mingw-w64-gcc        # Debian/Ubuntu: apt install gcc-mingw-w64-x86-64
#
#   sh packaging/build-windows.sh      # -> dist/sector64-<ver>-x86_64-windows.zip
set -eu

cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

TARGET=x86_64-pc-windows-gnu
CLI="s64ls s64extract s64val s64write s64pack s64arc s64lha s64wr3 s64cvt s64tap"

for tool in cargo x86_64-w64-mingw32-gcc zip; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: '$tool' not found (see the header of this script)" >&2
        exit 1
    fi
done

VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
NAME="sector64-$VER-x86_64-windows"
OUT="dist/$NAME"

echo "Building $NAME (release, $TARGET)…"
# Link the MinGW C runtime statically so the .exe files need no extra DLLs,
# and strip symbols to keep them small.
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static -C strip=symbols" \
    cargo build --release --target "$TARGET" -p sector64 -p sector64-gui

REL="${CARGO_TARGET_DIR:-target}/$TARGET/release"
rm -rf "$OUT" "dist/$NAME.zip"
mkdir -p "$OUT"
cp "$REL/sector64-gui.exe" "$OUT/Sector64.exe"
for t in $CLI; do
    cp "$REL/$t.exe" "$OUT/"
done
cp README.md CLI.md LICENSE-MIT LICENSE-APACHE "$OUT/"
cp gui/MANUAL.md "$OUT/MANUAL.md"

cat > "$OUT/README-WINDOWS.txt" <<'EOF'
Sector64 for Windows (portable)

Double-click Sector64.exe to start the disk-image browser. Nothing needs
installing; the folder can live anywhere, such as on a USB stick.

The s64*.exe files are the command-line tools (see CLI.md). Run them from
Command Prompt or PowerShell inside this folder, or add the folder to PATH.

Optional retro font: Sector64 uses the "C64 Pro Mono" TrueType font when it
can find it. It is not included for licensing reasons. Install it for your
user, or drop C64_Pro_Mono-STYLE.ttf into this folder next to Sector64.exe.
EOF

(cd dist && zip -qr "$NAME.zip" "$NAME")
echo "Done: dist/$NAME.zip"
