//! `s64basic` — turn a BASIC program's text into a runnable PRG.
//!
//! Usage: `s64basic [--dialect=v2|7.0|simons] [--load=HEX] <text> <out.prg>`
//!
//! The text is one program line per line, as `s64extract --list` or VICE's
//! `petcat` writes it. The dialect is V2 unless given; the load address is
//! the dialect's usual one ($0801, or $1C01 for BASIC 7.0) unless given or
//! named in a petcat heading. Problems are reported by line and nothing is
//! written.

#[macro_use]
mod common;

use std::process::ExitCode;

use sector64::tokenize::tokenize;
use sector64::BasicDialect;

const USAGE: &str = "usage: s64basic [--dialect=v2|7.0|simons] [--load=HEX] <text> <out.prg>";

fn main() -> ExitCode {
    let mut dialect = BasicDialect::V2;
    let mut load = None;
    let mut positional = Vec::new();
    for arg in std::env::args_os().skip(1) {
        let Some(opt) = arg.to_str().filter(|a| a.starts_with("--")) else {
            positional.push(arg);
            continue;
        };
        if let Some(d) = opt.strip_prefix("--dialect=") {
            dialect = match d.to_ascii_lowercase().as_str() {
                "v2" | "2" => BasicDialect::V2,
                "7.0" | "7" | "v7" => BasicDialect::V7,
                "simons" => BasicDialect::Simons,
                other => {
                    eprintln!("s64basic: unknown dialect \"{other}\" (use v2, 7.0 or simons)");
                    return ExitCode::from(2);
                }
            };
        } else if let Some(hex) = opt.strip_prefix("--load=") {
            let digits = hex.trim_start_matches('$').trim_start_matches("0x");
            match u16::from_str_radix(digits, 16) {
                Ok(a) => load = Some(a),
                Err(_) => {
                    eprintln!("s64basic: \"{hex}\" isn't a hex address");
                    return ExitCode::from(2);
                }
            }
        } else {
            eprintln!("s64basic: unknown option {opt}\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    let [path, out] = &positional[..] else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };

    let shown = path.to_string_lossy();
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("s64basic: cannot read {shown}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let t = match tokenize(&String::from_utf8_lossy(&bytes), dialect, load) {
        Ok(t) => t,
        Err(problems) => {
            for p in problems {
                eprintln!("s64basic: {shown}: {p}");
            }
            return ExitCode::FAILURE;
        }
    };
    for w in &t.warnings {
        eprintln!("s64basic: {shown}: warning: {w}");
    }
    if let Err(e) = std::fs::write(out, &t.prg) {
        eprintln!("s64basic: cannot write {}: {e}", out.to_string_lossy());
        return ExitCode::FAILURE;
    }
    outln!(
        "wrote {} ({} line{}, {} bytes, {}, loads at ${:04X})",
        out.to_string_lossy(),
        t.lines,
        if t.lines == 1 { "" } else { "s" },
        t.prg.len(),
        dialect.name(),
        t.load_address
    );
    ExitCode::SUCCESS
}
