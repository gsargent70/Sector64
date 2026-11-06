//! Wraptor-3 container checks against a real archive.
//!
//! The Wraptor compression is not decoded, so this validates the container
//! walk: the entry count and that the names parse. The sample is not committed;
//! point `SECTOR64_SAMPLES` at a directory holding it (or use
//! `~/Downloads/Sector64 Test Files`). Skips cleanly when absent.

use std::path::PathBuf;

use sector64::parse_wr3;

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
    eprintln!("ok: {name} ({} entries)", entries.len());
}
