//! `s64basic`, run as the real binary, and back through `s64extract --list`.

use std::process::Command;

use sector64::{detokenize_basic, Disk, FileType};

#[test]
fn text_to_prg_and_back() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_basic");
    std::fs::create_dir_all(&dir).unwrap();
    let (txt, prg) = (dir.join("p.txt"), dir.join("p.prg"));
    let _ = std::fs::remove_file(&prg);
    let run = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_s64basic"))
            .args(args)
            .arg(&txt)
            .arg(&prg)
            .output()
            .unwrap();
        (
            o.status.code(),
            String::from_utf8(o.stdout).unwrap(),
            String::from_utf8(o.stderr).unwrap(),
        )
    };
    let text = "10 PRINT \"{CLR}HI\"\n20 GOTO 10\n";
    std::fs::write(&txt, text).unwrap();
    let (code, out, err) = run(&[]);
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert!(
        out.ends_with("(2 lines, 25 bytes, BASIC V2, loads at $0801)\n"),
        "{out}"
    );
    let bytes = std::fs::read(&prg).unwrap();
    assert_eq!(detokenize_basic(&bytes).unwrap().text, text);

    // Onto a disk, listed by the other tool.
    let mut disk = Disk::format("B", "B1");
    disk.write_file("P", FileType::Prg, &bytes).unwrap();
    let img = dir.join("p.d64");
    std::fs::write(&img, disk.as_bytes()).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_s64extract"))
        .args(["--list", img.to_str().unwrap(), "P"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(o.stdout).unwrap(), text);

    let (code, out, _) = run(&["--dialect=7.0"]);
    assert_eq!(code, Some(0));
    assert!(out.contains("BASIC 7.0, loads at $1C01"), "{out}");
    let (_, out, _) = run(&["--load=$1001"]);
    assert!(out.contains("loads at $1001"), "{out}");
    for bad in ["--dialect=c65", "--load=xyz", "--lod=1"] {
        assert_eq!(run(&[bad]).0, Some(2), "{bad}");
    }

    // Every problem is listed, and the output is left alone.
    let before = std::fs::read(&prg).unwrap();
    std::fs::write(&txt, "10 PRINT\nOOPS\n20 {NOPE}\n").unwrap();
    let (code, out, err) = run(&[]);
    assert_eq!((code, out.as_str()), (Some(1), ""));
    assert_eq!(err.lines().count(), 2, "{err}");
    assert!(err.contains("text line 2: there's no line number"));
    assert_eq!(std::fs::read(&prg).unwrap(), before);
    // Bad UTF-8 is a problem on its line, not a crash.
    std::fs::write(&txt, b"10 PRINT \"\xff\"\n").unwrap();
    assert_eq!(run(&[]).0, Some(1));
}
