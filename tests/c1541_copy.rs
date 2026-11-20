//! Copying between disks, against VICE's `c1541`: c1541 builds a D64, this
//! crate copies its files onto a fresh D71 and D81, and c1541 must read every
//! copy back byte for byte, with the same names and types. Skipped when
//! `c1541` is absent.

use std::path::{Path, PathBuf};
use std::process::Command;

use sector64::{Disk, Error};

fn c1541(args: &[&str]) -> String {
    let out = Command::new("c1541")
        .args(args)
        .output()
        .expect("run c1541");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn c1541_available() -> bool {
    Command::new("c1541")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn copies_read_back_in_c1541() {
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c1541_copy");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();

    // Source files: a multi-block PRG, a one-block SEQ and an empty USR.
    let files: [(&str, &str, Vec<u8>); 3] = [
        (
            "game",
            "p",
            (0..3000u32).map(|i| (i * 13 + 1) as u8).collect(),
        ),
        ("notes", "s", b"HELLO FROM SEQ\r".to_vec()),
        ("empty", "u", Vec::new()),
    ];
    let src_img = work.join("src.d64");
    let mut args: Vec<String> = vec![
        "-format".into(),
        "source,s1".into(),
        "d64".into(),
        path(&src_img).into(),
    ];
    for (name, t, data) in &files {
        let host = work.join(name);
        std::fs::write(&host, data).unwrap();
        args.extend(["-write".into(), path(&host).into(), format!("{name},{t}")]);
    }
    c1541(&args.iter().map(String::as_str).collect::<Vec<_>>());
    let src = Disk::load(std::fs::read(&src_img).unwrap()).unwrap();
    let entries = src.directory().unwrap();
    assert_eq!(entries.len(), 3, "c1541 wrote the source disk");

    for (fmt, mut dst) in [
        ("d71", Disk::format_d71("target", "t1")),
        ("d81", Disk::format_d81("target", "t1")),
    ] {
        for e in &entries {
            dst.copy_file_from(&src, e).unwrap();
        }
        assert!(matches!(
            dst.copy_file_from(&src, &entries[0]),
            Err(Error::FileExists)
        ));
        assert!(dst.validate().unwrap().is_clean(), "{fmt} validates");
        let img = work.join(format!("dst.{fmt}"));
        std::fs::write(&img, dst.as_bytes()).unwrap();

        let listing = c1541(&["-attach", path(&img), "-list"]).to_lowercase();
        for ((name, t, data), e) in files.iter().zip(&entries) {
            let want = format!("\"{name}\"");
            let line = listing
                .lines()
                .find(|l| l.contains(&want))
                .unwrap_or_else(|| panic!("{fmt}: {name} not listed:\n{listing}"));
            let ty = match *t {
                "p" => "prg",
                "s" => "seq",
                _ => "usr",
            };
            assert!(line.contains(ty), "{fmt}: {line}");
            assert!(
                line.trim_start().starts_with(&e.blocks.to_string()),
                "{fmt}: {line}"
            );
            // c1541 must read the copy exactly as it reads the original.
            // An empty file is a degenerate one-block file: c1541 reads the
            // whole block, which holds leftover bytes in its own image and
            // zeros in ours, so only the length can match.
            let read = |img: &Path, tag: &str| {
                let out = work.join(format!("{fmt}-{tag}-{name}"));
                c1541(&[
                    "-attach",
                    path(img),
                    "-read",
                    &format!("{name},{t}"),
                    path(&out),
                ]);
                std::fs::read(&out).unwrap_or_else(|_| panic!("{fmt}: c1541 read {name}"))
            };
            let (got, orig) = (read(&img, "copy"), read(&src_img, "orig"));
            if data.is_empty() {
                assert_eq!(got.len(), orig.len(), "{fmt}: {name}");
            } else {
                assert_eq!(got, orig, "{fmt}: {name}");
                assert_eq!(&got, data, "{fmt}: {name}");
            }
        }
    }
}
