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

use sector64::convert::{convert_disk, convert_files, Converted, Target};
use sector64::listing::{read_archive, zipcode_parts};
use sector64::Disk;

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
    let shown = path.display();
    let read = |p: &Path| std::fs::read(p).map_err(|e| format!("cannot read {}: {e}", p.display()));
    let disk = |bytes: Vec<u8>| -> Result<Converted, String> {
        let disk = Disk::load(bytes).map_err(|e| format!("{shown}: {e}"))?;
        convert_disk(&disk, to).map_err(|e| format!("{shown}: {e}"))
    };
    if let Some(names) = zipcode_parts(path) {
        let mut parts = Vec::with_capacity(4);
        for name in names {
            parts.push(read(&path.with_file_name(name))?);
        }
        let d64 = sector64::parse_zipcode([&parts[0], &parts[1], &parts[2], &parts[3]])
            .map_err(|e| format!("{shown}: {e}"))?;
        return disk(d64);
    }
    let bytes = read(path)?;
    let ext = path.extension().and_then(OsStr::to_str).unwrap_or_default();
    let disk_error = match Disk::load(bytes.as_slice()) {
        Ok(_) => return disk(bytes),
        Err(e) => e,
    };
    match read_archive(ext, &bytes) {
        Some(Ok(archive)) => {
            let title = path
                .file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or("archive")
                .to_uppercase();
            Ok(convert_files(&title, &archive.files, to))
        }
        Some(Err(e)) => Err(format!("{shown}: {e}")),
        None => Err(format!(
            "{shown}: not a disk image or archive ({disk_error})"
        )),
    }
}
