//! D71 (1571) differentials against VICE's `c1541`, in both directions.
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

/// We write a D71; c1541 reads it back.
#[test]
fn c1541_reads_the_d71_we_write() {
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c1541_d71_write");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // The last file is large enough to use side 1.
    let sizes: [usize; 5] = [1, 254, 800, 2, 180_000];
    let names: Vec<String> = (0..sizes.len()).map(|i| format!("file{i}")).collect();

    let mut disk = Disk::format_d71("writetest", "71");
    let mut contents = Vec::new();
    for (i, &size) in sizes.iter().enumerate() {
        let data = pseudo_bytes(i as u32 + 1, size);
        disk.write_file(&names[i], FileType::Prg, &data).unwrap();
        contents.push(data);
    }
    let img = dir.join("mine.d71");
    fs::write(&img, disk.as_bytes()).unwrap();

    for (name, data) in names.iter().zip(&contents) {
        let out = dir.join(format!("{name}.ref"));
        let ok = Command::new("c1541")
            .arg(&img)
            .arg("-read")
            .arg(name)
            .arg(&out)
            .status()
            .expect("c1541 -read")
            .success();
        assert!(ok, "c1541 could not read {name} from our D71");
        assert_eq!(&fs::read(&out).unwrap(), data, "D71 read mismatch for {name}");
    }
    if let Some(free) = blocks_free_from_dir(&img) {
        assert_eq!(disk.blocks_free().unwrap(), free, "D71 blocks free vs c1541");
    }
}

/// c1541 writes a D71; we read it.
#[test]
fn we_read_a_c1541_d71() {
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c1541_d71_read");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let sizes: [usize; 5] = [2, 254, 255, 1000, 180_000];
    let names: Vec<String> = (0..sizes.len()).map(|i| format!("f{i}")).collect();

    let mut host = Vec::new();
    for (i, &size) in sizes.iter().enumerate() {
        let p = dir.join(format!("{}.prg", names[i]));
        fs::write(&p, pseudo_bytes(i as u32 + 7, size)).unwrap();
        host.push(p);
    }

    let img = dir.join("c.d71");
    let mut cmd = Command::new("c1541");
    cmd.args(["-format", "readtest,71", "d71"]).arg(&img);
    for (p, name) in host.iter().zip(&names) {
        cmd.arg("-write").arg(p).arg(name);
    }
    assert!(cmd.status().expect("c1541 build d71").success());

    let disk = Disk::load(fs::read(&img).unwrap()).unwrap();
    assert_eq!(disk.track_count(), 70);
    assert_eq!(disk.directory().unwrap().len(), names.len());
    for (p, name) in host.iter().zip(&names) {
        let e = disk.find_entry(name).unwrap().expect("entry present");
        assert_eq!(disk.read_entry(&e).unwrap(), fs::read(p).unwrap(), "{name}");
    }
    assert!(disk.validate().unwrap().is_clean());
    if let Some(free) = blocks_free_from_dir(&img) {
        assert_eq!(disk.blocks_free().unwrap(), free, "blocks free vs c1541");
    }
}
