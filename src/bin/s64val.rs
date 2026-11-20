//! `s64val` — validate a D64 image against its BAM.
//!
//! Usage: `s64val [--map] <image>`
//!
//! Walks every file's block chain and compares it to the Block Availability
//! Map, reporting cross-linked blocks and blocks used but marked free. Exits
//! non-zero if the disk is inconsistent or structurally broken.
//!
//! With `--map`, prints a text disk map instead: every block by track and
//! sector, marked with the file (or DOS structure) that owns it, then a
//! legend. The map carries on through damage; it exits non-zero only if a
//! block is in conflict or a chain is broken.

#[macro_use]
mod common;

use std::process::ExitCode;

use sector64::Disk;

const USAGE: &str = "usage: s64val [--map] <image>";

fn main() -> ExitCode {
    let mut map = false;
    let mut path = None;
    for arg in std::env::args_os().skip(1) {
        match arg.to_str() {
            Some("--map") => map = true,
            Some(a) if a.starts_with("--") => {
                eprintln!("s64val: unknown option {a}\n{USAGE}");
                return ExitCode::from(2);
            }
            _ if path.is_none() => path = Some(arg),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(path) = path else {
        eprintln!("{USAGE}");
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

    if map {
        return print_map(&disk);
    }

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

/// `--map`: the header line, then the text map and its legend.
fn print_map(disk: &Disk) -> ExitCode {
    match disk.header() {
        Ok(h) => outln!("0 \"{}\" {} {}", h.name, h.id, h.dos_type),
        Err(e) => outln!("(header unreadable: {e})"),
    }
    let map = disk.block_map();
    outln!("{}", map.to_text().trim_end());
    let (.., conflict) = map.counts();
    if conflict == 0 && map.problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
