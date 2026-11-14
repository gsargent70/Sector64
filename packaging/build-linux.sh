#!/usr/bin/env sh
# Build Sector64 for x86_64 Linux and package the prebuilt release tarball:
# the GUI, every command-line tool, a desktop launcher + icon, and the docs.
#
#   sh packaging/build-linux.sh        # -> dist/sector64-<ver>-x86_64-linux.tar.gz
#
# The binaries link dynamically against the build machine's glibc, so build
# on the oldest distribution you want to support (the release workflow uses
# an older Ubuntu LTS for that reason).
set -eu

cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

CLI="s64ls s64extract s64val s64write s64pack s64arc s64lha s64wr3 s64cvt s64tap"

for tool in cargo tar; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: '$tool' not found" >&2
        exit 1
    fi
done

VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
NAME="sector64-$VER-x86_64-linux"
OUT="dist/$NAME"

echo "Building $NAME (release)…"
RUSTFLAGS="${RUSTFLAGS:-} -C strip=symbols" \
    cargo build --release -p sector64 -p sector64-gui

REL="${CARGO_TARGET_DIR:-target}/release"
rm -rf "$OUT" "dist/$NAME.tar.gz"
mkdir -p "$OUT/bin" "$OUT/doc" "$OUT/share/applications" \
    "$OUT/share/icons/hicolor/scalable/apps"

install -m755 "$REL/sector64-gui" "$OUT/bin/"
for t in $CLI; do
    install -m755 "$REL/$t" "$OUT/bin/"
done
install -m644 packaging/sector64.desktop "$OUT/share/applications/"
install -m644 packaging/sector64.svg "$OUT/share/icons/hicolor/scalable/apps/"
install -m644 README.md CLI.md gui/MANUAL.md LICENSE-MIT LICENSE-APACHE "$OUT/doc/"

cat > "$OUT/INSTALL.txt" <<EOF
Sector64 $VER — prebuilt x86_64 Linux binaries
$(printf '%s' "Sector64 $VER — prebuilt x86_64 Linux binaries" | sed 's/./=/g')

These binaries are dynamically linked against glibc. If they fail to start
on an older distribution, build from source instead (see the project
README; \`sh packaging/install.sh\`).

Quick run (no install):
    ./bin/sector64-gui                 # the graphical browser
    ./bin/s64ls disk.d64               # a command-line tool

Install for your user (no root):
    mkdir -p ~/.local/bin
    cp bin/* ~/.local/bin/             # ensure ~/.local/bin is on your PATH

    # Optional: desktop launcher + icon
    cp share/applications/sector64.desktop ~/.local/share/applications/
    sed -i "s|^Exec=.*|Exec=\$HOME/.local/bin/sector64-gui %f|" \\
        ~/.local/share/applications/sector64.desktop
    mkdir -p ~/.local/share/icons/hicolor/scalable/apps
    cp share/icons/hicolor/scalable/apps/sector64.svg \\
        ~/.local/share/icons/hicolor/scalable/apps/

Tools: sector64-gui (GUI), s64ls, s64extract, s64val, s64write, s64pack,
       s64arc, s64lha, s64wr3, s64cvt, s64tap. See doc/MANUAL.md (GUI) and
       doc/CLI.md (command line).
EOF

tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
echo "Done: dist/$NAME.tar.gz"
