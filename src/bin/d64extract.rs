//! `d64extract` — extract one file from a D64 image to stdout.
//!
//! Usage: `d64extract <image.d64> <name>`
//!
//! `<name>` is matched case-insensitively against the displayed directory
//! name. The raw file bytes (including the two-byte PRG load address, if any)
//! are written to stdout, so redirect to a file:
//!
//! ```sh
//! d64extract disk.d64 GAME > game.prg
//! ```

use std::io::Write;
use std::process::ExitCode;

use d64::Disk;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(path), Some(name)) = (args.next(), args.next()) else {
        eprintln!("usage: d64extract <image.d64> <name>");
        return ExitCode::from(2);
    };
    let name = name.to_string_lossy().into_owned();

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("d64extract: cannot read {}: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    let disk = match Disk::load(bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("d64extract: {e}");
            return ExitCode::FAILURE;
        }
    };

    let entry = match disk.find_entry(&name) {
        Ok(Some(e)) => e,
        Ok(None) => {
            eprintln!("d64extract: no file named \"{name}\" on this disk");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("d64extract: {e}");
            return ExitCode::FAILURE;
        }
    };

    let data = match disk.read_entry(&entry) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("d64extract: reading \"{name}\": {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = std::io::stdout().write_all(&data) {
        eprintln!("d64extract: write failed: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!(
        "d64extract: {} bytes from \"{}\" ({})",
        data.len(),
        entry.name,
        entry.file_type.mnemonic()
    );
    ExitCode::SUCCESS
}
