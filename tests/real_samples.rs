//! Differentials against real-world sample files, kept out of the repo.
//!
//! Point `SECTOR64_SAMPLES` at a directory of real files (or drop them in
//! `~/Downloads/Sector64 Test Files`). The test validates each one against
//! `c1541` and skips cleanly when the directory, the files, or `c1541` are
//! absent — so it never fails in CI or on other machines, but exercises
//! genuine data wherever the samples exist.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sector64::parse_zipcode;

fn samples_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SECTOR64_SAMPLES") {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join("Downloads/Sector64 Test Files");
    p.is_dir().then_some(p)
}

fn c1541_available() -> bool {
    Command::new("c1541")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

/// Collect zip-code sets in `dir`: each `1!<base>` with siblings `2!`/`3!`/`4!`.
fn zipcode_sets(dir: &Path) -> Vec<(String, [PathBuf; 4])> {
    let mut sets = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return sets;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(base) = name.strip_prefix("1!") else {
            continue;
        };
        let paths = [
            dir.join(format!("1!{base}")),
            dir.join(format!("2!{base}")),
            dir.join(format!("3!{base}")),
            dir.join(format!("4!{base}")),
        ];
        if paths.iter().all(|p| p.is_file()) {
            sets.push((base.to_string(), paths));
        }
    }
    sets
}

#[test]
fn real_zipcode_sets_match_c1541() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }

    let sets = zipcode_sets(&dir);
    if sets.is_empty() {
        eprintln!("no zip-code sets found in {}", dir.display());
        return;
    }

    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real_zipcode");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();

    for (base, paths) in sets {
        let data: Vec<Vec<u8>> = paths.iter().map(|p| fs::read(p).unwrap()).collect();

        // Our decoder.
        let mine = parse_zipcode([&data[0], &data[1], &data[2], &data[3]])
            .unwrap_or_else(|e| panic!("parse_zipcode failed on {base}: {e}"));

        // c1541 unzip: copy to a space-free base name first.
        for (i, d) in data.iter().enumerate() {
            fs::write(work.join(format!("{}!zc", i + 1)), d).unwrap();
        }
        let out = work.join("ref.d64");
        let _ = fs::remove_file(&out);
        assert!(
            Command::new("c1541")
                .arg("-unzip")
                .arg(&out)
                .arg(work.join("zc"))
                .status()
                .unwrap()
                .success(),
            "c1541 unzip failed on {base}"
        );
        assert_eq!(fs::read(&out).unwrap(), mine, "decode mismatch on {base}");
        eprintln!("ok: zip-code set {base} ({} bytes)", mine.len());
    }
}
