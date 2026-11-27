# Sector64 — notes for Claude

Clean-room, **zero-dependency** Rust toolkit for Commodore disk images and
archives:
- core library (`src/`)
- 14 CLI tools (`src/bin/`, documented in `CLI.md`)
- an egui GUI in `gui/` (separate workspace member)

It runs on Linux, Windows and (experimental) macOS. Releases are on GitHub
(`gsargent70/Sector64`; latest v1.0.13). Licence: MIT OR Apache-2.0.

## Build / run / test

- Rust **1.90.0** is pinned in `mise.toml`. With mise active in the repo,
  plain `cargo` works. Otherwise:
  `C=$(mise where rust@1.90.0); env -u CARGO_HOME -u RUSTUP_HOME PATH="$C:$PATH" CARGO_TARGET_DIR=target cargo <cmd>`
  (keep `$PATH` so `gh` stays reachable).
- `default-members = ["."]`: a bare `cargo build`/`cargo test` builds only
  the core library and CLI tools. The GUI needs `-p sector64-gui`.
- Before every PR, run:
  - `cargo clippy --workspace --all-targets`: must be clean.
  - `cargo test --workspace`
  - rustfmt **on the files you touched**. `main` is not yet fully
    rustfmt-clean (e.g. `examples/`; the GUI crate is), so
    `cargo fmt --all --check` fails on lines you didn't write. Don't
    reformat unrelated code in a feature PR.
- **GUI dependencies are pinned for the 1.90 MSRV:** eframe 0.29 (default
  features off) and rfd 0.15. Don't bump them without checking
  `rust-version`. The eframe features are deliberate:
  - `wayland`/`x11`: without `wayland`, egui-winit ignores `with_app_id`
    and the window class is empty.
  - `wgpu`, Windows only (`[target.'cfg(windows)']`): the GUI renders with
    wgpu there (`Renderer::Wgpu` in `main()`), because glow aborts on
    machines without an RGBA8 GL pixel format (seen under Wine). Linux
    keeps glow.
- **Drag-and-drop on Linux:** winit 0.30 has no drag-and-drop on Wayland, so
  the GUI only receives drops on X11. `SECTOR64_X11=1` makes `main()` start
  the window on X11 (XWayland under Wayland) via `with_x11()`; that's why
  `gui/Cargo.toml` names `winit` directly (Linux only, the same 0.30 eframe
  uses). Drops add files when an editable disk/new archive is open, and open
  the first file otherwise.
- The GUI pins its C64 style to **both** egui themes and forces dark mode
  (`apply_c64_theme`). Otherwise a light-themed OS reverts it to egui's
  stock light style.
- GUI layout: `gui/src/main.rs` holds the state types (`App`, `Pane`,
  `Act`…) and `main()`; `actions.rs` is `App::apply` and the operations,
  `frame.rs` is `App::frame_ui` (one frame), and `listing`, `preview`,
  `tools`, `basic_ui`, `dialogs`, `files`, `vice`, `widgets` hold the rest.
  Modules `use crate::*`; shared items are `pub(crate)`. Tests are in
  `tests.rs`. Esc goes through one chain in `frame.rs` (front window
  first); any listing change goes through `Pane::refresh`, which drops
  the previews, disk map, marks and rename.
- The user manual is `gui/MANUAL.md`, embedded with `include_str!` and shown
  by the Help menu. Update it with GUI changes.
- **CLI output:** use `outln!` (from `src/bin/common/mod.rs`, pulled in with
  `#[macro_use] mod common;`), never `println!`. Use
  `common::write_stdout` for raw bytes. They exit quietly when stdout's
  reader goes away (`| head`); `println!` panics instead.
- If the repo or `target/` has been restored from a backup, binaries and
  build scripts can lose their execute bit. That gives `Permission denied
  (os error 13)`, and cargo doesn't rebuild because it thinks they're fresh.
  `chmod u+x` the ELF files under `target/` (check full dates with
  `ls -l`; `--time-style=+%T` hides them).

## Tests that need outside inputs (they skip, not fail, when missing)

- `tests/c1541_*.rs` cross-check against VICE's **`c1541`**, an independent
  implementation, both reading and writing. They need `c1541` on `PATH`; it
  is installed (`/usr/bin/c1541`).
- `*_samples.rs` (and a GUI Zip-Code test) read real-world files from
  `$SECTOR64_SAMPLES`, defaulting to `~/Downloads/Sector64 Test Files`.
- `tests/petcat_basic.rs` checks the BASIC detokenizer (`src/basic.rs`)
  against programs tokenized by VICE's **`petcat`** (installed with VICE):
  `-w2` BASIC V2, `-w70` C128 BASIC 7.0, `-wsimon` Simons' BASIC, every
  keyword of each. Note: in petcat source π is `~`; `{pi}` is literal text.
  The same file checks the tokenizer (`src/tokenize.rs`) byte for byte.
  petcat gets `?` and abbreviations wrong, so those were checked against the
  C64 and C128 ROMs in VICE (call CRUNCH through `$0304`).
  Simons' slots `$6422 $642A $642B $642D $642F $643C-$643F $6454 $645E` are
  unassigned even in petcat; they list as `{$64xx}`.
- `arc_samples.rs`'s squeeze plaintext check needs `ARC_PLAINTEXT` pointing at
  the plaintext directory. **Not** the samples dir; that trips the
  `checked > 0` assert.
- `tests/fuzz_robustness.rs` is the always-on, no-panic fuzzer. `fuzz/` holds
  the optional cargo-fuzz targets (see `fuzz/README.md`).

## Windows build and testing

