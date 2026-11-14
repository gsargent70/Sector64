//! `s64val` — validate a D64 image against its BAM.
//!
//! Usage: `s64val <image.d64>`
//!
//! Walks every file's block chain and compares it to the Block Availability
//! Map, reporting cross-linked blocks and blocks used but marked free. Exits
//! non-zero if the disk is inconsistent or structurally broken.

#[macro_use]
mod common;

use std::process::ExitCode;

use sector64::Disk;

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: s64val <image.d64>");
        return ExitCode::from(2);
    };

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("s64val: cannot read {}: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    let disk = match Disk::load(bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("s64val: {e}");
            return ExitCode::FAILURE;
        }
    };

    let report = match disk.validate() {
        Ok(r) => r,
        Err(e) => {
            // A structural fault (bad link, cycle) stops the walk.
            eprintln!("s64val: structural error: {e}");
            return ExitCode::FAILURE;
        }
    };

    outln!(
        "{} blocks used by files, {} free in BAM",
        report.blocks_used,
        report.blocks_free
    );
    for (t, s) in &report.cross_linked {
        outln!("  cross-linked block {t}/{s}");
    }
    for (t, s) in &report.used_but_free {
        outln!("  used but marked free in BAM: {t}/{s}");
    }

    if report.is_clean() {
        outln!("OK: no inconsistencies found.");
        ExitCode::SUCCESS
    } else {
        outln!(
            "FAIL: {} cross-linked, {} used-but-free.",
            report.cross_linked.len(),
            report.used_but_free.len()
        );
        ExitCode::FAILURE
    }
}
