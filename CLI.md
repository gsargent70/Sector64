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
  failure. (`s64cmp` uses `1` for "they differ" and `2` for any failure.)
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
s64extract [--list[=v2|7.0|simons]] <image> <name>
```

Writes the raw bytes of the named file to stdout. `<name>` is matched
case-insensitively against the displayed directory name. Redirect to
save:

```sh
s64extract games.d64 "ZORK I" > zork.prg
```

With `--list`, a BASIC program is printed as text instead, the way
`LIST` shows it on a C64. Control codes inside quotes are shown by name
(`{CLR}`, `{RED}`, `{5 DOWN}`), as in printed listings. The dialect is
detected: BASIC 7.0 for programs saved on a C128, Simons' BASIC when its
keywords are used, otherwise BASIC V2. Give it with `--list=v2`,
`--list=7.0` or `--list=simons` when the guess is wrong. stderr says
which dialect was used, and whether machine code follows the program. A
file that isn't a BASIC program is an error (exit 1).

```sh
s64extract --list games.d64 menu
```

### `s64val` — validate a disk against its BAM

```
s64val [--map] <image>
```

Walks every file's block chain and compares it to the Block Availability
Map, reporting cross-linked blocks and blocks that are in use but marked
free. Exits non-zero if the disk is inconsistent or structurally broken,
so it is usable in scripts:

```sh
s64val suspect.d64 && echo "disk is clean"
```

With `--map`, it prints a disk map instead: one line per track (long
tracks wrap every 64 sectors, marked `+64`, `+128`…), one character per
sector, then a legend.

```
       0         1         2
  trk  012345678901234567890
    1  AAAB.................
  ...
   18  ==.................

  .  free               =  DOS (header, BAM, directory)
  ?  allocated, unused   !  conflict (in use but free in the BAM, or shared)
  A  "ONE"              prg  3 blocks
  B  "TWO"              seq  1 block

683 blocks: 4 in files, 2 DOS, 677 free, 0 allocated but unused, 0 in conflict
```

Each file's blocks show its letter (`A`–`Z`, `a`–`z`, `0`–`9`, then `*`).
`?` blocks are allocated but no file uses them: lost blocks, or data a
copy-protected disk or boot loader keeps outside the directory. The map
follows broken chains as far as they go and lists the faults at the end.
It exits non-zero only when a block is in conflict or a chain is broken.
It works on every disk image, G64 and D2M included.

### `s64cmp` — compare two disks or archives

```
s64cmp [--all] <left> <right>
```

Compares the files in two disk images or archives, in any combination (a
disk with the archive it came from, say). Files are matched by name.
Each one that differs, or is on only one side, gets a line with its size
on the left and on the right, then a summary line follows. `--all` lists
the matching files too.

```
* "EDITED"                  4        6  differs from byte 2
* "RETYPED"                 2        2  type PRG / SEQ
< "GONE"                    1        -  only on the left
> "NEW"                     -        2  only on the right
2 differ, 1 only on the left, 1 only on the right, 1 the same
```

The mark is `=` same, `*` different, `<` only on the left, `>` only on
the right, or `?` unreadable on one side. LHA and Wraptor archives don't
record file types, so only the bytes are compared. Any part of a Zip-Code
set (`1!NAME` to `4!NAME`) stands for the whole set. Disk names, IDs and
lock flags aren't compared.

The exit status is like `cmp` and `diff`: `0` when the files are all the
same, `1` when they aren't, `2` when the two couldn't be compared (wrong
arguments, a missing file, or a format it doesn't know).

```sh
s64cmp original.d64 copy.d64 && echo "good copy"
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
| `s64extract` | Extract one file (or list it as BASIC)    | stdout       |
| `s64val`     | Validate a disk, or draw its block map    | stdout + exit|
| `s64cmp`     | Compare two disks or archives             | stdout + exit|
| `s64write`   | Create / edit a D64 (format/add/del/ren)  | the image    |
| `s64pack`    | Create a t64/lnx/lbr/p00 archive          | the archive  |
| `s64arc`     | List / extract ARC or SDA                 | stdout       |
| `s64lha`     | List / extract LHA (.lzh/.sfx)            | stdout       |
| `s64wr3`     | List / extract Wraptor 3                  | files/stdout |
| `s64cvt`     | Describe / extract a GEOS CVT             | stdout/files |
| `s64tap`     | Summarise a TAP tape image                | stdout       |
