//! `s64extract` — extract one file from a D64 image to stdout.
//!
//! Usage: `s64extract [--list[=v2|7.0|simons]] <image> <name>`
//!
//! `<name>` is matched case-insensitively against the displayed directory
//! name. The raw file bytes (including the two-byte PRG load address, if any)
//! are written to stdout, so redirect to a file:
//!
//! ```sh
//! s64extract disk.d64 GAME > game.prg
//! ```
//!
//! With `--list`, a BASIC program is listed as text instead, the way `LIST`
//! shows it on a C64. The dialect (BASIC V2, the C128's BASIC 7.0 or Simons'
//! BASIC) is detected, or given as `--list=v2`, `--list=7.0` or
//! `--list=simons`.

#[macro_use]
mod common;

use std::process::ExitCode;

use sector64::{BasicDialect, Disk};

const USAGE: &str = "usage: s64extract [--list[=v2|7.0|simons]] <image> <name>";

/// What to write: the raw bytes, or a BASIC listing (in a given dialect, or
/// detected when `None`).
enum Output {
    Raw,
    List(Option<BasicDialect>),
}

fn main() -> ExitCode {
    let mut output = Output::Raw;
    let mut positional = Vec::new();
    for arg in std::env::args_os().skip(1) {
        let opt = arg.to_str().filter(|a| a.starts_with("--"));
        match opt {
            Some("--list") => output = Output::List(None),
            Some(o) if o.starts_with("--list=") => {
                let d = match o["--list=".len()..].to_ascii_lowercase().as_str() {
                    "v2" | "2" => BasicDialect::V2,
                    "7.0" | "7" | "v7" => BasicDialect::V7,
                    "simons" => BasicDialect::Simons,
                    other => {
                        eprintln!(
                            "s64extract: unknown BASIC dialect \"{other}\" (use v2, 7.0 or simons)"
                        );
                        return ExitCode::from(2);
                    }
                };
                output = Output::List(Some(d));
            }
            Some(o) => {
                eprintln!("s64extract: unknown option {o}\n{USAGE}");
                return ExitCode::from(2);
            }
            None => positional.push(arg),
        }
    }
    let [path, name] = &positional[..] else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let name = name.to_string_lossy().into_owned();

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("s64extract: cannot read {}: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    let disk = match Disk::load(bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("s64extract: {e}");
            return ExitCode::FAILURE;
        }
    };

    let entry = match disk.find_entry(&name) {
        Ok(Some(e)) => e,
        Ok(None) => {
            eprintln!("s64extract: no file named \"{name}\" on this disk");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("s64extract: {e}");
            return ExitCode::FAILURE;
        }
    };

    let data = match disk.read_entry(&entry) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("s64extract: reading \"{name}\": {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Output::List(dialect) = output {
        let listing = match dialect {
            Some(d) => sector64::detokenize_basic_as(&data, d),
            None => sector64::detokenize_basic(&data),
        };
        let Some(l) = listing else {
            eprintln!("s64extract: \"{}\" is not a BASIC program", entry.name);
            return ExitCode::FAILURE;
        };
        outln!("{}", l.text.trim_end_matches('\n'));
        let mut note = format!(
            "s64extract: listed \"{}\" as {}",
            entry.name,
            l.dialect.name()
        );
        if l.trailing_bytes > 0 {
            note.push_str(&format!(
                "; {} bytes of machine code or data follow the program",
                l.trailing_bytes
            ));
        }
        eprintln!("{note}");
        return ExitCode::SUCCESS;
    }

    common::write_stdout(&data);
    eprintln!(
        "s64extract: {} bytes from \"{}\" ({})",
        data.len(),
        entry.name,
        entry.file_type.mnemonic()
    );
    ExitCode::SUCCESS
}
