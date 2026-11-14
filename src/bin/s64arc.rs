//! `s64arc` — list or extract a Commodore **ARC** or **SDA** archive.
//!
//! Usage:
//!   `s64arc list <archive.arc|.sda>`              list the entries
//!   `s64arc extract <archive.arc|.sda> <name>`    extract one file to stdout
//!
//! A `.sda` self-dissolving archive is detected by its BASIC loader and its
//! embedded ARC archive is read. Stored, packed, squeezed, crunched, and
//! squashed entries all decode; an entry using a method that is not
//! implemented reports that method instead.

#[macro_use]
mod common;

use std::process::ExitCode;

use sector64::{parse_arc, parse_sda, ArcEntry};

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
    // A `.SDA` self-dissolving archive is a C64 BASIC loader ($0801) with an
    // ARC archive appended; a bare `.ARC` starts at the header.
    let parsed = if bytes.starts_with(&[0x01, 0x08]) {
        parse_sda(&bytes)
    } else {
        parse_arc(&bytes)
    };
    parsed.map_err(|e| {
        eprintln!("s64arc: {e}");
        ExitCode::FAILURE
    })
}

fn run_list(path: &str) -> ExitCode {
    let entries = match load(path) {
        Ok(e) => e,
        Err(c) => return c,
    };
    outln!("{:<18} {:<4} {:>8} {:>5}  method", "name", "type", "size", "blks");
    for e in &entries {
        outln!(
            "{:<18} {:<4} {:>8} {:>5}  {}",
            format!("\"{}\"", e.name),
            e.file_type.mnemonic(),
            e.orig_size,
            e.blocks,
            e.method.label(),
        );
    }
    outln!("{} file(s)", entries.len());
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
            common::write_stdout(&data);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("s64arc: {e}");
            ExitCode::FAILURE
        }
    }
}
