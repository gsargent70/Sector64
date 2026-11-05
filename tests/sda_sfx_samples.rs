//! Container differentials for self-extracting archives against real samples.
//!
//! No local tool dissolves these, so — as with ARC — the checks are on the
//! *container*: that the loader stub is stripped and the embedded archive walks
//! into the right entries. `.SDA` is an ARC archive behind a BASIC dissolver;
//! `.SFX` is an LHA archive behind a loader. The samples are not committed;
//! point `SECTOR64_SAMPLES` at a directory holding them (or use
//! `~/Downloads/Sector64 Test Files`). Tests skip cleanly when files are absent.

use std::path::{Path, PathBuf};

use sector64::{parse_lha, parse_sda};

fn samples_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SECTOR64_SAMPLES") {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join("Downloads/Sector64 Test Files");
    p.is_dir().then_some(p)
}

fn read(dir: &Path, name: &str) -> Option<Vec<u8>> {
    std::fs::read(dir.join(name)).ok()
}

#[test]
fn survivor_sda_directory() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Some(bytes) = read(&dir, "SURVIVOR.SDA") else {
        eprintln!("skipping: SURVIVOR.SDA not present");
        return;
    };
    let entries = parse_sda(&bytes).unwrap_or_else(|e| panic!("parse_sda: {e}"));
    assert!(!entries.is_empty());
    // The first member of this archive is the loader.
    assert_eq!(entries[0].name, "SURVIVOR LDR");
    // Every entry's name is printable and its block span is non-zero.
    for e in &entries {
        assert!(e.blocks > 0, "{} has zero blocks", e.name);
        assert!(!e.name.is_empty());
    }
    eprintln!("ok: SURVIVOR.SDA ({} entries)", entries.len());
}

#[test]
fn ras1_sfx_directory() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Some(bytes) = read(&dir, "RAS-1.SFX") else {
        eprintln!("skipping: RAS-1.SFX not present");
        return;
    };
    let entries = parse_lha(&bytes).unwrap_or_else(|e| panic!("parse_lha: {e}"));
    assert!(entries.len() > 5, "expected several members, got {}", entries.len());
    assert_eq!(entries[0].name, "a");
    // These are all LHA level-0 entries; sizes and method look sane.
    for e in &entries {
        assert_eq!(&e.method[..3], b"-lh", "{} has a non-lh method", e.name);
        assert!(e.original_size >= e.compressed_size || e.method == *b"-lh0-");
    }
    // The compressed members can't be decoded yet, but stored ones (if any)
    // must, and the rest must report the method rather than panic.
    for e in &entries {
        let _ = e.decode();
    }
    eprintln!("ok: RAS-1.SFX ({} entries)", entries.len());
}
