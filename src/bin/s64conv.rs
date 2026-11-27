//! `s64conv` — convert a disk image or archive to another format in one step.
//!
//! ```text
//! s64conv [--force] <input> <output.d64|.d71|.d81|.t64|.lnx|.lbr>
//! ```
//!
//! The input is any disk image (D64, D71, D81, G64, D2M, a Zip-Code set) or
//! archive Sector64 reads. The output's extension picks its format. Disk to
//! disk keeps every entry exactly (names, types, lock flags, separators) and
//! the disk's name and ID; an archive made into a disk is named after the
//! archive. Files that can't go across are listed on stderr with the reason.
//!
//! Exit status: 0 when every file went across, 1 when some were left out,
//! 2 when nothing was written.

#[macro_use]
mod common;

use std::ffi::OsStr;
use std::path::Path;
use std::process::ExitCode;

use common::Loaded;
use sector64::convert::{convert_disk, convert_files, Converted, Target};

const USAGE: &str = "usage: s64conv [--force] <input> <output.d64|.d71|.d81|.t64|.lnx|.lbr>";

const TROUBLE: u8 = 2;

fn main() -> ExitCode {
    let mut force = false;
    let mut positional = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.to_str().filter(|a| a.starts_with("--")) {
            Some("--force") => force = true,
            Some(o) => {
                eprintln!("s64conv: unknown option {o}\n{USAGE}");
                return ExitCode::from(TROUBLE);
            }
            None => positional.push(arg),
        }
    }
    let [input, output] = &positional[..] else {
        eprintln!("{USAGE}");
        return ExitCode::from(TROUBLE);
    };
    let (input, output) = (Path::new(input), Path::new(output));
    let ext = output
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    let Some(to) = Target::from_ext(ext) else {
        eprintln!(
            "s64conv: {}: the output must end in .d64, .d71, .d81, .t64, .lnx or .lbr",
            output.display()
        );
        return ExitCode::from(TROUBLE);
    };
    if output.exists() && !force {
        eprintln!(
            "s64conv: {} already exists (--force replaces it)",
            output.display()
        );
        return ExitCode::from(TROUBLE);
    }
    let converted = match convert(input, to) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s64conv: {e}");
            return ExitCode::from(TROUBLE);
        }
    };
    if let Err(e) = std::fs::write(output, &converted.bytes) {
        eprintln!("s64conv: cannot write {}: {e}", output.display());
        return ExitCode::from(TROUBLE);
    }
    for (name, why) in &converted.skipped {
        eprintln!("s64conv: left out \"{name}\": {why}");
    }
    for (name, how) in &converted.notes {
        eprintln!("s64conv: \"{name}\" {how}");
    }
    let (n, left) = (converted.copied.len(), converted.skipped.len());
    let files = if n == 1 { "file" } else { "files" };
    if left == 0 {
        outln!("wrote {}: {n} {files}", output.display());
        ExitCode::SUCCESS
    } else {
        outln!("wrote {}: {n} {files}, {left} left out", output.display());
        ExitCode::FAILURE
    }
}

/// Read `path` as a disk image (or Zip-Code set) or an archive and convert it.
fn convert(path: &Path, to: Target) -> Result<Converted, String> {
    match common::load_any(path)? {
        Loaded::Disk(disk) => {
            convert_disk(&disk, to).map_err(|e| format!("{}: {e}", path.display()))
        }
        Loaded::Archive(title, files) => Ok(convert_files(&title.to_uppercase(), &files, to)),
    }
}
