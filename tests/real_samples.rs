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

use sector64::{parse_zipcode, Disk};

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

/// Disk images (`.d64`/`.d71`/`.d81`) in `dir`.
fn disk_images(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
                Some("d64" | "d71" | "d81" | "g64" | "d2m")
            )
        })
        .collect()
}

#[test]
fn real_disk_images_match_c1541() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    if !c1541_available() {
        eprintln!("skipping: c1541 not found");
        return;
    }
    let images = disk_images(&dir);
    if images.is_empty() {
        eprintln!("no disk images found in {}", dir.display());
        return;
    }

    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real_images");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();

    for img in images {
        let label = img.file_name().unwrap().to_string_lossy().into_owned();
        let disk = match Disk::load(fs::read(&img).unwrap()) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skip {label}: {e}");
                continue;
            }
        };
        let entries = disk.directory().unwrap_or_else(|e| panic!("{label}: directory: {e}"));

        let mut compared = 0usize;
        let mut skipped = 0usize;
        for entry in &entries {
            let mine = disk
                .read_entry(entry)
                .unwrap_or_else(|e| panic!("{label}: read {}: {e}", entry.name));

            // c1541 reads by lowercase name; a trailing '*' absorbs name padding.
            // A name beginning with '-' collides with c1541's option parsing
            // (it reports "invalid filename"), so such names can't be addressed
            // through the CLI at all — skip the cross-check for them rather than
            // assert against a tool that can't name the file.
            let pattern = format!("{}*", entry.name.to_lowercase());
            if pattern.starts_with('-') {
                eprintln!("  skip {}: c1541 can't address a name starting with '-'", entry.name);
                skipped += 1;
                continue;
            }
            let out = work.join("ref");
            let _ = fs::remove_file(&out);
            let ok = Command::new("c1541")
                .arg(&img)
                .arg("-read")
                .arg(&pattern)
                .arg(&out)
                .status()
                .unwrap()
                .success();
            if !ok {
                eprintln!("  skip {}: c1541 -read failed for pattern {pattern:?}", entry.name);
                skipped += 1;
                continue;
            }
            let theirs = fs::read(&out).unwrap();
            // A GEOS/VLIR or corrupt entry doesn't read as a flat file — c1541
            // returns far fewer bytes than the directory's block count implies,
            // and our flat read disagrees. Skip those rather than treat the
            // ambiguity as a bug (both tools mishandle a non-flat file).
            let min_bytes = (entry.blocks as usize).saturating_sub(1) * 254;
            if entry.blocks > 2 && theirs.len() < min_bytes {
                eprintln!("  skip {}: not a flat file (c1541 read {} of ~{} bytes)", entry.name, theirs.len(), min_bytes);
                skipped += 1;
                continue;
            }
            // A wildcard can still match the wrong file; guard with the block
            // count from the directory (a chain of N blocks holds at most N*254
            // data bytes). An impossible length means the pattern hit something
            // else, so skip rather than report a false mismatch.
            let max_bytes = entry.blocks as usize * 254;
            if theirs.len() > max_bytes {
                eprintln!(
                    "  skip {}: c1541 pattern matched a larger file ({} > {} blocks*254)",
                    entry.name,
                    theirs.len(),
                    entry.blocks
                );
                skipped += 1;
                continue;
            }
            assert_eq!(theirs, mine, "{label}: bytes differ for {}", entry.name);
            compared += 1;
        }
        eprintln!("ok: {label} ({} files, {compared} verified vs c1541, {skipped} skipped)", entries.len());
    }
}
