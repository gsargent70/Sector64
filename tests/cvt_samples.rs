//! CVT describe-level checks against real GEOS ConVerT samples.
//!
//! There is no local GEOS tool to validate record data against, so — like the
//! reference `geoinfo` — these assert the parsed header: name, structure, GEOS
//! file type, signature, and the VLIR record index's self-consistency (the
//! signature + info + index + data cover the whole file). The samples are not
//! committed; point `SECTOR64_SAMPLES` at a directory holding them (or use
//! `~/Downloads/Sector64 Test Files`). Skips cleanly when absent.

use std::path::PathBuf;

use sector64::{parse_cvt, GeosStructure};

fn samples_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SECTOR64_SAMPLES") {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join("Downloads/Sector64 Test Files");
    p.is_dir().then_some(p)
}

fn check(file: &str, name: &str, structure: GeosStructure, geos_type: u8, min_records: usize) {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join(file)) else {
        eprintln!("skipping {file}: not present");
        return;
    };
    let info = parse_cvt(&bytes).unwrap_or_else(|e| panic!("{file}: parse_cvt: {e}"));
    assert_eq!(info.name, name, "{file}: name");
    assert_eq!(info.structure, structure, "{file}: structure");
    assert_eq!(info.geos_file_type, geos_type, "{file}: geos type");
    assert!(info.signature.contains("formatted GEOS file"), "{file}: signature");
    assert_eq!(info.info_block.len(), 254, "{file}: info block size");
    assert!(info.records.len() >= min_records, "{file}: record count");

    // Self-consistency: signature(254) + info(254) + index(254) + data == file.
    let header = if structure == GeosStructure::Vlir { 3 * 254 } else { 2 * 254 };
    assert_eq!(header + info.data.len(), bytes.len(), "{file}: block tiling");
    eprintln!("ok: {file} ({} records, {} data bytes)", info.records.len(), info.data.len());
}

#[test]
fn desktop_cvt() {
    check("DESKTOPV1.32.CVT", "DESK TOP", GeosStructure::Vlir, 4, 2);
}

#[test]
fn geossym_cvt() {
    check("GEOSSYM.CVT", "geosSym", GeosStructure::Vlir, 7, 18);
}
