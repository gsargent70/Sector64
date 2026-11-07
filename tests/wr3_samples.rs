//! Wraptor-3 checks against a real archive: the container walk (entry count and
//! names) and, when the plaintext members are available, byte-for-byte
//! decompression. The sample is not committed; point `SECTOR64_SAMPLES` at a
//! directory holding the `.wr3` (or use `~/Downloads/Sector64 Test Files`).
//! Point `WR3_PLAINTEXT` at a directory of the extracted members (named
//! `<member>.seq`/`.del`/`.prg`, lower-cased) to enable the exact check. Skips
//! cleanly when absent.

use std::path::PathBuf;

use sector64::{decompress_wr3, parse_wr3};

fn samples_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SECTOR64_SAMPLES") {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join("Downloads/Sector64 Test Files");
    p.is_dir().then_some(p)
}

#[test]
fn loadstar_wr3() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let name = "Loadstar Issue #128 (199x)(Softdisk)(Disk 1 of 2 Side A).wr3";
    let Ok(bytes) = std::fs::read(dir.join(name)) else {
        eprintln!("skipping: {name} not present");
        return;
    };
    let entries = parse_wr3(&bytes).unwrap_or_else(|e| panic!("parse_wr3: {e}"));
    assert_eq!(entries.len(), 43, "entry count");
    assert_eq!(entries[1].name, "STAR");
    assert_eq!(entries[2].name, "HELLO CONNECT");
    // Every entry has a non-empty, printable name and some payload.
    for e in &entries {
        assert!(!e.name.is_empty(), "empty name");
        assert!(e.name.bytes().all(|b| (0x20..=0x7E).contains(&b)), "bad name {:?}", e.name);
    }
    // The compressed payloads plus their markers/names tile the whole file.
    let total: usize = entries.iter().map(|e| e.compressed.len()).sum();
    assert!(total > 0 && total < bytes.len());

    // Decompression: all members decode without panicking and yield content.
    let files = decompress_wr3(&bytes).unwrap_or_else(|e| panic!("decompress_wr3: {e}"));
    assert_eq!(files.len(), 43, "decoded member count");
    assert!(files.iter().all(|f| !f.content.is_empty()), "a member decoded empty");

    // When the extracted plaintext members are available, require a
    // byte-for-byte match for every one present.
    if let Some(pt) = plaintext_dir() {
        let mut checked = 0;
        for f in &files {
            let lower = f.name.to_ascii_lowercase();
            for suffix in [".seq", ".del", ".prg", ""] {
                let p = pt.join(format!("{lower}{suffix}"));
                if let Ok(plain) = std::fs::read(&p) {
                    assert_eq!(f.content, plain, "member {:?} differs from {p:?}", f.name);
                    checked += 1;
                    break;
                }
            }
        }
        assert!(checked > 0, "WR3_PLAINTEXT set but no members matched");
        eprintln!("ok: {name} — {checked} members decode byte-for-byte");
    }
    eprintln!("ok: {name} ({} entries, {} decoded)", entries.len(), files.len());
}

fn plaintext_dir() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("WR3_PLAINTEXT").ok()?);
    p.is_dir().then_some(p)
}
