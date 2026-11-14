//! `s64cvt <file> <outdir>` creates the output directory when it is missing
//! (it used to fail with "No such file or directory"). Uses the GEOSSYM.CVT
//! sample; skips cleanly when the samples directory is absent.

use std::path::PathBuf;
use std::process::Command;

fn samples_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SECTOR64_SAMPLES") {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join("Downloads/Sector64 Test Files");
    p.is_dir().then_some(p)
}

#[test]
fn creates_missing_output_dir() {
    let Some(cvt) = samples_dir()
        .map(|d| d.join("GEOSSYM.CVT"))
        .filter(|p| p.is_file())
    else {
        eprintln!("skipping: GEOSSYM.CVT not present");
        return;
    };
    let root = std::env::temp_dir().join(format!("s64cvt-outdir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let out = root.join("nested").join("out");

    let status = Command::new(env!("CARGO_BIN_EXE_s64cvt"))
        .arg(&cvt)
        .arg(&out)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "s64cvt failed: {status}");
    let written = std::fs::read_dir(&out).unwrap().count();
    assert!(
        written > 1,
        "expected the INFO block and records, got {written} files"
    );
    let _ = std::fs::remove_dir_all(&root);
}
