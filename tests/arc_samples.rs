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
    // (archive entry name, original file name)
    let map = [
        ("MILLE BORNE/RB", "mille borne-rb.prg"),
        ("MILLE MOD2", "mille mod2.prg"),
    ];
    let mut checked = 0;
    for (name, orig) in map {
        let Ok(plain) = std::fs::read(dir.join(orig)) else {
            eprintln!("skipping {orig}: original not present");
            continue;
        };
        let e = entries.iter().find(|e| e.name == name).expect("entry");
        assert_eq!(e.method, ArcMethod::Lz, "{name} should be crunch");
        let got = e.decode().unwrap_or_else(|err| panic!("decode {name}: {err}"));
        assert_eq!(got, plain, "crunch decode mismatch for {name}");
        checked += 1;
    }
    eprintln!("ok: MILLEBORNE crunch ({checked} members matched originals)");
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
            if matches!(e.method, ArcMethod::Lz | ArcMethod::LzOnePass) {
                let got = e.decode().unwrap_or_else(|err| panic!("{file}/{}: {err}", e.name));
                assert_eq!(got.len(), e.orig_size as usize, "{file}/{} size", e.name);
            }
        }
    }
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
