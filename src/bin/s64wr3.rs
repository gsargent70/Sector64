//! `s64wr3` — list a Wraptor-3 (`.wr3`) archive.
//!
//! Usage: `s64wr3 list <archive.wr3>`
//!
//! Lists the member names and compressed sizes. The Wraptor compression is not
//! decoded, so there is no extract command.

use std::process::ExitCode;

use sector64::parse_wr3;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.as_slice() {
        [cmd, path] if cmd == "list" => path,
        _ => {
            eprintln!("usage: s64wr3 list <archive.wr3>");
            return ExitCode::from(2);
        }
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("s64wr3: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let entries = match parse_wr3(&bytes) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("s64wr3: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("{:<20} {:>10}", "name", "packed");
    for e in &entries {
        println!("{:<20} {:>10}", format!("\"{}\"", e.name), e.compressed.len());
    }
    println!("{} file(s)", entries.len());
    ExitCode::SUCCESS
}