- `sh packaging/build-windows.sh` cross-compiles the GUI and all CLI tools
  for `x86_64-pc-windows-gnu` and writes
  `dist/sector64-<ver>-x86_64-windows.zip`. It needs the rustup target and
  MinGW-w64 (`mingw-w64-gcc`, installed). It links the C runtime statically
  and strips symbols.
- `gui/build.rs` embeds `gui/assets/sector64.ico` (generated from
  `packaging/sector64.svg`) into the `.exe` via windres or rc.exe. It does
  nothing on other targets.
- Wine is installed. Test in a **throwaway prefix**, never `~/.wine`:
  `WINEPREFIX=<scratch>/prefix WINEDLLOVERRIDES="mscoree,mshtml=" WINEDEBUG=-all wine …`.
  Compare each Windows tool's stdout, exit code and written files against
  the Linux build on the samples. The only expected difference is the `\` vs
  `/` path separator in `s64wr3 extract` messages.

## macOS build

- `packaging/build-macos.sh` runs on GitHub's
  `macos-15` runner and writes `dist/sector64-<ver>-universal-macos.zip`
  (`Sector64.app`, `bin/`, `doc/`, `README-MACOS.txt`): lipo'd arm64 + x86_64,
  macOS 11+, ad-hoc signed only (no Developer ID, no notarization).
- `packaging/check-macos.sh` is the automatic test: CI runs it on Apple
  Silicon and Intel runners. It checks the package, runs the CLI tools, starts
  the app, takes a screenshot (kept as a workflow artifact; look at it) and
  quits it. The Intel version (v1.0.11) has been tried by hand on macOS 14
  (Sonoma), Run in VICE included; Apple Silicon has not. Say so, and that it's
  experimental and unsigned, wherever the Mac version is announced.
- `gui/src/mac.rs` holds what winit leaves out: ⌘Q asking about unsaved
  changes, VICE's folders, and not working in `/` when started from Finder.
  Shortcut labels go through `key_name`, Open dialogs through `open_dialog`
  (macOS merges all filters and ignores `*`).
- `gui/assets/sector64.icns` is made by `packaging/make-icns.py`.

## Releases

- `packaging/build-linux.sh` builds `dist/sector64-<ver>-x86_64-linux.tar.gz`
  (`bin/`, `share/` launcher + icon, `doc/`, `INSTALL.txt`).
- `.github/workflows/release.yml` builds all three packages. On a pushed `v*` tag
  it checks that the tag matches the crate version, builds Linux on Ubuntu
  22.04 (glibc 2.35), Windows with MinGW and macOS, and attaches them plus
  `SHA256SUMS` to the release (creating it with generated notes). PRs that
  touch packaging or build files run the build without publishing.
- To cut a release:
  1. In a PR, bump `version` in **both** `Cargo.toml` and `gui/Cargo.toml`.
  2. After merging, tag the merge commit `vX.Y.Z` and push the tag.
  3. Once the workflow finishes, replace the generated notes with
     hand-written ones (`gh release edit --notes-file`).
  4. Download the assets and check them against `SHA256SUMS`.
  5. Publish to itch.io: `sh packaging/itch-publish.sh` (page
     `gsargent/sector64`, channels `linux`/`windows`/`mac`). It downloads the
     release, checks `SHA256SUMS`, adds an `.itch.toml` so the itch app
     launches the GUI, and pushes with butler (`--if-changed`). It needs
     butler on `PATH` or in `$BUTLER`, logged in (`butler login`, run by the
     user). Try `--dry-run` first.

## Rules

- **Core crate stays dependency-free** with `#![forbid(unsafe_code)]`; only
  `gui/` may have dependencies.
- **Clean room:** decoders are written from format knowledge and observed
  behaviour, never from copied source. Don't commit copyrighted sample
  archives or disk images; tests read them from `$SECTOR64_SAMPLES`. The only
  DirMaster mention is the README provenance line, kept deliberately.
- Fonts and ROMs (e.g. C64 Pro Mono for the GUI) are loaded at runtime,
  never bundled. On Windows the font is also found next to `Sector64.exe`.
- Reverse-engineering work: use the `retro-binary-cleanroom` skill. For 6502
  code in encrypted overlays, emulate it to decode rather than reading it
  statically.
- Checking that generated images work on a "real" C64: use the
  `c64-emulator-testing` skill (VICE + `c1541`, scripted, with no desktop
  keystrokes).
- GUI testing: use the `desktop-gui-testing` skill. No synthetic keystrokes;
  screenshots only when the window is in front. The GUI's window class is
  `sector64-gui` (`sector64.exe` under Wine). The user's terminal title can
  contain "Sector64", so match on class, not just title.

## Packaging

`packaging/install.sh` is a no-root per-user install into `~/.local`. It
builds from source and installs the GUI, all CLI tools, the icon, the
`.desktop` file and docs. `packaging/uninstall.sh` reverses it. Both should
be executable (mode 755). `sector64.desktop` has
`StartupWMClass=sector64-gui` so the window matches its launcher entry.

## Workflow

- **Feature list:** `docs/FEATURES.md` is the single list of what's built
  and what's next (the `feature-list` skill describes the format). Add new
  ideas and follow-ups to it as they come up, and update a feature's row in
  the same PR that builds it; on release, turn `Done (unreleased, #PR)`
  into `Done vX.Y.Z`.
- One branch and one PR per change. The user approves with "merge", then
  squash-merge and delete the branch.
- Commit messages end with the Co-Authored-By line from the system prompt.
  PR bodies end with the Claude Code line.
- `Cargo.lock` is gitignored in this repo; leave it untracked.
