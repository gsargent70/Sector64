//! `s64arc` — list or extract a Commodore **ARC** archive.
//!
//! Usage:
//!   `s64arc list <archive.arc>`              list the entries
//!   `s64arc extract <archive.arc> <name>`    extract one file to stdout
//!
//! Only stored (method 0) entries can be extracted today; compressed entries
//! report the method that is not yet supported.

use std::io::Write;
use std::process::ExitCode;

use sector64::{parse_arc, ArcEntry};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, path] if cmd == "list" => run_list(path),
        [cmd, path, name] if cmd == "extract" => run_extract(path, name),
        _ => {
            eprintln!(
                "usage:\n  s64arc list <archive.arc>\n  s64arc extract <archive.arc> <name>"
            );
            ExitCode::from(2)
        }
    }
}

fn load(path: &str) -> Result<Vec<ArcEntry>, ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| {
        eprintln!("s64arc: cannot read {path}: {e}");
        ExitCode::FAILURE
    })?;
    parse_arc(&bytes).map_err(|e| {
        eprintln!("s64arc: {e}");
        ExitCode::FAILURE
    })
}

fn run_list(path: &str) -> ExitCode {
    let entries = match load(path) {
        Ok(e) => e,
        Err(c) => return c,
    };
    println!("{:<18} {:<4} {:>8} {:>5}  method", "name", "type", "size", "blks");
    for e in &entries {
        println!(
            "{:<18} {:<4} {:>8} {:>5}  {}",
            format!("\"{}\"", e.name),
            e.file_type.mnemonic(),
            e.orig_size,
            e.blocks,
            e.method.label(),
        );
    }
    println!("{} file(s)", entries.len());
    ExitCode::SUCCESS
}

fn run_extract(path: &str, name: &str) -> ExitCode {
    let entries = match load(path) {
        Ok(e) => e,
        Err(c) => return c,
    };
    let want = name.to_ascii_uppercase();
    let Some(entry) = entries.iter().find(|e| e.name.trim().to_ascii_uppercase() == want) else {
        eprintln!("s64arc: no entry named {name:?}");
        return ExitCode::FAILURE;
    };
    match entry.decode() {
        Ok(data) => {
            if !entry.verify_checksum(&data) {
                eprintln!(
                    "s64arc: warning: checksum mismatch (stored {:#06x})",
                    entry.checksum
                );
            }
            if std::io::stdout().write_all(&data).is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("s64arc: {e}");
            ExitCode::FAILURE
        }
    }
}
