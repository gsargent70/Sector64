//! `s64cmp`, run as the real binary on disks and archives built here.

use std::path::PathBuf;
use std::process::Command;

use sector64::{write_t64, write_zipcode, Disk, FileType, ImportFile};

fn dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("cli_cmp")
        .join(tag);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_s64cmp"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code(),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    )
}

fn disk(files: &[(&str, FileType, &[u8])]) -> Disk {
    let mut d = Disk::format("CMP", "C1");
    for (name, t, data) in files {
        d.write_file(name, *t, data).unwrap();
    }
    d
}

fn save(dir: &std::path::Path, name: &str, bytes: &[u8]) -> String {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p.to_str().unwrap().to_string()
}

#[test]
fn lists_the_differences_between_two_disks() {
    use FileType::{Prg, Seq};
    let d = dir("disks");
    let left = disk(&[
        ("SAME", Prg, b"abc"),
        ("EDITED", Prg, b"abcd"),
        ("RETYPED", Prg, b"xy"),
        ("GONE", Prg, b"1"),
    ]);
    let right = disk(&[
        ("SAME", Prg, b"abc"),
        ("EDITED", Prg, b"abXdef"),
        ("RETYPED", Seq, b"xy"),
        ("NEW", Seq, b"22"),
    ]);
    let l = save(&d, "left.d64", left.as_bytes());
    let r = save(&d, "right.d64", right.as_bytes());

    let (code, out, err) = run(&[&l, &r]);
    assert_eq!((code, err.as_str()), (Some(1), ""));
    assert_eq!(
        out,
        "* \"EDITED\"                  4        6  differs from byte 2\n\
         * \"RETYPED\"                 2        2  type PRG / SEQ\n\
         < \"GONE\"                    1        -  only on the left\n\
         > \"NEW\"                     -        2  only on the right\n\
         2 differ, 1 only on the left, 1 only on the right, 1 the same\n"
    );
    let (code, all, _) = run(&[&l, "--all", &r]);
    assert_eq!(code, Some(1));
    assert!(all.starts_with("= \"SAME\" "), "{all}");
    assert_eq!(all.lines().count(), out.lines().count() + 1);

    let (code, out, _) = run(&[&l, &l]);
    assert_eq!(code, Some(0));
    assert_eq!(
        out,
        "Identical: the same 4 file(s) on both sides, byte for byte.\n"
    );
}

#[test]
fn compares_a_disk_with_an_archive_and_a_zipcode_set() {
    let d = dir("archives");
    let image = disk(&[("ONE", FileType::Prg, &[1, 8, 5])]);
    let d64 = save(&d, "disk.d64", image.as_bytes());
    let t64 = write_t64(&[ImportFile {
        name: "ONE".into(),
        file_type: FileType::Prg,
        data: vec![1, 8, 5],
    }]);
    let t64 = save(&d, "TAPE.T64", &t64);
    let zip: Vec<String> = write_zipcode(image.as_bytes())
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, part)| save(&d, &format!("{}!DISK", i + 1), part))
        .collect();
    assert_eq!(run(&[&d64, &t64]).0, Some(0));
    assert_eq!(run(&[&zip[2], &d64]).0, Some(0));
    // A part missing: can't compare.
    std::fs::remove_file(&zip[3]).unwrap();
    let (code, _, err) = run(&[&zip[0], &d64]);
    assert_eq!(code, Some(2));
    assert!(err.contains("4!DISK"), "{err}");
}

#[test]
fn trouble_exits_with_2() {
    let d = dir("trouble");
    let ok = save(&d, "ok.d64", disk(&[]).as_bytes());
    let junk = save(&d, "junk.txt", b"not a disk");
    let bad = save(&d, "bad.t64", b"not a tape");
    let missing = d.join("missing.d64").to_str().unwrap().to_string();
    for args in [
        vec![],
        vec![ok.as_str()],
        vec![&ok, &ok, &ok],
        vec!["--al", &ok, &ok],
        vec![&ok, &missing],
        vec![&ok, &junk],
        vec![&bad, &ok],
    ] {
        let (code, out, err) = run(&args);
        assert_eq!((code, out.as_str()), (Some(2), ""), "{args:?}");
        assert!(!err.is_empty(), "{args:?}");
    }
    assert_eq!(run(&[&ok, &ok]).0, Some(0));
}
