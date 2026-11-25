#!/usr/bin/env sh
# Check a macOS package on a Mac: the release workflow runs this on GitHub's
# Apple Silicon and Intel machines.
#
#   sh packaging/check-macos.sh <package.zip> <output folder>
#
# It unpacks the zip, checks the signatures and architectures, runs the
# command-line tools on a disk it makes, then starts the app on that disk,
# takes a screenshot into the output folder, and quits it.
set -eu

zip=$1
mkdir -p "$2"
out=$(CDPATH= cd -- "$2" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

unzip -q "$zip" -d "$work"
pkg=$(echo "$work"/sector64-*-universal-macos)
app="$pkg/Sector64.app"
bin="$pkg/bin"

echo "== package"
codesign --verify --strict --deep --verbose "$app"
for f in "$app/Contents/MacOS/Sector64" "$bin"/*; do
    case "$(lipo -archs "$f")" in
        *x86_64*arm64* | *arm64*x86_64*) ;;
        *) echo "error: $f is not universal" >&2; exit 1 ;;
    esac
    codesign --verify --strict "$f"
done
n=$(find "$bin" -type f | wc -l)
test "$n" -eq 12 || { echo "error: $n tools, expected 12" >&2; exit 1; }

echo "== command-line tools ($(uname -m))"
cd "$work"
printf '10 PRINT "HELLO FROM A MAC"\n20 GOTO 10\n' >hello.txt
"$bin/s64basic" hello.txt hello.prg
"$bin/s64write" format disk.d64 "MAC DISK" 2A
"$bin/s64write" add disk.d64 HELLO hello.prg prg
"$bin/s64ls" disk.d64 | tee ls.txt
grep -q HELLO ls.txt
"$bin/s64val" disk.d64
"$bin/s64extract" disk.d64 HELLO >back.prg
cmp hello.prg back.prg
cp disk.d64 copy.d64
"$bin/s64cmp" disk.d64 copy.d64
"$bin/s64pack" hello.t64 hello.prg
"$bin/s64ls" hello.t64 >/dev/null 2>&1 || true

echo "== app"
open "$app" --args "$work/disk.d64"
sleep 15
pid=$(pgrep -x Sector64) || { echo "error: the app isn't running" >&2; exit 1; }
screencapture -x "$out/sector64-$(uname -m).png" || echo "(no screenshot)"
sleep 5
kill -0 "$pid"
echo "running as $pid; asking it to quit"
osascript -e 'tell application "Sector64" to quit' || echo "(osascript failed)"
i=0
while kill -0 "$pid" 2>/dev/null; do
    i=$((i + 1))
    if [ $i -gt 20 ]; then
        echo "error: it didn't quit" >&2
        kill "$pid"
        exit 1
    fi
    sleep 1
done
ls -l "$HOME/Library/Application Support/sector64-gui"
echo "All good."
