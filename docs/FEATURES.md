# Sector64 feature list

Sector64 is a clean-room, dependency-free Rust toolkit for Commodore disk
images and archives, with a C64-styled GUI and twelve command-line tools. This
is the project's single list of features: what's built, what's planned, and
ideas for later.

## How to use this list

- Pick features by ID (e.g. `EDIT-08`). Each ID is one implementable,
  testable unit, and IDs never change.
- New ideas go in the **Ideas inbox** as soon as they come up, then move
  into a section once discussed.
- A feature's row is updated in the same change that builds it.

**Status:** `Idea` (captured) · `Planned` (wanted) · `Doing` (on a branch,
PR in Notes) · `Done vX.Y.Z` (shipped; `Done (unreleased, #PR)` until the
release) · `Dropped` (reason in Notes)

**Size:** `S` an afternoon · `M` a day or two · `L` several PRs

**Where:** `core` = the library (`src/`), `GUI`, `CLI`. Most features touch
the core plus one front end.

---

## Ideas inbox

| ID | Idea | Notes |
|---|---|---|
| EDIT-09 | BASIC editor: renumber, find/replace, and keep machine code after a listed program | M; follow-up to EDIT-06 |
| CLI-05 | Repair from the command line (`s64val --repair`) | S; core done in DISK-04 |
| FMT-14 | CMD-native subdirectories: list and map the files inside them | M; today only the top directory is read, so repair never frees blocks on such disks |
| DISK-07 | Repair broken REL and GEOS files (rebuild side sectors, VLIR index) | L; DISK-04 can only remove them |
| APP-10 | Open disk images from Finder (double-click, "Open with", drop on the Dock icon) | M; macOS sends these as Apple events winit 0.30 ignores; follow-up to APP-09 |
| APP-11 | Sign and notarize the Mac app | S once enrolled; needs an Apple Developer account ($99/yr) |
| APP-12 | Test the Mac GUI by remote control in CI (or a Mac over SSH): file dialogs, drag-and-drop, shortcuts, Run in VICE | M; less urgent now v1.0.11 was tried by hand; follow-up to APP-09; needs the runners to allow scripted clicks and keys, not tried yet |
| DISK-08 | Sector editor: type text into the PETSCII column, and search the disk for bytes or text | M; follow-up to DISK-05 (today it edits in hex only) |
| CLI-06 | Search from the command line (`s64find`): text or hex inside the files of disks and archives | S; core done in VIEW-10 |
| APP-13 | Bug: turning the preview pane on when the window is too narrow for it and the list leaves part of the window unpainted (other windows show through) | S; reported by the user, seen while testing VIEW-10 |

---

## 1. Formats (FMT)

Reading and writing disk images and archives (core, used by both front
ends).

| ID | Feature | Size | Status | Notes |
|---|---|---|---|---|
| FMT-01 | Read D64, D71, D81 disk images | M | Done v1.0.0 | |
| FMT-02 | Read G64 (GCR bitstream, decoded to D64) | M | Done v1.0.0 | read-only |
| FMT-03 | Read CMD D2M: native (DNP) and 1581 partitions | M | Done v1.0.0 | |
| FMT-04 | Read T64, LNX, LBR and P00 | M | Done v1.0.0 | |
| FMT-05 | Read and write Zip-Code sets | M | Done v1.0.0 | |
| FMT-06 | ARC/SDA: store, pack, squeeze, crunch, squash | L | Done v1.0.0 | methods reversed from 6502 code |
| FMT-07 | LHA/SFX `-lh1-` | M | Done v1.0.0 | |
| FMT-08 | Wraptor 3 (WR3) | M | Done v1.0.0 | |
| FMT-09 | GEOS ConVerT (CVT), VLIR records split out | M | Done v1.0.0 | |
| FMT-10 | TAP datasette images | M | Done v1.0.0 | |
| FMT-11 | Write T64, LNX, LBR and P00 archives | M | Done v1.0.0 | |
| FMT-12 | Write to D64/D71/D81 and CMD-native D2M | L | Done v1.0.0 | DNP BAM bit-order fix in v1.0.6 |
| FMT-13 | Format conversion in one step (e.g. D64 → D81, archive or T64 → disk) | M | Idea | |

## 2. Browse and preview (VIEW)

Seeing inside files without extracting them.

