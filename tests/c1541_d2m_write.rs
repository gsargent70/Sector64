//! DNP write differential: add a file to a real D2M (CMD-native) image with
//! this crate, then confirm `c1541` reads it back byte-for-byte. Skipped when
//! the sample or `c1541` is absent.

use std::path::PathBuf;
use std::process::Command;

use sector64::{Disk, FileType};

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

#[test]
fn dnp_write_reads_back_in_c1541() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join("cbm-prog_fd-copy-test-disk.d2m")) else {
        eprintln!("skipping: fd-copy-test-disk.d2m not present");
        return;
    };

    let mut disk = Disk::load(bytes).expect("load");
    let before = disk.blocks_free().unwrap();
    let data: Vec<u8> = [0x01u8, 0x08].into_iter().chain((0..500u32).map(|x| x as u8)).collect();
    disk.write_file("TESTADD", FileType::Prg, &data).expect("write_file");

    // The 502-byte PRG occupies two 254-byte blocks.
    assert_eq!(before - disk.blocks_free().unwrap(), 2, "free-block delta");

    // Our own read-back.
    let entry = disk.find_entry("TESTADD").unwrap().expect("entry present");
    assert_eq!(disk.read_entry(&entry).unwrap(), data, "our read-back");

    if !c1541_available() {
        eprintln!("skipping c1541 check: not found");
        return;
    }
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("d2m_write");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let img = work.join("out.d2m");
    std::fs::write(&img, disk.as_bytes()).unwrap();

    let out = work.join("r");
    let ok = Command::new("c1541")
        .arg(&img)
        .arg("-read")
        .arg("testadd")
        .arg(&out)
        .status()
        .unwrap()
        .success();
    assert!(ok, "c1541 could not read the added file");
    assert_eq!(std::fs::read(&out).unwrap(), data, "c1541 read differs from written");
    eprintln!("ok: DNP write — c1541 reads the added file byte-for-byte");
}
