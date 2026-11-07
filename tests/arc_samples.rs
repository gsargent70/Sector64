//! ARC container differential against real-world sample archives.
//!
//! No local tool dissolves Commodore ARC, so these assert the *container* walk
//! (entry count, names, types, methods, sizes, block spans) against the two
//! real archives' known directory contents — the same facts an ARC lister
//! prints. The data files are not committed; point `SECTOR64_SAMPLES` at a
//! directory holding them (or drop them in `~/Downloads/Sector64 Test Files`).
//! The test skips cleanly when the files are absent.

use std::path::PathBuf;

use sector64::{parse_arc, ArcMethod, FileType};

fn samples_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SECTOR64_SAMPLES") {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join("Downloads/Sector64 Test Files");
    p.is_dir().then_some(p)
}

/// (name, type, method, original size in bytes, packed blocks).
type Row = (&'static str, FileType, ArcMethod, u32, u16);

fn check(file: &str, expected: &[Row]) {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let path = dir.join(file);
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skipping {file}: not present");
        return;
    };

    let entries = parse_arc(&bytes).unwrap_or_else(|e| panic!("{file}: parse_arc: {e}"));
    assert_eq!(entries.len(), expected.len(), "{file}: entry count");
    for (got, want) in entries.iter().zip(expected) {
        assert_eq!(got.name, want.0, "{file}: name");
        assert_eq!(got.file_type, want.1, "{file}: type of {}", want.0);
        assert_eq!(got.method, want.2, "{file}: method of {}", want.0);
        assert_eq!(got.orig_size, want.3, "{file}: orig_size of {}", want.0);
        assert_eq!(got.blocks, want.4, "{file}: blocks of {}", want.0);
        assert_eq!(got.version, 2, "{file}: version of {}", want.0);
    }
    eprintln!("ok: {file} ({} entries)", entries.len());
}

#[test]
fn flaghunt_arc_directory() {
    check(
        "FLAGHUNT.ARC",
        &[
            ("DOC PRINTER", FileType::Prg, ArcMethod::Huffman, 6677, 26),
            ("MODEM FLAGHUNT", FileType::Prg, ArcMethod::Huffman, 35405, 135),
            ("T.FLAGHUNT DOCS", FileType::Prg, ArcMethod::Lz, 14011, 29),
        ],
    );
}

/// Decode MILLEBORNE.ARC's crunch members and compare to the original PRG
/// files (known plaintext), when both are present in the samples dir.
#[test]
fn milleborne_crunch_matches_originals() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join("MILLEBORNE.ARC")) else {
        eprintln!("skipping: MILLEBORNE.ARC not present");
        return;
    };
    let entries = parse_arc(&bytes).expect("parse");
    // (archive entry name, original file name, expected method)
    let map = [
        ("MILLE BORNE/RB", "mille borne-rb.prg", ArcMethod::Lz),
        ("MILLE MOD2", "mille mod2.prg", ArcMethod::Lz),
        // Squash = Huffman over the run-length stream (the cracked method).
        ("MILLE MOD1", "mille mod1.prg", ArcMethod::HuffmanRle),
    ];
    let mut checked = 0;
    for (name, orig, method) in map {
        let Ok(plain) = std::fs::read(dir.join(orig)) else {
            eprintln!("skipping {orig}: original not present");
            continue;
        };
        let e = entries.iter().find(|e| e.name == name).expect("entry");
        assert_eq!(e.method, method, "{name} method");
        let got = e.decode().unwrap_or_else(|err| panic!("decode {name}: {err}"));
        assert_eq!(got, plain, "decode mismatch for {name}");
        checked += 1;
    }
    eprintln!("ok: MILLEBORNE ({checked} members matched originals)");
}

/// Every crunch member of both archives at least decodes to its stated size
/// without error (structural check for files we lack originals for).
#[test]
fn crunch_members_decode_to_size() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    for file in ["MILLEBORNE.ARC", "FLAGHUNT.ARC"] {
        let Ok(bytes) = std::fs::read(dir.join(file)) else {
            continue;
        };
        for e in parse_arc(&bytes).expect("parse") {
            if matches!(
                e.method,
                ArcMethod::Rle
                    | ArcMethod::Huffman
                    | ArcMethod::HuffmanRle
                    | ArcMethod::Lz
                    | ArcMethod::LzOnePass
            ) {
                let got = e.decode().unwrap_or_else(|err| panic!("{file}/{}: {err}", e.name));
                assert_eq!(got.len(), e.orig_size as usize, "{file}/{} size", e.name);
            }
        }
    }
}

/// Decode FLAGHUNT.ARC's squeeze (method 2, Huffman-only) members and compare
/// to their originals, when a plaintext directory is provided via
/// `ARC_PLAINTEXT` (the files live outside the samples dir). Skips otherwise.
#[test]
fn flaghunt_squeeze_matches_originals() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Ok(pt) = std::env::var("ARC_PLAINTEXT") else {
        eprintln!("skipping: ARC_PLAINTEXT not set");
        return;
    };
    let pt = PathBuf::from(pt);
    let Ok(bytes) = std::fs::read(dir.join("FLAGHUNT.ARC")) else {
        eprintln!("skipping: FLAGHUNT.ARC not present");
        return;
    };
    let entries = parse_arc(&bytes).expect("parse");
    let map = [
        ("DOC PRINTER", "doc printer.prg", ArcMethod::Huffman),
        ("MODEM FLAGHUNT", "modem flaghunt.prg", ArcMethod::Huffman),
        ("T.FLAGHUNT DOCS", "t.flaghunt docs.prg", ArcMethod::Lz),
    ];
    let mut checked = 0;
    for (name, orig, method) in map {
        let Ok(plain) = std::fs::read(pt.join(orig)) else {
            eprintln!("skipping {orig}: not present");
            continue;
        };
        let e = entries.iter().find(|e| e.name == name).expect("entry");
        assert_eq!(e.method, method, "{name} method");
        let got = e.decode().unwrap_or_else(|err| panic!("decode {name}: {err}"));
        assert_eq!(got, plain, "decode mismatch for {name}");
        checked += 1;
    }
    assert!(checked > 0, "ARC_PLAINTEXT set but nothing matched");
    eprintln!("ok: FLAGHUNT squeeze ({checked} members matched originals)");
}

#[test]
fn milleborne_arc_directory() {
    check(
        "MILLEBORNE.ARC",
        &[
            ("MILLE BORNE/RB", FileType::Prg, ArcMethod::Lz, 8503, 24),
            ("MILLE MOD1", FileType::Prg, ArcMethod::HuffmanRle, 16363, 37),
            ("MILLE MOD2", FileType::Prg, ArcMethod::Lz, 1224, 4),
        ],
    );
}
