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
    let existing: Vec<(String, Vec<u8>)> = disk
        .directory()
        .unwrap()
        .iter()
        .map(|e| (e.name.clone(), disk.read_entry(e).unwrap()))
        .collect();
    let data: Vec<u8> = [0x01u8, 0x08].into_iter().chain((0..500u32).map(|x| x as u8)).collect();
    disk.write_file("TESTADD", FileType::Prg, &data).expect("write_file");

    // The 502-byte PRG occupies two 254-byte blocks.
    assert_eq!(before - disk.blocks_free().unwrap(), 2, "free-block delta");

    // Our own read-back.
    let entry = disk.find_entry("TESTADD").unwrap().expect("entry present");
    assert_eq!(disk.read_entry(&entry).unwrap(), data, "our read-back");

    // The write must not touch the files already there (it once did: the BAM
    // was read LSB-first, so blocks in use looked free).
    for (name, bytes) in &existing {
        let e = disk.find_entry(name).unwrap().expect("still listed");
        assert_eq!(&disk.read_entry(&e).unwrap(), bytes, "{name} changed");
    }
    assert!(
        disk.validate().unwrap().is_clean(),
        "BAM consistent after write"
    );

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

/// The other direction: c1541 adds a file, and our BAM reading must agree with
/// the blocks it allocated (a clean validate and the same free count).
#[test]
fn c1541_dnp_write_matches_our_bam() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join("cbm-prog_fd-copy-test-disk.d2m")) else {
        eprintln!("skipping: fd-copy-test-disk.d2m not present");
        return;
    };
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("d2m_c1541_write");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let img = work.join("c.d2m");
    std::fs::write(&img, &bytes).unwrap();
    let prg = work.join("big.prg");
    std::fs::write(&prg, vec![0x42u8; 20_000]).unwrap(); // 79 blocks
    let ok = Command::new("c1541")
        .arg(&img)
        .arg("-write")
        .arg(&prg)
        .arg("big")
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "c1541 could not write");

    let disk = Disk::load(std::fs::read(&img).unwrap()).unwrap();
    let before = Disk::load(bytes).unwrap().blocks_free().unwrap();
    assert_eq!(disk.blocks_free().unwrap(), before - 79);
    let report = disk.validate().unwrap();
    assert!(report.is_clean(), "{report:?}");
}
