# Sector64 command-line tools

Sector64 ships a set of small, single-purpose command-line programs that
cover the same formats as the GUI from a shell. Each does one job, reads
its input from a file path, and writes results to standard output so they
compose with normal shell redirection and pipes.

This is the reference for those tools. For the graphical browser, see
[`gui/MANUAL.md`](gui/MANUAL.md).

## Building and running

The tools are binaries of the `sector64` crate. Build them all with:

```sh
cargo build --release
```

The executables are then under `target/release/` (`s64ls`,
`s64extract`, …). Run one directly from there, or `cargo run` it:

```sh
target/release/s64ls disk.d64
cargo run --release --bin s64ls -- disk.d64
```

The examples below use the bare command name as if the tools are on your
`PATH`.

## Conventions

- **Input** is always a file path given as an argument. Nothing reads
  from stdin.
- **Extracted bytes go to stdout**, raw and unmodified (for a PRG that
  includes the two-byte load address). Redirect to a file to save them.
- **Messages and errors go to stderr**, so they never mix into extracted
  data.
- **Exit status**: `0` on success, `2` for a usage error (wrong
  arguments), and `1` (non-zero) when the operation fails — a missing
  file, an unreadable image, a name that isn't present, or a validation
  failure.
- **CBM names** are matched case-insensitively against the name as shown
  in the directory listing.

---

## Disk images

### `s64ls` — list a disk directory

```
s64ls <image.d64>
```

Prints the directory of a disk image in the classic Commodore layout
(the header line, each file with its block count and type, and the
blocks-free line).

```sh
s64ls games.d64
```

### `s64extract` — extract one file to stdout

```
s64extract <image.d64> <name>
```

Writes the raw bytes of the named file to stdout. `<name>` is matched
case-insensitively against the displayed directory name. Redirect to
save:

```sh
s64extract games.d64 "ZORK I" > zork.prg
```

### `s64val` — validate a disk against its BAM

```
s64val <image.d64>
```

Walks every file's block chain and compares it to the Block Availability
Map, reporting cross-linked blocks and blocks that are in use but marked
free. Exits non-zero if the disk is inconsistent or structurally broken,
so it is usable in scripts:

```sh
s64val suspect.d64 && echo "disk is clean"
```

### `s64write` — create and edit D64 images

```
s64write format <image.d64> <disk name> <id>
s64write add    <image.d64> <cbm name> <host file> [prg|seq|usr]
s64write delete <image.d64> <cbm name>
s64write rename <image.d64> <old name> <new name>
```

Creates and edits a D64 image in place. `format` writes a fresh, empty
image. `add` stores a host file's raw bytes as a new directory entry
(type defaults to `prg`; for a PRG the bytes include the two-byte load
address). `delete` scratches an entry and frees its blocks. `rename`
changes a name. Edits are written back to the same image file.

```sh
s64write format disk.d64 "MY DISK" 2A
s64write add    disk.d64 HELLO hello.prg prg
s64write add    disk.d64 README readme.txt seq
s64write rename disk.d64 HELLO GREETING
s64write delete disk.d64 README
```

---

## Archives

### `s64pack` — create an archive from host files

```
s64pack <out.t64|.lnx|.lbr|.p00> <file>...
```

Packs one or more host files into a Commodore container. The output
format is chosen by the extension you give the output file. Each input
becomes a member whose CBM name is the upper-cased file stem (≤16
characters) and whose type is PRG. A `.p00` holds exactly one file.

```sh
s64pack demos.t64 part1.prg part2.prg part3.prg
s64pack single.p00 game.prg
```

### `s64arc` — list or extract ARC / SDA archives

```
s64arc list    <archive.arc|.sda>
s64arc extract <archive.arc|.sda> <name>
```

Reads a Commodore **ARC** archive, or an **SDA** self-dissolving archive
(detected by its BASIC loader; its embedded ARC is read automatically).
`list` shows the members, their sizes, block counts, and compression
method. `extract` decompresses one member and writes it to stdout —
store, pack, squeeze, crunch, and squash members all decode. A member
using a method that is not implemented reports that method instead.

```sh
s64arc list    utils.arc
s64arc extract utils.arc "COPY ALL" > copyall.prg
```

### `s64lha` — list or extract LHA archives

```
s64lha list    <archive.lzh|.sfx>
s64lha extract <archive.lzh|.sfx> <name>
```

Reads a Commodore **LHA** archive (`.lzh`), or a self-extracting `.sfx`
(the first archive header past its loader stub is located automatically).
`list` shows the members; `extract` decompresses one to stdout
(`-lh0-` stored and `-lh1-` members decode).

```sh
s64lha list    pack.lzh
s64lha extract pack.lzh INTRO > intro.prg
```

### `s64wr3` — list or extract Wraptor-3 archives

```
s64wr3 list    <archive.wr3>
s64wr3 extract <archive.wr3> [out-dir]
```

Reads a **Wraptor 3** (`.wr3`) archive. `list` shows member names and
decompressed sizes. Because every member shares one sliding window, the
whole archive is decoded together: `extract` writes each member into
`out-dir` (default: the current directory) under a safe file name.

```sh
s64wr3 list demo.wr3
s64wr3 extract demo.wr3 ./out
```

---

## Other containers

### `s64cvt` — describe or extract a GEOS CVT file

```
s64cvt <file.cvt>            describe the GEOS file
s64cvt <file.cvt> <outdir>   also write each record to disk
```

Reads a GEOS **CVT** (ConVerT) container. With just the file, it prints
the GEOS file's name, types, structure and — for a VLIR file — its record
index. With an output directory (created if it doesn't exist), it also
writes the INFO block and each record's data as separate files
(`<name>.rNN`).

```sh
s64cvt notepad.cvt
s64cvt notepad.cvt ./geos-out
```

### `s64tap` — summarise a TAP tape image

```
s64tap <file.tap>
```

Prints the header and pulse-stream statistics of a Commodore **TAP**
datasette image. A TAP records raw tape pulses rather than files, and the
bit encoding is loader-specific (most commercial tapes use custom turbo
loaders), so there is no general file extraction — this tool reports on
the pulse stream only.

```sh
s64tap game.tap
```

---

## Quick reference

| Command      | Purpose                                   | Writes to    |
|--------------|-------------------------------------------|--------------|
| `s64ls`      | List a disk directory                     | stdout       |
| `s64extract` | Extract one file from a disk              | stdout       |
| `s64val`     | Validate a disk against its BAM           | stderr + exit|
| `s64write`   | Create / edit a D64 (format/add/del/ren)  | the image    |
| `s64pack`    | Create a t64/lnx/lbr/p00 archive          | the archive  |
| `s64arc`     | List / extract ARC or SDA                 | stdout       |
| `s64lha`     | List / extract LHA (.lzh/.sfx)            | stdout       |
| `s64wr3`     | List / extract Wraptor 3                  | files/stdout |
| `s64cvt`     | Describe / extract a GEOS CVT             | stdout/files |
| `s64tap`     | Summarise a TAP tape image                | stdout       |
