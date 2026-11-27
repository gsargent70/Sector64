#!/usr/bin/env sh
# Build Sector64 for macOS (on a Mac) and package it as a zip: Sector64.app,
# every command-line tool, and the docs. The binaries are universal (Apple
# Silicon and Intel).
#
# Needs the Xcode command-line tools and both Rust targets:
#   xcode-select --install
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#
#   sh packaging/build-macos.sh        # -> dist/sector64-<ver>-universal-macos.zip
#
# Nothing is signed with an Apple Developer ID or notarized. The ad-hoc
# signatures below cost nothing and only let the binaries run at all (Apple
# Silicon refuses unsigned code); Gatekeeper still blocks a download until the
# user allows it, as README-MACOS.txt explains.
set -eu

cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

CLI="s64ls s64extract s64val s64write s64pack s64arc s64lha s64wr3 s64cvt s64tap s64cmp s64basic s64conv"
ARM=aarch64-apple-darwin
INTEL=x86_64-apple-darwin
# The oldest macOS to support: the first that runs on both kinds of Mac.
MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
export MACOSX_DEPLOYMENT_TARGET

for tool in cargo lipo codesign plutil zip; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: '$tool' not found (see the header of this script)" >&2
        exit 1
    fi
done

VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
NAME="sector64-$VER-universal-macos"
OUT="dist/$NAME"

echo "Building $NAME (release)…"
for target in $ARM $INTEL; do
    RUSTFLAGS="${RUSTFLAGS:-} -C strip=symbols" \
        cargo build --release --target "$target" -p sector64 -p sector64-gui
done

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
APP="$OUT/Sector64.app/Contents"
rm -rf "$OUT" "dist/$NAME.zip"
mkdir -p "$APP/MacOS" "$APP/Resources" "$OUT/bin" "$OUT/doc"

# universal <built name> <destination>
universal() {
    lipo -create -output "$2" \
        "$TARGET_DIR/$ARM/release/$1" "$TARGET_DIR/$INTEL/release/$1"
    chmod 755 "$2"
}

universal sector64-gui "$APP/MacOS/Sector64"
for t in $CLI; do
    universal "$t" "$OUT/bin/$t"
    codesign --force --sign - "$OUT/bin/$t"
done

cp gui/assets/sector64.icns "$APP/Resources/"
printf 'APPL????' >"$APP/PkgInfo"
cat >"$APP/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>Sector64</string>
    <key>CFBundleExecutable</key>
    <string>Sector64</string>
    <key>CFBundleIconFile</key>
    <string>sector64</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.gsargent70.Sector64</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Sector64</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VER</string>
    <key>CFBundleVersion</key>
    <string>$VER</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.utilities</string>
    <key>LSMinimumSystemVersion</key>
    <string>$MACOSX_DEPLOYMENT_TARGET</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF
plutil -lint "$APP/Info.plist"
codesign --force --sign - "$OUT/Sector64.app"
codesign --verify --strict --deep "$OUT/Sector64.app"

cp README.md CLI.md gui/MANUAL.md LICENSE-MIT LICENSE-APACHE "$OUT/doc/"
cat >"$OUT/README-MACOS.txt" <<'EOF'
Sector64 for macOS
==================

EXPERIMENTAL. The Intel version has been tried by hand on macOS 14 (Sonoma).
The Apple Silicon version hasn't: GitHub's build machines compile it, run its
tests and check that it starts. If something doesn't work (or does!), please
say so: https://github.com/gsargent70/Sector64/issues

It's built for macOS 11 or newer, on Apple Silicon and Intel Macs.

Opening it the first time
-------------------------

The app isn't signed with an Apple Developer ID, so macOS blocks it until you
allow it. Either:

  - Open Sector64.app, dismiss the warning, then go to System Settings >
    Privacy & Security, scroll down to the message about Sector64 and click
    Open Anyway. (On macOS 14 or older, you can also right-click the app and
    choose Open.)

  - Or, in Terminal, clear the download mark from this whole folder, which
    also covers the command-line tools:

        xattr -dr com.apple.quarantine /path/to/this/folder

You only need to do this once per download. You can move Sector64.app to
Applications.

Command-line tools
------------------

The s64* programs are in bin/ (see doc/CLI.md). Allow them with the xattr
command above, then run them from Terminal, or add bin/ to your PATH.

Good to know
------------

  - Wherever the manual says Ctrl, use Cmd.
  - The C64 Pro Mono font isn't included, for licensing reasons. Open the
    .ttf in Font Book to install it and Sector64 will use it.
  - Opening a disk image by double-clicking it in Finder isn't supported yet.
    Use File > Open, or drag the file onto the Sector64 window.
EOF

(cd dist && zip -qryX "$NAME.zip" "$NAME")
echo "Done: dist/$NAME.zip"
