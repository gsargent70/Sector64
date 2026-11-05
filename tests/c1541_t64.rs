//! T64 import differential against VICE's `c1541`.
//!
//! Builds a T64, extracts it two ways — with this crate's `parse_t64` and with
//! `c1541`'s independent `tape` reader (which writes the files onto a disk) —
//! and checks they produce identical PRG bytes. Skipped when `c1541` is absent.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use sector64::parse_t64;

fn c1541_available() -> bool {
    Command::new("c1541")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

fn build_t64(entries: &[(&str, u16, Vec<u8>)]) -> Vec<u8> {
    let n = entries.len();
    let mut hdr = b"C64S tape image file\0".to_vec();
    hdr.resize(0x20, 0);
    hdr.extend_from_slice(&0x0101u16.to_le_bytes());
    hdr.extend_from_slice(&(n as u16).to_le_bytes());
    hdr.extend_from_slice(&(n as u16).to_le_bytes());
    hdr.extend_from_slice(&0u16.to_le_bytes());
    let mut name = b"TEST TAPE".to_vec();
    name.resize(24, b' ');
    hdr.extend_from_slice(&name);

    let data_off = 0x40 + 0x20 * n;
    let mut dir = Vec::new();
    let mut blob = Vec::new();
    let mut off = data_off;
    for (nm, load, data) in entries {
        dir.push(1u8);
        dir.push(0x82);
        dir.extend_from_slice(&load.to_le_bytes());
        dir.extend_from_slice(&(load + data.len() as u16).to_le_bytes());
        dir.extend_from_slice(&0u16.to_le_bytes());
        dir.extend_from_slice(&(off as u32).to_le_bytes());
        dir.extend_from_slice(&0u32.to_le_bytes());
        let mut f = nm.as_bytes().to_vec();
        f.resize(16, b' ');
        dir.extend_from_slice(&f);
        blob.extend_from_slice(data);
        off += data.len();
    }
    [hdr, dir, blob].concat()
}

#[test]
fn t64_parse_matches_c1541_tape() {
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c1541_t64");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let entries: Vec<(&str, u16, Vec<u8>)> = vec![
        ("HELLO", 0x0801, (0..200u32).map(|x| x as u8).collect()),
        ("WORLD", 0x1000, vec![0xAA; 50]),
        ("SOLO", 0x2000, vec![0x42; 3000]),
    ];
    let t64 = build_t64(&entries);
    let t64_path = dir.join("demo.t64");
    fs::write(&t64_path, &t64).unwrap();

    // Our parse.
    let mine = parse_t64(&t64).unwrap();
    assert_eq!(mine.len(), entries.len());

    // c1541 extracts the same T64 onto a disk, independently.
    let disk = dir.join("dest.d64");
    assert!(Command::new("c1541")
        .args(["-format", "t64dest,01", "d64"])
        .arg(&disk)
        .arg("-tape")
        .arg(&t64_path)
        .status()
        .unwrap()
        .success());

    for (f, (name, load, data)) in mine.iter().zip(&entries) {
        // Our reconstruction is load address + data.
        let expected: Vec<u8> = [&load.to_le_bytes()[..], data].concat();
        assert_eq!(f.data, expected, "our parse of {name}");

        let out = dir.join(format!("{name}.ref"));
        // c1541 maps typed lowercase ASCII to uppercase PETSCII (the stored name).
        assert!(Command::new("c1541")
            .arg(&disk)
            .arg("-read")
            .arg(name.to_lowercase())
            .arg(&out)
            .status()
            .unwrap()
            .success());
        assert_eq!(fs::read(&out).unwrap(), f.data, "c1541 vs ours for {name}");
    }
}
