# Sector64

A small, clean-room Rust library (crate `sector64`) for Commodore **D64**
(1541), **D71** (1571), and **D81** (1581) disk images — reading, writing, and
validation.

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

- **1581 / D81** (`Disk::format_d81`, and `load` auto-detects by size): the
  80-track geometry, two-sector BAM on track 40, and 1581 header. Reading,
  writing, scratch/rename and validation all work the same as for the 1541;
  what it reads and writes is confirmed byte-for-byte against `c1541` in both
  directions (see Testing).

- **1571 / D71** (`Disk::format_d71`, `load` auto-detects by size): the
  70-track double-sided geometry (side 1 mirrors side 0's speed zones) with the
  split BAM — free counts at `$DD` in 18/0, bitmaps in 53/0. Reading, writing
  (including files that span both sides), scratch/rename and validation all
  work, confirmed byte-for-byte against `c1541` in both directions.

- **Archive import** (`parse_t64`, `parse_lnx`, `parse_lbr` → `ImportFile`,
  `s64write import`): reads **T64** tape archives, **LNX** (Lynx) archives, and
  **LBR** libraries and reconstructs each file. T64/LNX are confirmed
  byte-for-byte against `c1541`'s independent readers (`tape` and `unlynx`), and
  `parse_lnx` was validated against a real-world `.lnx`. **LBR** (the C64
  library format — a CR-delimited `DWB` text directory plus concatenated,
  uncompressed member data; not the CP/M `.lbr`) was validated against a real
  7-member library: its directory parses exactly, and every member round-trips
  byte-for-byte when written to a disk and read back by `c1541`.

- **Zip-Code** (`parse_zipcode` / `write_zipcode`, `s64write unzip`): decodes
  and encodes the four-file `1!`…`4!` disk-compression format (raw / fill / RLE
  per sector). A D64 round-trips through it, and `c1541 unzip` reproduces the
  source from our encoding — so our bytes are real Zip-Code.

- **G64** (`g64::to_d64`, and `Disk::load` auto-detects the `GCR-1541`
  signature): the GCR *bitstream*-level 1541 format. Where a D64 stores decoded
  sectors, a G64 stores each track's raw recorded bitstream (sync marks and
  all), which is how it represents copy protection. The reader walks the track
  table, scans each track's bitstream for syncs, GCR-decodes every sector (5
  recorded bits → 4 data bits) and verifies the header and data checksums,
  assembling a standard 35-track D64 that the ordinary machinery then reads.
  Undecodable/protected sectors are left zero rather than erroring. Confirmed
  byte-for-byte against `c1541` reading the G64 directly (every file on a real
  Epyx disk). `s64ls`/`s64extract`/`s64val` therefore work on a `.g64` directly.

- **ARC archive** (`parse_arc` → `ArcEntry`, `s64arc`): walks a Commodore
  **ARC** archive (ARC230 and compatibles — the C64 format, unrelated to MS-DOS
  ARC) into its entries, validating every header field and the 254-byte block
  arithmetic, rejecting cycles/overflow, and stopping at the trailing padding.
  Each entry exposes its version, compression method, 16-bit checksum, original
  size, file type and name. **Listing works today** and is validated against
  real archives' known contents; **extraction is implemented only for stored
  (method 0)** entries. The compressed methods (RLE, Huffman, LZ) return
  **crunch** (methods 3 and 5 — the most common) is fully decoded: the C64 ARC
  LZW variant (big-endian, bit-reversed within bytes, 9–12-bit with early-change
  growth and `first_free = 258`) over a run-length stream (escape `0xFE`),
  verified byte-for-byte against a real archive's original files. Huffman
  (squeeze/squash, methods 2 and 4) still returns `Error::UnsupportedArcMethod`.

