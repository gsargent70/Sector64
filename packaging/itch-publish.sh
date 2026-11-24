#!/usr/bin/env sh
# Publish a GitHub release of Sector64 to itch.io with butler.
#
#   sh packaging/itch-publish.sh              # the version in Cargo.toml
#   sh packaging/itch-publish.sh 1.0.4        # a specific release
#   sh packaging/itch-publish.sh --dry-run    # list what would be pushed
#
# It downloads the release's Linux tarball, Windows zip and macOS zip (releases
# before 1.0.11 have no macOS one), checks them against SHA256SUMS, unpacks
# them and pushes each folder to its own channel (`linux`, `windows`, `mac`)
# with --userversion set to the release version. Butler pushes plain folders,
# not archives, so later releases upload only what changed. Each folder gets
# an `.itch.toml` so the itch app launches the GUI rather than whichever CLI
# tool its heuristics pick. A channel whose files haven't changed is skipped
# (--if-changed), so re-running is harmless.
#
# Needs: curl, sha256sum, tar, unzip and butler (on PATH or $BUTLER; get it
# from https://itchio.itch.io/butler), logged in once with `butler login` or
# given $BUTLER_API_KEY. The target page defaults to gsargent/sector64;
# override it with $ITCH_TARGET.
set -eu

cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

REPO=gsargent70/Sector64
TARGET="${ITCH_TARGET:-gsargent/sector64}"
BUTLER="${BUTLER:-butler}"

dry_run=
version=
for arg in "$@"; do
    case "$arg" in
        --dry-run) dry_run=--dry-run ;;
        -h|--help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*) echo "error: unknown option '$arg'" >&2; exit 2 ;;
        *) version="${arg#v}" ;;
    esac
done
if [ -z "$version" ]; then
    version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
fi

for tool in curl sha256sum tar unzip; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: '$tool' not found" >&2
        exit 1
    fi
done
if ! command -v "$BUTLER" >/dev/null 2>&1; then
    echo "error: butler not found. Install it from https://itchio.itch.io/butler," >&2
    echo "       or point \$BUTLER at the binary." >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

linux="sector64-$version-x86_64-linux.tar.gz"
windows="sector64-$version-x86_64-windows.zip"
mac="sector64-$version-universal-macos.zip"
url="https://github.com/$REPO/releases/download/v$version"

echo "Downloading the v$version release from GitHub…"
fetch() {
    curl -fsSL -o "$work/$1" "$url/$1" || {
        echo "error: couldn't download $url/$1 (is v$version released?)" >&2
        exit 1
    }
}
fetch SHA256SUMS
channels="linux windows"
fetch "$linux"
fetch "$windows"
if grep -qF "$mac" "$work/SHA256SUMS"; then
    channels="$channels mac"
    fetch "$mac"
fi
(cd "$work" && sha256sum -c SHA256SUMS)

# unpack_zip <zip> <channel>: a zip holds a single top-level folder; push its
# contents.
unpack_zip() {
    mkdir "$work/unzip"
    unzip -q "$work/$1" -d "$work/unzip"
    mv "$work"/unzip/* "$work/$2"
    rmdir "$work/unzip"
}
mkdir "$work/linux"
tar -xzf "$work/$linux" -C "$work/linux" --strip-components=1
unpack_zip "$windows" windows

# itch_toml <channel> <program>
itch_toml() {
    printf '[[actions]]\nname = "Sector64"\npath = "%s"\n' "$2" >"$work/$1/.itch.toml"
}
itch_toml linux bin/sector64-gui
itch_toml windows Sector64.exe
case "$channels" in *mac*)
    unpack_zip "$mac" mac
    itch_toml mac Sector64.app
    ;;
esac

for channel in $channels; do
    "$BUTLER" validate "$work/$channel" >/dev/null
    echo "Pushing $channel to $TARGET:$channel (v$version)…"
    "$BUTLER" push "$work/$channel" "$TARGET:$channel" --userversion "$version" --if-changed $dry_run
done

if [ -z "$dry_run" ]; then
    for channel in $channels; do
        "$BUTLER" status "$TARGET:$channel"
    done
fi
