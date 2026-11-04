# d64

A small, clean-room library for Commodore 1541 **D64** disk images, in Rust —
reading, validation, and now writing.

Written from the publicly documented 1541 disk format. It is **not** derived
from DirMaster or any other tool's code, and it contains none of their fonts,
icons, or branding. It is intended as the format-engine core you can build a
native Linux GUI (wxWidgets/Qt/egui) around.

## Why it exists / what's different

A static audit of a Windows D64 utility found its track/sector→offset math had
no bounds clamp and its directory walk had no cycle detection, so a crafted
image could drive reads out of range or loop forever. This crate does the same
job with those failure modes designed out:

- **Total, checked addressing** (`geometry::ts_to_offset`): every track/sector
  is validated against the real speed-zone geometry before it becomes an offset.
- **Start-and-length block checks** (`Disk::block`): a read is rejected unless
  the whole 256-byte block fits the data area, not just its first byte.
- **Hardened directory walk** (`Disk::directory`): the chain is followed with a
  visited-set (rejects cycles) and a hard cap (rejects over-long chains), and
  every link is bounds-checked.
- `#![forbid(unsafe_code)]`, overflow checks on in release, zero dependencies.

## Supported

- 35- and 40-track images, with or without the trailing per-block error map
  (174848 / 175531 / 196608 / 197376 bytes).
- Disk header (name, id, DOS type), blocks-free (standard 35-track BAM),
  directory walk with file type, flags (splat/locked), first block, size.
- **File extraction** (`Disk::read_file` / `read_entry`, `find_entry`): walks a
  file's track/sector data chain, handling the final block's byte count, with
  the same cycle and bounds guards as the directory walk.
- **Block chain** (`Disk::file_block_chain`): the ordered blocks a file
  occupies — the basis of extraction and validation.
- **BAM** (`Disk::bam` → `Bam`): per-track free counts and the allocation
  bitmap, with a bounds-checked `is_allocated(track, sector)`.
- **40-track BAM** (`DosFormat`): the tracks 36–40 extension for DolphinDOS
  (`$AC`) and SpeedDOS (`$C0`). The format is detected at load by a
  self-consistency heuristic (`Disk::dos`) and can be forced with
  `Disk::set_dos`. **Caveat:** the extended offsets are from the documented
  formats and are *not* verified against hardware-written images; when
  detection declines to guess, the extended tracks are simply not counted.
- **Validation** (`Disk::validate` → `ValidationReport`): cross-checks every
  file's chain against the BAM, reporting cross-linked blocks and blocks used
  but marked free.
- **Relative files** (`Disk::side_sector_chain`, `rel_data_blocks`,
  `read_rel` → `RelFile`): follows a REL file's side-sector chain, gathers the
  data-block pointers it indexes, and splits the data into fixed-length
  records — same cycle/bounds guards as the other walks.

- **Writing** (`Disk::format`, `write_file`, `scratch`, `rename`, `as_bytes`):
  format a blank disk, allocate blocks through the BAM, write a file's data
  chain and directory entry (extending onto a second directory sector when
  needed), delete a file (freeing its blocks and side sectors and reusing the
  slot), and rename in place. What it writes and edits is read back
  byte-for-byte by `c1541` (see Testing).

Not yet: 1581/1571 formats, archive/graphics formats. Those are the next layers.

## Build and run

```sh
cargo test                           # unit tests, incl. malformed-image rejections
cargo run --bin d64ls -- disk.d64    # directory listing
cargo run --bin d64extract -- disk.d64 GAME > game.prg   # extract one file
cargo run --bin d64val -- disk.d64                       # validate vs. the BAM
```

Example listing:

```
0 "MY DISK         " ID 2A
10   "GAME"             prg
664 blocks free.
```

## Testing

Three tiers, the latter two validating against implementations this crate does
not share code with:

1. **Unit tests** (`src/*.rs`): synthetic images and the malformed-input
   rejections. Always run.
2. **`c1541` read differential** (`tests/c1541_diff.rs`): builds a disk with
   VICE's `c1541`, then checks directory, blocks-free, and per-file extraction
   byte-for-byte against `c1541`'s own output.
   **`c1541` write round-trip** (`tests/c1541_write.rs`): formats and writes
   files with this crate, then confirms `c1541` reads them all back
   byte-for-byte and agrees on the file count and free blocks. Both run when
   `c1541` is on `PATH`, skip cleanly otherwise.
3. **Emulator-minted REL** (`tests/rel_fixture.rs`): reads `fixtures/rel.d64`, a
   relative file created by the emulated 1541 DOS (VICE `x64sc` running
   `fixtures/makerel.bas`), and checks the side-sector path returns the exact
   records written. `c1541` cannot read REL files, so this is the only
   independent check of that path. The fixture is committed; regenerate it by
   autostarting `makerel.bas` from a formatted disk under `x64sc`.

Still synthetic-only: the **40-track DolphinDOS/SpeedDOS** BAM (neither `c1541`
nor a stock emulated 1541 produces one), so those offsets remain unverified
against a real disk.

## Layout

| file | contents |
|------|----------|
| `src/geometry.rs`  | speed zones, bounds-checked track/sector → block/offset |
| `src/disk.rs`      | image loading, block accessor, header, BAM, directory walk |
| `src/directory.rs` | 32-byte slot layout and entry parsing |
| `src/bam.rs`       | Block Availability Map: free counts + allocation bitmap |
| `src/petscii.rs`   | minimal PETSCII→UTF-8 for display |
| `src/error.rs`     | error type (every malformed input is a recoverable error) |
| `src/bin/d64ls.rs` | CLI directory listing |
| `src/bin/d64extract.rs` | CLI: extract one file to stdout |
| `src/bin/d64val.rs` | CLI: validate the disk against its BAM |

## License

MIT OR Apache-2.0.
