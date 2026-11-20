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

    // Validation runs, agrees on the free count, and finds the BAM consistent
    // with every file. (An earlier LSB-first reading of the DNP bitmap made
    // 4/16 and 4/17 look used-but-free; c1541 allocates around them.)
    let report = disk.validate().expect("validate");
    assert_eq!(report.blocks_free, 5610);
    assert!(report.is_clean(), "{report:?}");
    eprintln!(
        "ok: d2m DNP BAM — {} free, {} used, {} used-but-free",
        report.blocks_free,
        report.blocks_used,
        report.used_but_free.len()
    );
}

/// The disk map on both sample D2Ms: the 1581 one is a GEOS disk (VLIR files
/// and info blocks), the DNP one has a file whose first block is zeroed.
#[test]
fn d2m_block_maps() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    if let Ok(bytes) = std::fs::read(dir.join("cbm-prog_lharc-c-source-port.d2m")) {
        let disk = Disk::load(bytes).expect("load");
        let map = disk.block_map();
        assert!(map.problems.is_empty(), "{:?}", map.problems);
        assert_eq!(map.counts().4, 0, "no conflicts");
        // Every file, GEOS VLIR ones included, owns exactly its block count.
        for (i, e) in map.files.iter().enumerate() {
            assert_eq!(map.blocks_of(i).len(), e.blocks as usize, "{}", e.name);
        }
    }
    if let Ok(bytes) = std::fs::read(dir.join("cbm-prog_fd-copy-test-disk.d2m")) {
        let disk = Disk::load(bytes).expect("load");
        let map = disk.block_map();
        let (_, _, system, orphan, conflict) = map.counts();
        assert_eq!(system, 35, "track 1 blocks 0-34");
        assert_eq!(conflict, 0);
        // "128 BASIC ODER" lists 130 blocks from 1/64, but that block is
        // zeroed (end of chain), so the other 129 are allocated and unowned.
        assert_eq!(map.blocks_of(0), vec![(1, 64)]);
        assert_eq!(orphan, 129);
    }
}