- **CMD D2M** (`Disk::load` auto-detects by size): the CMD FD-2000 native image.
  A D2M holds one partition — either **CMD-native (DNP)** geometry (256 sectors
  per track, `block = (track−1)×256 + sector`, header at 1/1, directory at 1/34)
  or a **1581** partition — and `load` probes the two header positions to pick
  the right one, then reads it with the ordinary directory/extraction machinery
  (a new `Geometry::CmdNative`). Confirmed against `c1541` on two real disks
  (a DNP and a 1581-partition D2M): every regular file extracts byte-for-byte.
  The CMD-native block-availability map isn't decoded, so `blocks_free` /
  `validate` report `UnsupportedGeometry` for a DNP; and GEOS/VLIR members
  (which aren't flat files) are not flat-extracted.

- **TAP** (`parse_tap` → `TapImage`, `s64tap`): the raw datasette pulse stream.
  A TAP records the length of every tape pulse in clock cycles, nothing more —
  the *bit* encoding is defined by whichever loader wrote the tape, and most
  commercial tapes use custom turbo loaders, so there is no general way to
  recover files (emulators replay the pulses rather than decode them). This
  parses the header (version 0/1/2, platform, video) and decodes the pulse
  stream, handling the non-zero `×8` form and the version-1/2 24-bit overflow
  escape. Validated against a real tape (Azimuth 3000): the decode tiles the
  declared data exactly (588144 pulses, ~236 s). File extraction is out of
  scope by nature of the format.

- **GEOS CVT** (`parse_cvt` → `CvtInfo`, `s64cvt`): the ConVerT container that
  linearises a GEOS file (its INFO block plus a sequential or **VLIR** payload)
  for transport. `parse_cvt` reads the 254-byte signature block (GEOS name, CBM
  and GEOS file types, structure, `"… formatted GEOS file"` signature), the
  INFO block, and — for VLIR — the record index (sector-count / last-size
  pairs). It **describes** a CVT; it does not split the concatenated record data
  into individual records, because the per-record padding varies between Convert
  versions and no local GEOS tool exists to validate a split (the reference
  `geoinfo` stops here too). Validated against real samples (DESKTOPV1.32.CVT,
  GEOSSYM.CVT): the header parses exactly and the 254-byte blocks tile the file.

- **PC64 single-file** (`parse_p00`, `s64write import`): the `.P00`/`.S00`/
  `.U00`/`.R00`/`.D00` container — a 26-byte `C64File` header (16-byte CBM name,
  reserved and REL-record bytes) in front of the raw file. The CBM type comes
  from the extension letter, which `s64write import` applies. Validated against
  a real `.p00`: the name and payload parse exactly, and the payload round-trips
  byte-for-byte when written to a disk and read back by `c1541`.

- **Self-extracting archives** (`parse_sda`, `parse_lha` → `LhaEntry`, `s64arc`,
  `s64lha`): two self-extractor families that bundle an archive behind a C64
  loader. A **`.SDA`** (self-dissolving) is an ARC archive behind a BASIC
  dissolver — `parse_sda` finds the embedded archive at `(line − 6) × 254` and
  reads it with [`parse_arc`]. A **`.SFX`** is an **LHA** archive behind a
  loader — `parse_lha` locates the first level-0 header past the stub by its
  checksum, walks the entries (name, CBM type, method, sizes, CRC) and ends at
  the terminator or trailing padding. Both **list** real samples correctly
  (SURVIVOR.SDA, and RAS-1.SFX's 20 members / SWB.SFX). **LHA `-lh0-` (stored)
  and `-lh1-` (LZHUF) members fully extract** — the lh1 decoder (`src/lzhuf.rs`)
  is a clean-room port of the public `lzhuf.c`, and every member of both real
  `.sfx` archives decodes to the exact length *and* matching CRC-16 (the
  archive's own integrity check). **`.sda` crunch members also fully extract**
  (via the ARC crunch decoder). ARC Huffman (squeeze/squash) and other LHA
  methods still return their `Unsupported…Method` error.

- **Wraptor 3** (`parse_wr3` → `Wr3Entry`, `s64wr3`): the `.wr3` archive. Each
  entry is introduced by the marker `FF "BL" FF`, its NUL-terminated name, and
  its compressed data; `parse_wr3` recovers the entries by scanning for markers
  and validating the name after each (the marker does not appear inside the
  compressed streams). Lists real archives correctly (Loadstar #128, 43
  members). The Wraptor compression is a custom scheme and is not decoded, so
  the payload is exposed as-is — the decoder is the next layer.

Not yet: ARC Huffman (squeeze/squash) and Wraptor decompression, and
GEOS/graphics formats. Those are the next layers.

## GUI

A native front-end (`gui/`, a separate workspace member so the engine stays
dependency-free) built on [`egui`](https://github.com/emilk/egui). It opens a
disk image or any supported archive, lists the contents, and extracts files —
the core of what DirMaster does.

```sh
cargo run -p sector64-gui                 # open the window
cargo run -p sector64-gui -- disk.d64     # open a file on launch
```

Open a `.d64/.d71/.d81/.g64/.d2m` disk or an `.arc/.sda/.lha/.lzh/.sfx/.lbr/
.t64/.lnx/.p00/.wr3` archive, click a file, and **Extract selected** (or
**Extract all**) to the chosen output directory. The side pane shows a hex +
ASCII **preview** of the selected file; for an open D64/D71/D81, **Validate
disk** cross-checks every file chain against the BAM. Members that need a
not-yet-implemented decoder are listed with the reason instead of bytes.

For the writable sector formats (`.d64/.d71/.d81`) it is also an **editor**:
create a blank image, add a host file (button or drag-and-drop) as prg/seq/usr,
rename, delete, and Save / Save As. G64/D2M and archives are read-only.
Double-click a file to extract it; the Open dialog filters to known types.

Install it as a desktop app for the current user:

```sh
packaging/install.sh    # builds release, installs the binary + a .desktop launcher
```

## Build and run

```sh
cargo test                           # unit tests, incl. malformed-image rejections
cargo run --bin s64ls -- disk.d64    # directory listing
cargo run --bin s64extract -- disk.d64 GAME > game.prg   # extract one file
cargo run --bin s64val -- disk.d64                       # validate vs. the BAM
cargo run --bin s64write -- format disk.d64 "my disk" 01 # create a blank image
cargo run --bin s64write -- add disk.d64 hello hello.prg # write a host file in
cargo run --bin s64write -- rename disk.d64 hello hi     # rename
cargo run --bin s64write -- delete disk.d64 hi           # scratch
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
   The `t64`/`lnx`/`zipcode` importers have their own `c1541` differentials too.
3. **Robustness fuzzing** (`tests/fuzz_robustness.rs`): a dependency-free,
   deterministic fuzzer that throws random and corrupted input at every parser
   and asserts none panic — including buffers at the exact image sizes and valid
   disks with bytes flipped. For coverage-guided libFuzzer fuzzing, see
   [`fuzz/`](fuzz/) (needs nightly).
4. **Emulator-minted REL** (`tests/rel_fixture.rs`): reads `fixtures/rel.d64`, a
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
| `src/container.rs` | T64 / LNX / LBR / Zip-Code importers |
| `src/g64.rs`       | G64 GCR bitstream decode → D64 |
| `src/cvt.rs`       | GEOS ConVerT (CVT) container describe |
| `src/bin/s64cvt.rs` | CLI: describe a GEOS CVT container |
| `src/tap.rs`       | TAP datasette pulse-stream decode |
| `src/bin/s64tap.rs` | CLI: summarise a TAP tape image |
| `src/wr3.rs`       | Wraptor-3 archive container walk |
| `src/bin/s64wr3.rs` | CLI: list a Wraptor-3 archive |
| `src/petscii.rs`   | minimal PETSCII→UTF-8 for display |
| `src/error.rs`     | error type (every malformed input is a recoverable error) |
| `src/bin/s64ls.rs` | CLI directory listing |
| `src/bin/s64extract.rs` | CLI: extract one file to stdout |
| `src/bin/s64val.rs` | CLI: validate the disk against its BAM |
| `src/bin/s64write.rs` | CLI: format, add, delete, rename |
| `src/arc.rs`       | Commodore ARC + SDA walk, stored + crunch (LZW+RLE) decode |
| `src/lha.rs`       | LHA / SFX archive walk + lh0/lh1 decode |
| `src/lzhuf.rs`     | LZHUF (LHA `-lh1-`) decompressor + CRC-16 |
| `src/bin/s64arc.rs` | CLI: list / extract an ARC or SDA archive |
| `src/bin/s64lha.rs` | CLI: list / extract an LHA or SFX archive |

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option.
