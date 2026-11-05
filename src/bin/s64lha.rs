//! `s64lha` — list or extract a Commodore **LHA** archive (`.lzh`/`.sfx`).
//!
//! Usage:
//!   `s64lha list <archive.lzh|.sfx>`              list the entries
//!   `s64lha extract <archive.lzh|.sfx> <name>`    extract one file to stdout
//!
//! A self-extracting `.sfx` is handled by locating the first archive header
//! past its loader stub. Only stored (`-lh0-`) entries can be extracted today;
//! compressed entries report the method that is not yet supported.

use std::io::Write;
use std::process::ExitCode;

use sector64::{parse_lha, LhaEntry};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, path] if cmd == "list" => run_list(path),
        [cmd, path, name] if cmd == "extract" => run_extract(path, name),
        _ => {
            eprintln!(
                "usage:\n  s64lha list <archive.lzh|.sfx>\n  s64lha extract <archive.lzh|.sfx> <name>"
            );
            ExitCode::from(2)
        }
    }
}

fn load(path: &str) -> Result<Vec<LhaEntry>, ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| {
        eprintln!("s64lha: cannot read {path}: {e}");
        ExitCode::FAILURE
    })?;
    parse_lha(&bytes).map_err(|e| {
        eprintln!("s64lha: {e}");
        ExitCode::FAILURE
    })
}

fn run_list(path: &str) -> ExitCode {
    let entries = match load(path) {
        Ok(e) => e,
        Err(c) => return c,
    };
    println!("{:<18} {:<4} {:>8} {:>8}  method", "name", "type", "orig", "packed");
    for e in &entries {
        println!(
            "{:<18} {:<4} {:>8} {:>8}  {}",
            format!("\"{}\"", e.name),
            e.file_type.mnemonic(),
            e.original_size,
            e.compressed_size,
            e.method_str(),
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
    let Some(entry) = entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)) else {
        eprintln!("s64lha: no entry named {name:?}");
        return ExitCode::FAILURE;
    };
    match entry.decode() {
        Ok(data) => {
            if std::io::stdout().write_all(&data).is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("s64lha: {e}");
            ExitCode::FAILURE
        }
    }
}
