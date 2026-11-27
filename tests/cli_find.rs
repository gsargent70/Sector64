//! `s64find`, run as the real binary on a disk and an archive built here.

use std::path::{Path, PathBuf};
use std::process::Command;

use sector64::{write_lnx, Disk, FileType, ImportFile};

fn dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_find");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn find(args: &[&str]) -> (Option<i32>, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_s64find"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code(),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    )
}

/// A disk where "HELLO" is written in three case sets, the second copy
/// in the file's second block, and an LNX holding another copy.
fn inputs(at: &Path) -> (String, String) {
    let mut long = vec![0u8; 600];
    long[10..15].copy_from_slice(b"HELLO"); // unshifted PETSCII capitals
    long[300..305].copy_from_slice(&[0xC8, 0xC5, 0xCC, 0xCC, 0xCF]); // shifted
    long[500..505].copy_from_slice(b"hello"); // ASCII lower case
    long[520..523].copy_from_slice(&[0xA9, 0x00, 0x8D]); // LDA #0 / STA
    let mut d = Disk::format("FIND ME", "F1");
    d.write_file("GREETINGS", FileType::Prg, &long).unwrap();
    d.write_file("NOTHING", FileType::Seq, b"no match here")
        .unwrap();
    d.write_file("HELLO WORLD", FileType::Seq, b"named, empty inside")
        .unwrap();
    let disk = at.join("disk.d64");
    std::fs::write(&disk, d.as_bytes()).unwrap();
    let lnx = at.join("arch.lnx");
    let member = ImportFile {
        name: "INSIDE".into(),
        file_type: FileType::Prg,
        data: b"say hello".to_vec(),
    };
    std::fs::write(&lnx, write_lnx(&[member])).unwrap();
    (
        disk.to_str().unwrap().to_string(),
        lnx.to_str().unwrap().to_string(),
    )
}

#[test]
fn text_finds_every_case_set_names_and_the_block() {
    let at = dir();
    let (disk, lnx) = inputs(&at);
    let (code, out, err) = find(&["hello", &disk, &lnx]);
    assert_eq!(code, Some(0), "{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert!(
        lines[0].contains("\"GREETINGS\"")
            && lines[0].contains("3 matches at $000A $012C $01F4")
            && lines[0].contains("; first in track "),
        "{out}"
    );
    assert!(
        lines[1].contains("\"HELLO WORLD\"") && lines[1].ends_with(" name"),
        "{out}"
    );
    assert!(lines[2].starts_with(&format!("{lnx}: \"INSIDE\"")), "{out}");
    assert!(
        lines[2].contains("1 match at $0004") && !lines[2].contains("track"),
        "{out}"
    );

    // The block the first line names really holds the first match.
    let at: Vec<usize> = lines[0]
        .rsplit("; first in track ")
        .next()
        .unwrap()
        .split(|c: char| !c.is_ascii_digit())
        .filter(|n| !n.is_empty())
        .map(|n| n.parse().unwrap())
        .collect();
    let (t, s, b) = (at[0] as u8, at[1] as u8, at[2]);
    let d = Disk::load(std::fs::read(&disk).unwrap()).unwrap();
    assert_eq!(
        &d.block(t, s).unwrap()[b..b + 5],
        b"HELLO",
        "{t}/{s} byte {b}"
    );

    // -l lists just the files; --max caps the offsets shown.
    let (_, out, _) = find(&["-l", "hello", &disk]);
    assert_eq!(out.lines().count(), 2, "{out}");
    assert!(out.lines().all(|l| !l.contains("match")), "{out}");
    let (_, out, _) = find(&["--max", "1", "hello", &disk]);
    assert!(out.contains("3 matches at $000A (+2 more)"), "{out}");
}

#[test]
fn hex_with_wildcards_and_grep_like_exit_codes() {
    let at = dir();
    let (disk, _) = inputs(&at);
    let (code, out, _) = find(&["--hex", "A9 ?? 8D", &disk]);
    assert_eq!(code, Some(0));
    assert!(
        out.contains("\"GREETINGS\"") && out.contains("1 match at $0208"),
        "{out}"
    );
    assert!(
        !out.contains("HELLO WORLD"),
        "hex doesn't match names: {out}"
    );

    assert_eq!(find(&["xyzzy", &disk]).0, Some(1), "nothing found");
    let (code, _, err) = find(&["--hex", "GG", &disk]);
    assert_eq!(code, Some(2));
    assert!(err.contains("isn't a hex byte"), "{err}");
    let (code, _, err) = find(&["hello", &format!("{disk}.missing")]);
    assert_eq!(code, Some(2));
    assert!(err.contains("cannot read"), "{err}");
    assert_eq!(find(&["hello"]).0, Some(2), "usage");
}
