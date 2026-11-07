//! `s64wr3` — list or extract a Wraptor-3 (`.wr3`) archive.
//!
//! Usage:
//!   `s64wr3 list <archive.wr3>`
//!   `s64wr3 extract <archive.wr3> [out-dir]`
//!
//! `list` shows member names and decompressed sizes. `extract` decodes every
//! member (they share one sliding window, so the whole archive is decoded) and
//! writes each file into `out-dir` (default: the current directory), sanitising
//! the member name into a safe file name.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sector64::{decompress_wr3, Wr3File};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, path] if cmd == "list" => list(path),
        [cmd, path] if cmd == "extract" => extract(path, Path::new(".")),
        [cmd, path, out] if cmd == "extract" => extract(path, Path::new(out)),
        _ => {
            eprintln!("usage: s64wr3 list <archive.wr3>");
            eprintln!("       s64wr3 extract <archive.wr3> [out-dir]");
            ExitCode::from(2)
        }
    }
}

fn load(path: &str) -> Result<Vec<Wr3File>, ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| {
        eprintln!("s64wr3: cannot read {path}: {e}");
        ExitCode::FAILURE
    })?;
    decompress_wr3(&bytes).map_err(|e| {
        eprintln!("s64wr3: {e}");
        ExitCode::FAILURE
    })
}

fn list(path: &str) -> ExitCode {
    let files = match load(path) {
        Ok(f) => f,
        Err(c) => return c,
    };
    println!("{:<22} {:>10}", "name", "size");
    for f in &files {
        println!("{:<22} {:>10}", format!("\"{}\"", f.name), f.content.len());
    }
    println!("{} file(s)", files.len());
    ExitCode::SUCCESS
}

fn extract(path: &str, out_dir: &Path) -> ExitCode {
    let files = match load(path) {
        Ok(f) => f,
        Err(c) => return c,
    };
    if let Err(e) = std::fs::create_dir_all(out_dir) {
        eprintln!("s64wr3: cannot create {}: {e}", out_dir.display());
        return ExitCode::FAILURE;
    }
    let mut written = 0;
    for f in &files {
        let out = out_dir.join(safe_name(&f.name));
        match std::fs::write(&out, &f.content) {
            Ok(()) => {
                println!("{} ({} bytes)", out.display(), f.content.len());
                written += 1;
            }
            Err(e) => eprintln!("s64wr3: cannot write {}: {e}", out.display()),
        }
    }
    println!("extracted {written}/{} file(s)", files.len());
    if written == files.len() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Turn a member name into a single safe path component: keep printable
/// characters, replace path separators and other awkward bytes with `_`.
fn safe_name(name: &str) -> PathBuf {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = s.trim();
    if trimmed.is_empty() {
        s = "unnamed".to_string();
    } else if trimmed.len() != s.len() {
        s = trimmed.to_string();
    }
    PathBuf::from(s)
}
