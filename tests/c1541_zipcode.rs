//! Zip-Code differential against VICE's `c1541 unzip`.
//!
//! Formats a disk with `c1541`, encodes it to Zip-Code with `write_zipcode`,
//! then decodes the four files two ways — `c1541 unzip` and `parse_zipcode` —
//! and checks both reproduce the original D64 exactly. Skipped when `c1541` is
//! absent.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use d64::{parse_zipcode, write_zipcode};

fn c1541_available() -> bool {
    Command::new("c1541")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

#[test]
fn zipcode_decodes_with_c1541_and_ourselves() {
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c1541_zipcode");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // A real, valid source disk built by c1541, with a couple of files.
    let a = dir.join("a.prg");
    let b = dir.join("b.prg");
    fs::write(&a, b"HELLO WORLD PROGRAM DATA").unwrap();
    fs::write(&b, (0..4000u32).map(|x| (x * 7) as u8).collect::<Vec<_>>()).unwrap();
    let src = dir.join("src.d64");
    assert!(Command::new("c1541")
        .args(["-format", "zipsrc,zc", "d64"])
        .arg(&src)
        .arg("-write")
        .arg(&a)
        .arg("alpha")
        .arg("-write")
        .arg(&b)
        .arg("beta")
        .status()
        .unwrap()
        .success());
    let source = fs::read(&src).unwrap();

    // Encode to Zip-Code and write the four host files "1!zc".."4!zc".
    let files = write_zipcode(&source).unwrap();
    for (i, data) in files.iter().enumerate() {
        fs::write(dir.join(format!("{}!zc", i + 1)), data).unwrap();
    }

    // c1541 unzip must reproduce the source exactly.
    let out = dir.join("out.d64");
    assert!(Command::new("c1541")
        .arg("-unzip")
        .arg(&out)
        .arg(dir.join("zc"))
        .status()
        .unwrap()
        .success());
    assert_eq!(fs::read(&out).unwrap(), source, "c1541 unzip != source");

    // And our own decoder reproduces it too.
    let mine = parse_zipcode([&files[0], &files[1], &files[2], &files[3]]).unwrap();
    assert_eq!(mine, source, "parse_zipcode != source");
}
