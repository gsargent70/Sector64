//! Archive-writer differential: build a T64 and a Lynx archive with this
//! crate's writers, then confirm `c1541`'s independent `tape`/`unlynx` readers
//! reconstruct the exact files. Skipped when `c1541` is absent.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use sector64::{write_lnx, write_t64, FileType, ImportFile};

fn c1541_available() -> bool {
    Command::new("c1541")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

fn files() -> Vec<ImportFile> {
    let prg = |name: &str, data: Vec<u8>| ImportFile { name: name.into(), file_type: FileType::Prg, data };
    vec![
        prg("HELLO", [0x01u8, 0x08].into_iter().chain(0..100).collect()),
        prg("BETA", vec![0x5A; 300]),           // multi-block
        prg("GAMMA", (0..6000u32).map(|x| x as u8).collect()),
    ]
}

#[test]
fn t64_written_by_us_reads_in_c1541() {
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pack_t64");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let files = files();
    let path = dir.join("out.t64");
    fs::write(&path, write_t64(&files)).unwrap();

    let disk = dir.join("dest.d64");
    assert!(Command::new("c1541")
        .args(["-format", "t64,01", "d64"])
        .arg(&disk)
        .arg("-tape")
        .arg(&path)
        .status()
        .unwrap()
        .success());

    for f in &files {
        let out = dir.join("r");
        let _ = fs::remove_file(&out);
        assert!(Command::new("c1541")
            .arg(&disk)
            .arg("-read")
            .arg(f.name.to_lowercase())
            .arg(&out)
            .status()
            .unwrap()
            .success());
        assert_eq!(fs::read(&out).unwrap(), f.data, "c1541 read of {} differs", f.name);
    }
    eprintln!("ok: write_t64 round-trips through c1541 tape ({} files)", files.len());
}

#[test]
fn lnx_written_by_us_reads_in_c1541() {
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pack_lnx");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let files = files();
    let path = dir.join("out.lnx");
    fs::write(&path, write_lnx(&files)).unwrap();

    let disk = dir.join("dest.d64");
    assert!(Command::new("c1541")
        .args(["-format", "lnx,01", "d64"])
        .arg(&disk)
        .arg("-unlynx")
        .arg(&path)
        .status()
        .unwrap()
        .success());

    for f in &files {
        let out = dir.join("r");
        let _ = fs::remove_file(&out);
        assert!(Command::new("c1541")
            .arg(&disk)
            .arg("-read")
            .arg(format!("{}*", f.name.to_lowercase()))
            .arg(&out)
            .status()
            .unwrap()
            .success());
        assert_eq!(fs::read(&out).unwrap(), f.data, "c1541 read of {} differs", f.name);
    }
    eprintln!("ok: write_lnx round-trips through c1541 unlynx ({} files)", files.len());
}
