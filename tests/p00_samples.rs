//! PC64 `.P00` differential against a real-world sample.
//!
//! A P00 is a single file behind a 26-byte header, so correctness is the name
//! parse and byte-exact payload. This asserts the known name, then — when
//! `c1541` is present — writes the extracted file into a fresh D64 and reads it
//! back with `c1541` to confirm the payload survives a round-trip through an
//! independent tool. The sample is not committed; point `SECTOR64_SAMPLES` at a
//! directory holding it (or use `~/Downloads/Sector64 Test Files`). Skips
//! cleanly when the file or `c1541` is absent.

use std::path::PathBuf;
use std::process::Command;

use sector64::{parse_p00, Disk, FileType};

fn samples_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SECTOR64_SAMPLES") {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join("Downloads/Sector64 Test Files");
    p.is_dir().then_some(p)
}

fn c1541_available() -> bool {
    Command::new("c1541")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

const SAMPLE: &str = "Boulder Dash - Construction Kit (1986)(First Star).p00";

#[test]
fn boulderdash_p00_roundtrip() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join(SAMPLE)) else {
        eprintln!("skipping {SAMPLE}: not present");
        return;
    };

    let file = parse_p00(&bytes).unwrap_or_else(|e| panic!("parse_p00: {e}"));
    assert_eq!(file.name, "BOULDER D. CNS.");
    assert_eq!(file.file_type, FileType::Prg);
    // Payload is everything past the 26-byte header, load address first.
    assert_eq!(file.data.len(), bytes.len() - 26);
    assert_eq!(&file.data[..2], &[0x01, 0x08]); // $0801

    if !c1541_available() {
        eprintln!("skipping round-trip: c1541 not found");
        return;
    }

    let mut disk = Disk::format("p00 test", "p0");
    disk.write_file("bd", file.file_type, &file.data).expect("write");
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p00_rt");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let img = work.join("p00.d64");
    std::fs::write(&img, disk.as_bytes()).unwrap();

    let out = work.join("bd.read");
    let ok = Command::new("c1541")
        .arg(&img)
        .arg("-read")
        .arg("bd")
        .arg(&out)
        .status()
        .unwrap()
        .success();
    assert!(ok, "c1541 could not read the imported file");
    assert_eq!(std::fs::read(&out).unwrap(), file.data, "payload differs after round-trip");
    eprintln!("ok: {SAMPLE} ({} bytes) round-tripped through c1541", file.data.len());
}
