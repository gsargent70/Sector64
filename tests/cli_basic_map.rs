//! `s64extract --list` (BASIC listings) and `s64val --map` (text disk maps),
//! run as the real binaries on disks built here.

use std::path::PathBuf;
use std::process::{Command, Output};

use sector64::{Disk, FileType};

/// `10 PRINT "HI"` as a C64 BASIC V2 program at $0801, then 2 bytes of code.
const HELLO: &[u8] = &[
    0x01, 0x08, 0x0D, 0x08, 0x0A, 0x00, 0x99, 0x20, 0x22, 0x48, 0x49, 0x22, 0x00, 0x00, 0x00, 0xEA,
    0x60,
];

fn image(tag: &str, disk: &Disk) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_basic_map");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{tag}.d64"));
    std::fs::write(&p, disk.as_bytes()).unwrap();
    p
}

fn run(bin: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let Output {
        status,
        stdout,
        stderr,
    } = Command::new(bin).args(args).output().unwrap();
    (
        status.code(),
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

fn test_disk() -> Disk {
    let mut disk = Disk::format("cli test", "ct");
    disk.write_file("HELLO", FileType::Prg, HELLO).unwrap();
    disk.write_file("CODE", FileType::Prg, &[0x00, 0xC0, 0xA9, 0x00, 0x60])
        .unwrap();
    disk
}

#[test]
fn extract_lists_basic_programs() {
    let img = image("list", &test_disk());
    let img = img.to_str().unwrap();
    let bin = env!("CARGO_BIN_EXE_s64extract");

    let (code, out, err) = run(bin, &["--list", img, "hello"]);
    assert_eq!((code, out.as_str()), (Some(0), "10 PRINT \"HI\"\n"));
    assert_eq!(
        err,
        "s64extract: listed \"HELLO\" as BASIC V2; 2 bytes of machine code or data follow the program\n"
    );
    // A dialect by hand, with the option after the arguments.
    let (code, _, err) = run(bin, &[img, "HELLO", "--list=7.0"]);
    assert_eq!(code, Some(0));
    assert!(err.contains("as BASIC 7.0"), "{err}");

    let (code, out, err) = run(bin, &["--list", img, "code"]);
    assert_eq!((code, out.as_str()), (Some(1), ""));
    assert_eq!(err, "s64extract: \"CODE\" is not a BASIC program\n");

    // Without --list the raw bytes come out as before.
    let raw = Command::new(bin).args([img, "hello"]).output().unwrap();
    assert_eq!(raw.stdout, HELLO);

    assert_eq!(run(bin, &["--list=c65", img, "hello"]).0, Some(2));
    assert_eq!(run(bin, &["--lsit", img, "hello"]).0, Some(2));
    assert_eq!(run(bin, &["--list", img]).0, Some(2));
}

#[test]
fn val_draws_a_map_and_fails_on_conflicts() {
    let bin = env!("CARGO_BIN_EXE_s64val");
    let disk = test_disk();
    let img = image("map", &disk);
    let (code, out, _) = run(bin, &["--map", img.to_str().unwrap()]);
    assert_eq!(code, Some(0), "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "0 \"CLI TEST\" CT 2A");
    assert_eq!(lines[1], "       0         1         2");
    assert!(lines.contains(&"  A  \"HELLO\"            prg  1 block"));
    assert!(lines.contains(&"  B  \"CODE\"             prg  1 block"));
    assert!(
        out.ends_with("0 allocated but unused, 0 in conflict\n"),
        "{out}"
    );
    // The plain validation report is unchanged.
    let (code, out, _) = run(bin, &[img.to_str().unwrap()]);
    assert_eq!(code, Some(0));
    assert!(out.ends_with("OK: no inconsistencies found.\n"));

    // Free HELLO's block in the BAM (18/0 from byte 4, 4 bytes per track):
    // a conflict.
    let e = disk.find_entry("HELLO").unwrap().unwrap();
    let mut bytes = disk.as_bytes().to_vec();
    let bam = 17 * 21 * 256 + 4 + 4 * (e.first_track as usize - 1);
    bytes[bam] += 1;
    bytes[bam + 1 + e.first_sector as usize / 8] |= 1 << (e.first_sector % 8);
    let broken = image("conflict", &Disk::load(bytes).unwrap());
    let (code, out, _) = run(bin, &["--map", broken.to_str().unwrap()]);
    assert_eq!(code, Some(1));
    assert!(
        out.contains('!') && out.ends_with("1 in conflict\n"),
        "{out}"
    );

    assert_eq!(run(bin, &["--mapp", img.to_str().unwrap()]).0, Some(2));
    assert_eq!(run(bin, &["--map"]).0, Some(2));
}
