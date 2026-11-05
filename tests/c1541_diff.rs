//! Differential test against VICE's `c1541`.
//!
//! Builds a disk with a known set of files using `c1541`, then checks that this
//! crate's directory walk and file extraction agree with `c1541`'s own `read`
//! and `dir` output. This is the test that catches a format *misunderstanding*,
//! since the reference image is produced by an independent implementation.
//!
//! It is skipped (not failed) when `c1541` is not on `PATH`, so `cargo test`
//! still passes on machines without VICE installed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sector64::Disk;

/// Deterministic pseudo-random bytes (a tiny LCG), so no `rand` dependency and
/// the images are reproducible.
fn pseudo_bytes(seed: u32, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761).wrapping_add(1);
    (0..len)
        .map(|_| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 24) as u8
        })
        .collect()
}

fn c1541_available() -> bool {
    Command::new("c1541")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

fn blocks_free_from_dir(img: &Path) -> Option<u16> {
    let out = Command::new("c1541").arg(img).arg("-dir").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_suffix("blocks free.") {
            return rest.trim().parse().ok();
        }
    }
    None
}

#[test]
fn matches_c1541_on_a_known_disk() {
    if !c1541_available() {
        eprintln!("skipping: c1541 (VICE) not found on PATH");
        return;
    }

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c1541_diff");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Sizes chosen to exercise the block/record boundaries: empty-ish, a full
    // single block (254), one byte past it (255), and several multi-block files.
    // Eleven files also push the directory onto a second sector.
    let sizes: [usize; 11] = [2, 3, 10, 200, 254, 255, 256, 509, 800, 2, 5000];
    let names: Vec<String> = (0..sizes.len()).map(|i| format!("file{i:02}")).collect();

    let mut host_paths = Vec::new();
    for (i, &size) in sizes.iter().enumerate() {
        let p = dir.join(format!("{}.prg", names[i]));
        fs::write(&p, pseudo_bytes(i as u32 + 1, size)).unwrap();
        host_paths.push(p);
    }

    // Build the image with c1541.
    let img = dir.join("test.d64");
    let mut cmd = Command::new("c1541");
    cmd.args(["-format", "test disk,01", "d64"]).arg(&img);
    for (p, name) in host_paths.iter().zip(&names) {
        cmd.arg("-write").arg(p).arg(name);
    }
    let status = cmd.status().expect("run c1541 -format/-write");
    assert!(status.success(), "c1541 image build failed");

    let disk = Disk::load(fs::read(&img).unwrap()).unwrap();

    // Directory: one entry per file.
    let entries = disk.directory().unwrap();
    assert_eq!(entries.len(), names.len(), "directory entry count");

    // Each file's bytes must match both the original and c1541's own read-back.
    for (p, name) in host_paths.iter().zip(&names) {
        let original = fs::read(p).unwrap();

        let ref_out = dir.join(format!("{name}.ref"));
        let status = Command::new("c1541")
            .arg(&img)
            .arg("-read")
            .arg(name)
            .arg(&ref_out)
            .status()
            .expect("run c1541 -read");
        assert!(status.success(), "c1541 -read {name} failed");
        let reference = fs::read(&ref_out).unwrap();

        let entry = disk.find_entry(name).unwrap().expect("entry present");
        let mine = disk.read_entry(&entry).unwrap();

        assert_eq!(mine, original, "extracted bytes differ from original for {name}");
        assert_eq!(mine, reference, "extracted bytes differ from c1541 for {name}");
    }

    // Blocks-free must match c1541's footer.
    if let Some(expected_free) = blocks_free_from_dir(&img) {
        assert_eq!(disk.blocks_free().unwrap(), expected_free, "blocks free");
    }
}
