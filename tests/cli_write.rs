//! `s64write`, run as the real binary: making and editing a disk, its
//! directory included, with VICE's `c1541` (when installed) reading the
//! result.

use std::path::{Path, PathBuf};
use std::process::Command;

use sector64::Disk;

fn dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("cli_write")
        .join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `s64write` with these arguments; the exit code and what it printed.
fn w(args: &[&str]) -> (Option<i32>, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_s64write"))
        .args(args)
        .output()
        .unwrap();
    let mut text = String::from_utf8(o.stdout).unwrap();
    text += &String::from_utf8(o.stderr).unwrap();
    (o.status.code(), text)
}

fn ok(args: &[&str]) -> String {
    let (code, text) = w(args);
    assert_eq!(code, Some(0), "s64write {args:?}: {text}");
    text
}

fn listing(img: &Path) -> Vec<(String, bool)> {
    let d = Disk::load(std::fs::read(img).unwrap()).unwrap();
    d.directory()
        .unwrap()
        .into_iter()
        .map(|e| (e.name.to_lowercase(), e.locked))
        .collect()
}

fn names(img: &Path) -> Vec<String> {
    listing(img).into_iter().map(|e| e.0).collect()
}

fn setup(tag: &str) -> (PathBuf, String) {
    let at = dir(tag);
    let host = at.join("data.bin");
    std::fs::write(&host, [1, 8, 0, 0]).unwrap();
    let img = at.join("disk.d64");
    let i = img.to_str().unwrap().to_string();
    ok(&["format", &i, "work disk", "01"]);
    for n in ["one", "two", "three"] {
        ok(&["add", &i, n, host.to_str().unwrap()]);
    }
    (img, i)
}

#[test]
fn rename_delete_and_the_disk_name() {
    let (img, i) = setup("basic");
    // Names list in capitals (lower-case ASCII is stored as PETSCII capitals).
    assert!(ok(&["rename", &i, "two", "second"]).contains("renamed \"TWO\" to \"second\""));
    assert!(ok(&["delete", &i, "#1"]).contains("deleted \"ONE\""));
    assert_eq!(names(&img), ["second", "three"]);
    ok(&["name", &i, "new name", "n2"]);
    let d = Disk::load(std::fs::read(&img).unwrap()).unwrap();
    let h = d.header().unwrap();
    assert_eq!((h.name.as_str(), h.id.as_str()), ("NEW NAME", "N2"));
}

#[test]
fn lock_move_and_separators() {
    let (img, i) = setup("dir");
    ok(&["lock", &i, "two"]);
    assert_eq!(listing(&img)[1], ("two".into(), true));
    ok(&["move", &i, "three", "1"]);
    assert_eq!(names(&img), ["three", "one", "two"]);
    ok(&["move", &i, "#1", "last"]);
    assert_eq!(names(&img), ["one", "two", "three"]);
    assert!(listing(&img)[1].1, "the lock moved with its file");

    // At the end by default, or after an entry.
    assert!(ok(&["separator", &i]).contains("#4"));
    assert!(ok(&["separator", &i, "one"]).contains("#2"));
    let sep = "----------------".to_string();
    assert_eq!(names(&img), ["one", &sep, "two", "three", &sep]);

    // Separators share a name: #N picks one; a name finds the first.
    ok(&["rename", &i, "#5", "== end =="]);
    assert_eq!(names(&img)[4], "== end ==");
    ok(&["delete", &i, &sep]);
    assert_eq!(names(&img), ["one", "two", "three", "== end =="]);
    ok(&["unlock", &i, "two"]);
    assert!(listing(&img).iter().all(|e| !e.1));
    let d = Disk::load(std::fs::read(&img).unwrap()).unwrap();
    assert!(d.validate().unwrap().is_clean());
}

#[test]
fn bad_entries_and_positions_are_refused() {
    let (img, i) = setup("bad");
    let before = std::fs::read(&img).unwrap();
    for (args, says) in [
        (vec!["lock", &i, "nope"], "no file named \"nope\""),
        (vec!["move", &i, "one", "4"], "the position must be 1 to 3"),
        (vec!["move", &i, "one", "0"], "the position must be 1 to 3"),
        (
            vec!["delete", &i, "#4"],
            "there's no entry #4: the directory has 3",
        ),
        (vec!["separator", &i, "#0"], "there's no entry #0"),
    ] {
        let (code, text) = w(&args);
        assert_eq!(code, Some(1), "{args:?}: {text}");
        assert!(text.contains(says), "{args:?}: {text}");
    }
    assert_eq!(std::fs::read(&img).unwrap(), before, "nothing was written");
    assert_eq!(w(&["move", &i, "one"]).0, Some(2), "usage");
}

#[test]
fn c1541_reads_the_edited_directory() {
    if Command::new("c1541").arg("-help").output().is_err() {
        eprintln!("skipping: c1541 (VICE) not found on PATH");
        return;
    }
    let (img, i) = setup("c1541");
    ok(&["name", &i, "edited", "e1"]);
    ok(&["lock", &i, "one"]);
    ok(&["move", &i, "three", "1"]);
    ok(&["separator", &i, "three"]);
    let out = Command::new("c1541")
        .arg(&img)
        .arg("-dir")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    let lines: Vec<&str> = text.lines().filter(|l| l.contains('"')).collect();
    assert!(
        lines[0].contains("\"edited ") && lines[0].contains("e1"),
        "{text}"
    );
    let order: Vec<&str> = lines[1..]
        .iter()
        .filter_map(|l| l.split('"').nth(1))
        .collect();
    assert_eq!(order, ["three", "----------------", "one", "two"], "{text}");
    assert!(lines[3].contains("prg<"), "{text}");
    let copy = img.with_file_name("validated.d64");
    std::fs::copy(&img, &copy).unwrap();
    Command::new("c1541")
        .arg(&copy)
        .arg("-validate")
        .output()
        .unwrap();
    assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(&img).unwrap());
}
