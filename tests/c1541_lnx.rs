//! LNX import differential against VICE's `c1541 unlynx`.
//!
//! Builds a Lynx archive, dissolves it two ways — this crate's `parse_lnx` and
//! `c1541`'s independent `unlynx` (which writes the files onto a disk) — and
//! checks the file bytes match. Skipped when `c1541` is absent.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use sector64::parse_lnx;

fn c1541_available() -> bool {
    Command::new("c1541")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

/// Build a minimal Lynx archive (self-made BASIC stub + text directory).
fn build_lnx(files: &[(&str, char, Vec<u8>)]) -> Vec<u8> {
    let blocks = |l: usize| l.div_ceil(254).max(1);
    let last = |l: usize| l - (blocks(l) - 1) * 254 + 1;
    let stub: &[u8] = &[0x01, 0x08, 0x09, 0x08, 0x00, 0x00, 0x8F, 0x00, 0x00, 0x00];
    let header = |db: usize| -> Vec<u8> {
        let mut s = stub.to_vec();
        s.extend_from_slice(format!("\r {db}  *LYNX TEST\r{}\r", files.len()).as_bytes());
        for (name, ty, data) in files {
            let mut nm = name.as_bytes().to_vec();
            nm.resize(16, b' ');
            s.extend_from_slice(&nm);
            s.extend_from_slice(
                format!("\r{}\r{}\r{}\r", blocks(data.len()), ty, last(data.len())).as_bytes(),
            );
        }
        s
    };
    let mut db = 1;
    loop {
        let need = blocks(header(db).len());
        if need == db {
            break;
        }
        db = need;
    }
    let mut out = header(db);
    out.resize(db * 254, 0);
    for (_, _, data) in files {
        let start = out.len();
        out.extend_from_slice(data);
        out.resize(start + blocks(data.len()) * 254, 0);
    }
    out
}

#[test]
fn lnx_parse_matches_c1541_unlynx() {
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c1541_lnx");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // All PRG so c1541 -read reconstructs them identically. Varied sizes,
    // including a multi-block file.
    let files: Vec<(&str, char, Vec<u8>)> = vec![
        ("ALPHA", 'P', vec![0x01, 0x08].into_iter().chain(0..100).collect()),
        ("BETA", 'P', vec![0x5A; 300]),
        ("GAMMA", 'P', (0..6000u32).map(|x| x as u8).collect()),
    ];
    let lnx = build_lnx(&files);
    let lnx_path = dir.join("sample.lnx");
    fs::write(&lnx_path, &lnx).unwrap();

    let mine = parse_lnx(&lnx).unwrap();
    assert_eq!(mine.len(), files.len());

    // c1541 dissolves the same archive onto a disk.
    let disk = dir.join("dest.d64");
    assert!(Command::new("c1541")
        .args(["-format", "lnxdest,01", "d64"])
        .arg(&disk)
        .arg("-unlynx")
        .arg(&lnx_path)
        .status()
        .unwrap()
        .success());

    for (f, (name, _, data)) in mine.iter().zip(&files) {
        assert_eq!(&f.data, data, "our parse of {name}");
        let out = dir.join(format!("{name}.ref"));
        // unlynx keeps the 16-char space-padded name; match it with a wildcard.
        assert!(Command::new("c1541")
            .arg(&disk)
            .arg("-read")
            .arg(format!("{}*", name.to_lowercase()))
            .arg(&out)
            .status()
            .unwrap()
            .success());
        assert_eq!(fs::read(&out).unwrap(), f.data, "c1541 vs ours for {name}");
    }
}
