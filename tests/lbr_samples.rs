//! LBR differential against a real-world sample library.
//!
//! The C64 LBR format carries no compression, so correctness is two things:
//! the directory parse (names/types/sizes) and byte-exact member data. This
//! test asserts the known directory of a real `.lbr`, then — when `c1541` is
//! present — writes every member into a fresh D64 and reads it back with
//! `c1541` to confirm the extracted bytes survive a round-trip through an
//! independent tool. The sample is not committed; point `SECTOR64_SAMPLES` at
//! a directory holding it (or drop it in `~/Downloads/Sector64 Test Files`).
//! The test skips cleanly when the file or `c1541` is absent.

use std::path::PathBuf;
use std::process::Command;

use sector64::{parse_lbr, Disk, FileType};

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

const SAMPLE: &str = "C64 Shotgun (1987)(Kracker Jax).lbr";

/// (name, type, byte size) for every member of the sample, as an LBR lister
/// would show it. Verified to sum (with the directory) to the exact file size.
const EXPECTED: &[(&str, FileType, usize)] = &[
    ("BOOT", FileType::Prg, 249),
    ("C64SHOTGUN", FileType::Prg, 6162),
    ("MIRROR BOOT", FileType::Prg, 905),
    ("LOADER", FileType::Prg, 172),
    ("MIRRO", FileType::Prg, 9218),
    ("GRAPHC", FileType::Prg, 8384),
    ("MIRROR COPY", FileType::Prg, 351),
];

#[test]
fn shotgun_lbr_directory() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join(SAMPLE)) else {
        eprintln!("skipping {SAMPLE}: not present");
        return;
    };

    let files = parse_lbr(&bytes).unwrap_or_else(|e| panic!("parse_lbr: {e}"));
    assert_eq!(files.len(), EXPECTED.len(), "member count");
    for (got, want) in files.iter().zip(EXPECTED) {
        assert_eq!(got.name, want.0, "name");
        assert_eq!(got.file_type, want.1, "type of {}", want.0);
        assert_eq!(got.data.len(), want.2, "size of {}", want.0);
    }
    eprintln!("ok: {SAMPLE} ({} members)", files.len());
}

#[test]
fn shotgun_lbr_members_roundtrip_c1541() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join(SAMPLE)) else {
        eprintln!("skipping {SAMPLE}: not present");
        return;
    };
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }

    let files = parse_lbr(&bytes).unwrap_or_else(|e| panic!("parse_lbr: {e}"));

    // Write every member into a fresh disk. The members total ~25 KB, well
    // within a 1541's capacity. Use short unique names so c1541 can address
    // each one unambiguously (the real names collide/repeat for our purposes).
    let mut disk = Disk::format("lbr test", "lb");
    let mut names = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let name = format!("f{i}");
        disk.write_file(&name, f.file_type, &f.data)
            .unwrap_or_else(|e| panic!("write {name}: {e}"));
        names.push(name);
    }

    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("lbr_rt");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let img = work.join("lbr.d64");
    std::fs::write(&img, disk.as_bytes()).unwrap();

    for (name, f) in names.iter().zip(&files) {
        let out = work.join("m");
        let _ = std::fs::remove_file(&out);
        let ok = Command::new("c1541")
            .arg(&img)
            .arg("-read")
            .arg(name)
            .arg(&out)
            .status()
            .unwrap()
            .success();
        assert!(ok, "c1541 could not read {name}");
        assert_eq!(std::fs::read(&out).unwrap(), f.data, "bytes differ for {} ({name})", f.name);
    }
    eprintln!("ok: {} members round-tripped through c1541", files.len());
}
