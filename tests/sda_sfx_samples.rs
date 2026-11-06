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
    // Mostly LHA level-0 -lh1- (LZHUF), with a stored (-lh0-) member. Decode
    // each and verify against the archive's own length and CRC-16 — an
    // independent oracle for the decompressor.
    let mut lh1 = 0;
    for e in &entries {
        let out = e.decode().unwrap_or_else(|err| panic!("decode {}: {err}", e.name));
        assert_eq!(out.len(), e.original_size as usize, "length of {}", e.name);
        assert!(e.verify_crc(&out), "CRC mismatch for {}", e.name);
        if e.method_str() == "-lh1-" {
            lh1 += 1;
        }
    }
    assert!(lh1 >= 15, "expected mostly lh1 entries, got {lh1}");
    eprintln!("ok: RAS-1.SFX ({} entries decoded + CRC-checked, {lh1} lh1)", entries.len());
}

#[test]
fn swb_sfx_decodes() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Some(bytes) = read(&dir, "SWB.SFX") else {
        eprintln!("skipping: SWB.SFX not present");
        return;
    };
    let entries = parse_lha(&bytes).unwrap_or_else(|e| panic!("parse_lha: {e}"));
    for e in &entries {
        let out = e.decode().unwrap_or_else(|err| panic!("decode {}: {err}", e.name));
        assert_eq!(out.len(), e.original_size as usize, "length of {}", e.name);
        assert!(e.verify_crc(&out), "CRC mismatch for {}", e.name);
    }
    eprintln!("ok: SWB.SFX ({} entries decoded + CRC-checked)", entries.len());
}
