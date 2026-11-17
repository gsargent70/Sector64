#!/usr/bin/env sh
# Build the Sector64 GUI from this checkout and launch it — handy for trying
# out a branch without installing anything.
#
#   sh packaging/run-gui.sh                  # start screen
#   sh packaging/run-gui.sh disk.d64         # open a disk or archive
#   SECTOR64_X11=1 sh packaging/run-gui.sh   # via X11, for drag-and-drop on Wayland
#
# Uses the toolchain pinned in mise.toml when mise is installed, otherwise
# whatever `cargo` is on PATH. The build goes to target/release as usual.
set -eu

# Resolve a relative file argument before changing directory.
if [ $# -gt 0 ] && [ -e "$1" ]; then
    file=$(CDPATH= cd -- "$(dirname -- "$1")" && pwd)/$(basename -- "$1")
    shift
    set -- "$file" "$@"
fi

cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

if command -v mise >/dev/null 2>&1 && toolchain=$(mise where rust 2>/dev/null); then
    # mise's Rust carries its own cargo/rustup; don't let a global
    # CARGO_HOME / RUSTUP_HOME point it elsewhere.
    unset CARGO_HOME RUSTUP_HOME
    PATH="$toolchain:$PATH"
    export PATH
elif ! command -v cargo >/dev/null 2>&1; then
    echo "error: 'cargo' is not installed. Install Rust from https://rustup.rs" >&2
    exit 1
fi

echo "Building the Sector64 GUI ($(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo 'this checkout'))…"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}" cargo build --release -p sector64-gui

exec "${CARGO_TARGET_DIR:-target}/release/sector64-gui" "$@"
