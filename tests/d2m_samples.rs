//! CMD D2M (DNP native partition) BAM checks against a real sample.
//!
//! The native-partition BAM gives blocks-free and feeds validation. c1541
//! reports 5610 blocks free for this disk, which our decode must match. The
//! sample is not committed; point `SECTOR64_SAMPLES` at a directory holding it
//! (or use `~/Downloads/Sector64 Test Files`). Skips when absent.

use std::path::PathBuf;

use sector64::Disk;

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
fn dnp_bam_blocks_free_matches_c1541() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join("cbm-prog_fd-copy-test-disk.d2m")) else {
        eprintln!("skipping: fd-copy-test-disk.d2m not present");
        return;
    };
    let disk = Disk::load(bytes).expect("load d2m");

    // c1541 reports 5610 free for this native partition.
    assert_eq!(disk.blocks_free().unwrap(), 5610, "blocks free");

    // Validation runs and agrees on the free count. This particular disk has
    // two genuine used-but-free blocks (4/16, 4/17), confirmed by an
    // independent BAM walk — a real anomaly the check correctly surfaces.
    let report = disk.validate().expect("validate");
    assert_eq!(report.blocks_free, 5610);
    assert_eq!(report.cross_linked.len(), 0);
    assert!(report.used_but_free.contains(&(4, 16)));
    assert!(report.used_but_free.contains(&(4, 17)));
    eprintln!(
        "ok: d2m DNP BAM — {} free, {} used, {} used-but-free",
        report.blocks_free,
        report.blocks_used,
        report.used_but_free.len()
    );
}