| ID | Feature | Size | Status | Notes |
|---|---|---|---|---|
| VIEW-01 | Directory listing with type badges, sort and filter | M | Done v1.0.0 | |
| VIEW-02 | HEX and PETSCII TEXT preview | S | Done v1.0.0 | |
| VIEW-03 | BASIC listing preview, C64 BASIC V2 | M | Done v1.0.5 | |
| VIEW-04 | BASIC 7.0 (C128) and Simons' BASIC dialects | M | Done v1.0.6 | checked against petcat |
| VIEW-05 | GFX: sprites and characters, hires or multicolour | M | Done v1.0.7 | |
| VIEW-06 | DIS: 6502 disassembly, starting at the SYS target | M | Done v1.0.8 | all 256 opcodes checked against VICE |
| VIEW-07 | PIC: Koala, Art Studio, Advanced Art Studio, Doodle! | M | Done v1.0.8 | pixel-checked against VICE |
| VIEW-08 | geoPaint pictures in PIC | L | Idea | needs a GEOS geoPaint decoder first |
| VIEW-09 | SID music info: title, author, release, load/init/play addresses, songs | S | Idea | |
| VIEW-10 | Search by name across a listing, or inside files for text or hex bytes | M | Done (unreleased, #PR) | core `sector64::search`; GUI Find window (Ctrl+F) shows matches in the hex preview or the Sector Editor |

## 3. Editing (EDIT)

Changing disks and building archives.

| ID | Feature | Size | Status | Notes |
|---|---|---|---|---|
| EDIT-01 | Create blank D64/D71/D81; add, rename, delete; save | M | Done v1.0.0 | |
| EDIT-02 | Build a new archive and save it as T64/LNX/LBR/P00 | M | Done v1.0.0 | |
| EDIT-03 | Undo and redo | M | Done v1.0.5 | |
| EDIT-04 | Copy files between two disks (Copy window) | M | Done v1.0.7 | exact names, types, lock flags |
| EDIT-05 | Copy across panes with F5 | S | Done v1.0.9 | |
| EDIT-06 | BASIC text → runnable PRG (V2, 7.0, Simons'), the reverse of VIEW-03/04 | L | Done v1.0.10 | core `sector64::tokenize`, `s64basic`, GUI BASIC editor; checked against petcat and the C64/C128 ROMs |
| EDIT-07 | Directory editing: reorder, separator/"art" entries, disk name and ID, lock/unlock | M | Idea | |
| EDIT-08 | Proper REL and GEOS copy between disks (side sectors, VLIR records) | M | Idea | both are skipped today |

## 4. Disk tools (DISK)

Checking and inspecting a disk's structure.

| ID | Feature | Size | Status | Notes |
|---|---|---|---|---|
| DISK-01 | Validate: every file chain against the BAM | M | Done v1.0.0 | |
| DISK-02 | Disk map: every block, coloured by owner | M | Done v1.0.6 | |
| DISK-03 | Compare Panes: same / differs (first byte) / only on one side | M | Done v1.0.9 | core `sector64::compare` |
| DISK-04 | Validate **and repair**: rebuild the BAM, free orphaned blocks, drop broken entries, with undo | L | Done v1.0.10 | core `sector64::repair`; also ends broken chains and splits cross-linked files; BAM checked against `c1541 -validate` |
| DISK-05 | Sector editor: view/edit any track and sector in hex, jump from the disk map, follow chains | L | Done (unreleased, #50) | GUI; Write is one undo step; read-only disks view only |
| DISK-06 | Compare window: "Copy missing files" button that copies everything that's only on one side across | S | Done v1.0.10 | either direction; follow-up to DISK-03 |

## 5. Working with other tools (TOOL)

| ID | Feature | Size | Status | Notes |
|---|---|---|---|---|
| TOOL-01 | Run in VICE: start the disk or selected program (x64sc / x128) | M | Done v1.0.8 | runs a temp copy |
| TOOL-02 | Drag files out of the list to the file manager | M | Idea | winit drag-out support needed |

## 6. Command-line tools (CLI)

| ID | Feature | Size | Status | Notes |
|---|---|---|---|---|
| CLI-01 | Ten tools: s64ls, s64extract, s64write, s64val, s64pack, s64arc, s64lha, s64wr3, s64cvt, s64tap | L | Done v1.0.0 | see CLI.md |
| CLI-02 | `s64extract --list` (BASIC listing) and `s64val --map` (block map) | S | Done v1.0.7 | |
| CLI-03 | Compare two disks or archives from the command line | S | Done v1.0.10 | `s64cmp`; core `sector64::listing` reads any disk or archive |
| CLI-04 | `--json` output for scripts | M | Idea | |

## 7. GUI and app (APP)

| ID | Feature | Size | Status | Notes |
|---|---|---|---|---|
| APP-01 | C64-styled egui GUI with PETSCII font, manual in Help | M | Done v1.0.0 | |
| APP-02 | Linux and Windows builds and the release workflow | M | Done v1.0.1 | itch.io publishing added in v1.0.5 |
| APP-03 | Drop a file on the window to open or add it | S | Done v1.0.4 | needs X11 on Linux |
| APP-04 | Open Recent and a recent-files start screen | S | Done v1.0.5 | |
| APP-05 | File / Disk / Preview / Help menus with shortcut hints; disk logo | S | Done v1.0.8 | |
| APP-06 | Dual-pane view: two disks side by side, Tab to switch | M | Done v1.0.9 | |
| APP-07 | Background loading with a progress bar | M | Idea | |
| APP-08 | Grow the window to fit the preview or long names | S | Idea | |
| APP-09 | macOS build: universal, unsigned Sector64.app and CLI tools in a zip | M | Done v1.0.11 | CI builds, tests and starts it; v1.0.11 (Intel) tried by hand on macOS 14, incl. Run in VICE: all worked; Apple Silicon not tried by hand |

---

## Suggested order (a design choice; the user decides)

1. **Next:** APP-13 (preview pane bug), EDIT-07, FMT-13.

## Done by release

Releases v1.0.0–v1.0.4 established the formats, GUI, CLI and packaging;
the rows above give each feature's first release.

- **v1.0.5:** VIEW-03, EDIT-03, APP-04
- **v1.0.6:** VIEW-04, DISK-02
- **v1.0.7:** VIEW-05, EDIT-04, CLI-02
- **v1.0.8:** VIEW-06, VIEW-07, TOOL-01, APP-05
- **v1.0.9:** EDIT-05, DISK-03, APP-06
- **v1.0.10:** DISK-04, DISK-06, EDIT-06, CLI-03
- **v1.0.11:** APP-09
