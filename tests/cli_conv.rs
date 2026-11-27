//! `s64conv`, run as the real binary, with `s64cmp` and (when installed)
//! VICE's `c1541` checking what it wrote.

use std::path::{Path, PathBuf};
use std::process::Command;

use sector64::{Disk, FileType, SEPARATOR};

fn dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("cli_conv")
        .join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(exe: &str, args: &[&Path]) -> (Option<i32>, String, String) {
    let o = Command::new(exe).args(args).output().unwrap();
    (
        o.status.code(),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    )
}

fn conv(args: &[&Path]) -> (Option<i32>, String, String) {
    run(env!("CARGO_BIN_EXE_s64conv"), args)
}

fn source(at: &Path) -> PathBuf {
    let mut d = Disk::format("CONVERT ME", "C1");
    for i in 0..12 {
        let data: Vec<u8> = (0..700 * (i + 1)).map(|b| (b * 7) as u8).collect();
        d.write_file(&format!("FILE{i}"), FileType::Prg, &data)
            .unwrap();
    }
    d.write_file("NOTES", FileType::Seq, b"some text").unwrap();
    d.add_separator(SEPARATOR, Some(0)).unwrap();
    d.set_locked(2, true).unwrap(); // FILE1, after the separator
    let p = at.join("source.d64");
    std::fs::write(&p, d.as_bytes()).unwrap();
    p
}

#[test]
fn a_d64_becomes_a_d81_and_a_d71_with_everything_kept() {
    let at = dir("disk");
    let src = source(&at);
    for ext in ["d81", "d71"] {
        let out = at.join(format!("out.{ext}"));
        let (code, stdout, stderr) = conv(&[&src, &out]);
        assert_eq!(code, Some(0), "{stderr}");
        assert!(stdout.ends_with(": 14 files\n"), "{stdout}");
        let (code, stdout, _) = run(env!("CARGO_BIN_EXE_s64cmp"), &[&src, &out]);
        assert_eq!(code, Some(0), "{stdout}");
        let d = Disk::load(std::fs::read(&out).unwrap()).unwrap();
        let h = d.header().unwrap();
        assert_eq!((h.name.as_str(), h.id.as_str()), ("CONVERT ME", "C1"));
        assert!(d.directory().unwrap()[2].locked);
        assert!(d.validate().unwrap().is_clean());
    }
}

#[test]
fn archives_go_both_ways_and_say_what_changed() {
    let at = dir("archive");
    let src = source(&at);
    let t64 = at.join("tape.t64");
    let (code, _, stderr) = conv(&[&src, &t64]);
    assert_eq!(code, Some(0), "notes aren't failures: {stderr}");
    assert!(
        stderr.contains("\"----------------\" a separator line, left out"),
        "{stderr}"
    );
    assert!(
        stderr.contains("\"NOTES\" stored as PRG, not SEQ"),
        "{stderr}"
    );

    // An archive back to a disk is named after the archive.
    let back = at.join("back.d64");
    let (code, stdout, stderr) = conv(&[&t64, &back]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.ends_with(": 13 files\n"), "{stdout}");
    let d = Disk::load(std::fs::read(&back).unwrap()).unwrap();
    assert_eq!(d.header().unwrap().name, "TAPE");

    // LNX keeps the types.
    let lnx = at.join("kept.lnx");
    let (code, _, stderr) = conv(&[&src, &lnx]);
    assert_eq!(code, Some(0));
    assert!(!stderr.contains("stored as PRG"), "{stderr}");
}

#[test]
fn what_does_not_fit_makes_status_1_and_bad_requests_status_2() {
    let at = dir("status");
    // A full D81's worth of files doesn't fit on a D64.
    let mut big = Disk::format_d81("BIG", "B1");
    for i in 0..4 {
        big.write_file(&format!("PART{i}"), FileType::Prg, &vec![0x55; 180_000])
            .unwrap();
    }
    let src = at.join("big.d81");
    std::fs::write(&src, big.as_bytes()).unwrap();
    let out = at.join("small.d64");
    let (code, stdout, stderr) = conv(&[&src, &out]);
    assert_eq!(code, Some(1), "{stdout}{stderr}");
    assert!(stdout.contains("left out"), "{stdout}");
    assert!(stderr.contains("left out \"PART1\""), "{stderr}");

    // It won't replace a file unless asked to.
    let (code, _, stderr) = conv(&[&src, &out]);
    assert_eq!(code, Some(2));
    assert!(stderr.contains("already exists"), "{stderr}");
    let (code, _, _) = conv(&[Path::new("--force"), &src, &out]);
    assert_eq!(code, Some(1));

    let (code, _, stderr) = conv(&[&src, &at.join("out.zip")]);
    assert_eq!(code, Some(2));
    assert!(stderr.contains("must end in"), "{stderr}");
    let (code, _, _) = conv(&[&at.join("missing.d64"), &at.join("x.d64")]);
    assert_eq!(code, Some(2));
}

#[test]
fn c1541_reads_the_converted_disk() {
    let Ok(o) = Command::new("c1541").arg("-help").output() else {
        eprintln!("skipping: c1541 (VICE) not found on PATH");
        return;
    };
    let _ = o;
    let at = dir("c1541");
    let src = source(&at);
    let out = at.join("out.d81");
    assert_eq!(conv(&[&src, &out]).0, Some(0));
    let listing = Command::new("c1541")
        .arg(&out)
        .arg("-dir")
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&listing.stdout).to_lowercase();
    let names: Vec<&str> = listing
        .lines()
        .filter_map(|l| l.split('"').nth(1))
        .collect();
    assert_eq!(names.len(), 15, "the header and 14 entries: {listing}");
    assert_eq!(
        names[1..4],
        ["file0", "----------------", "file1"],
        "{listing}"
    );
    assert!(listing.contains("prg<"), "{listing}");
}
