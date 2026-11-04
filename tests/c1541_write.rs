//! Write round-trip against VICE's `c1541`.
//!
//! Formats a blank disk and writes files with this crate, then confirms that
//! `c1541` — an independent implementation — reads every file back byte-for-byte
//! and agrees on the file count and blocks-free. This is the reverse of
//! `c1541_diff.rs`: it proves what we *write* is valid, not just what we read.
//!
//! Skipped (not failed) when `c1541` is not on `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use d64::{Disk, FileType};

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
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(rest) = line.trim().strip_suffix("blocks free.") {
            return rest.trim().parse().ok();
        }
    }
    None
}

#[test]
fn c1541_reads_what_we_write() {
    if !c1541_available() {
        eprintln!("skipping: c1541 (VICE) not found on PATH");
        return;
    }

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c1541_write");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Boundary sizes again: tiny, exactly one block (254), one past it (255),
    // and multi-block. Eleven files also push onto a second directory sector.
    let sizes: [usize; 11] = [1, 2, 10, 200, 254, 255, 256, 509, 800, 2, 5000];
    let names: Vec<String> = (0..sizes.len()).map(|i| format!("file{i:02}")).collect();

    let mut disk = Disk::format("writetest", "01");
    let mut contents = Vec::new();
    for (i, &size) in sizes.iter().enumerate() {
        let data = pseudo_bytes(i as u32 + 1, size);
        disk.write_file(&names[i], FileType::Prg, &data).unwrap();
        contents.push(data);
    }

    let img = dir.join("mine.d64");
    fs::write(&img, disk.as_bytes()).unwrap();

    // c1541 reads each file back identically.
    for (name, data) in names.iter().zip(&contents) {
        let out = dir.join(format!("{name}.ref"));
        let status = Command::new("c1541")
            .arg(&img)
            .arg("-read")
            .arg(name)
            .arg(&out)
            .status()
            .expect("run c1541 -read");
        assert!(status.success(), "c1541 could not read {name} we wrote");
        assert_eq!(&fs::read(&out).unwrap(), data, "c1541 read mismatch for {name}");
    }

    // c1541's view agrees on the file count and the free count.
    let out = Command::new("c1541").arg(&img).arg("-dir").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let prg_lines = text.lines().filter(|l| l.contains(" prg")).count();
    assert_eq!(prg_lines, names.len(), "c1541 directory file count");

    if let Some(free) = blocks_free_from_dir(&img) {
        assert_eq!(disk.blocks_free().unwrap(), free, "blocks free vs c1541");
    }
}
