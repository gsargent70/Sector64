//! `s64pack` — create a Commodore archive from host files.
//!
//! Usage: `s64pack <out.t64|.lnx|.lbr|.p00> <file>...`
//!
//! Each input file becomes a member; its CBM name is the upper-cased file stem
//! (≤16 chars) and its type is PRG. A `.p00` holds exactly one file.

#[macro_use]
mod common;

use std::path::Path;
use std::process::ExitCode;

use sector64::{write_lbr, write_lnx, write_p00, write_t64, FileType, ImportFile};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: s64pack <out.t64|.lnx|.lbr|.p00> <file>...");
        return ExitCode::from(2);
    }
    let out = &args[0];
    let inputs = &args[1..];

    let mut files = Vec::new();
    for path in inputs {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("s64pack: cannot read {path}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("file");
        let name: String = stem.to_uppercase().chars().take(16).collect();
        files.push(ImportFile { name, file_type: FileType::Prg, data });
    }

    let lower = out.to_ascii_lowercase();
    let is_p00 = {
        let e = lower.rsplit('.').next().unwrap_or("");
        let b = e.as_bytes();
        b.len() == 3 && matches!(b[0], b'p' | b's' | b'u' | b'r' | b'd') && b[1].is_ascii_digit() && b[2].is_ascii_digit()
    };

    let bytes = if lower.ends_with(".t64") {
        write_t64(&files)
    } else if lower.ends_with(".lnx") {
        write_lnx(&files)
    } else if lower.ends_with(".lbr") {
        write_lbr(&files)
    } else if lower.ends_with(".p00") || is_p00 {
        if files.len() != 1 {
            eprintln!("s64pack: a .p00 holds exactly one file");
            return ExitCode::FAILURE;
        }
        write_p00(&files[0])
    } else {
        eprintln!("s64pack: unknown output type (expected .t64/.lnx/.lbr/.p00)");
        return ExitCode::from(2);
    };

    match std::fs::write(out, &bytes) {
        Ok(()) => {
            outln!("wrote {out} ({} file(s), {} bytes)", files.len(), bytes.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("s64pack: cannot write {out}: {e}");
            ExitCode::FAILURE
        }
    }
}
