# Sector64 — notes for Claude

Clean-room, **zero-dependency** Rust toolkit for Commodore disk images and
archives:
- core library (`src/`)
- 10 CLI tools (`src/bin/`, documented in `CLI.md`)
- an egui GUI in `gui/` (separate workspace member)

Released as v1.0.0 on GitHub (`gsargent70/Sector64`). Licence: MIT OR Apache-2.0.

## Build / run / test

- Rust **1.90.0** is pinned in `mise.toml`. With mise active in the repo,
  plain `cargo` works. Otherwise:
  `C=$(mise where rust@1.90.0); env -u CARGO_HOME -u RUSTUP_HOME PATH="$C:$PATH" CARGO_TARGET_DIR=target cargo <cmd>`
  (keep `$PATH` so `gh` stays reachable).
- `default-members = ["."]`: a bare `cargo build`/`cargo test` builds only
  the core library and CLI tools. The GUI needs `-p sector64-gui`.
- Before every PR, run all of:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets`
  - `cargo test --workspace`
- **GUI dependencies are pinned for the 1.90 MSRV:** eframe 0.29 (default
  features off), rfd 0.15. Don't bump them without checking `rust-version`.
- The user manual is `gui/MANUAL.md`, embedded with `include_str!` and shown
  by the Help menu. Update it with GUI changes.

## Tests that need outside inputs (they skip, not fail, when missing)

- `tests/c1541_*.rs` cross-check against VICE's **`c1541`**, an independent
  implementation, both reading and writing. They need `c1541` on `PATH`; it is
  installed (`/usr/bin/c1541`).
- `*_samples.rs` read real-world files from `$SECTOR64_SAMPLES` (formerly
  `~/Downloads/Sector64 Test Files`, which was missing on 2026-11-10; ask
  the user where the samples are now).
- `arc_samples.rs`'s squeeze plaintext check needs `ARC_PLAINTEXT` pointing at
  the plaintext directory. **Not** the samples dir; that trips the
  `checked > 0` assert.
- `tests/fuzz_robustness.rs` is the always-on, no-panic fuzzer. `fuzz/` holds
  the optional cargo-fuzz targets (see `fuzz/README.md`).

## Rules

- **Core crate stays dependency-free** with `#![forbid(unsafe_code)]`; only
  `gui/` may have dependencies.
- **Clean room:** decoders are written from format knowledge and observed
  behaviour, never from copied source. Don't commit copyrighted sample
  archives or disk images; tests read them from `$SECTOR64_SAMPLES`. The only
  DirMaster mention is the README provenance line, kept deliberately.
- Fonts and ROMs (e.g. C64 Pro Mono for the GUI) are loaded at runtime,
  never bundled.
- Reverse-engineering work: use the `retro-binary-cleanroom` skill. For 6502
  code in encrypted overlays, emulate it to decode rather than reading it
  statically.
- Checking that generated images work on a "real" C64: use the
  `c64-emulator-testing` skill (VICE + `c1541`, scripted, with no desktop
  keystrokes).
- GUI testing: use the `desktop-gui-testing` skill. No synthetic keystrokes;
  screenshots only when the window is in front.

## Packaging

`packaging/install.sh` is a no-root per-user install into `~/.local`. It
builds from source and installs the GUI, all CLI tools, the icon, the
`.desktop` file and docs. `packaging/uninstall.sh` reverses it. Both should
be executable (mode 755).

## Workflow

- One branch and one PR per change. The user approves with "merge", then
  squash-merge and delete the branch.
- Commit messages end with the Co-Authored-By line from the system prompt.
  PR bodies end with the Claude Code line.
- `Cargo.lock` is gitignored in this repo; leave it untracked.
