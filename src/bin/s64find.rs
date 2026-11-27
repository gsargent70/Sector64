//! `s64find` — search the files of disks and archives for text or bytes.
//!
//! ```text
//! s64find [--hex] [-l] [--max N] <pattern> <disk or archive>...
//! ```
//!
//! Text matches the way a C64 shows it: letters in either case, whichever
//! character set wrote them, and a file whose name contains the text is
//! listed too. `--hex` takes bytes such as `A9 00 8D`, with `??` for any
//! byte. Each file with a match gets a line with its offsets (and, on a
//! disk, the track and sector of the first); `-l` prints only the names.
//!
//! Exit status, like `grep`: 0 when something matched, 1 when nothing did,
//! 2 on an error.

#[macro_use]
mod common;

use std::path::Path;
use std::process::ExitCode;

use common::Loaded;
use sector64::search::{block_of_offset, Pattern};

const USAGE: &str = "usage: s64find [--hex] [-l] [--max N] <pattern> <disk or archive>...";

const TROUBLE: u8 = 2;

fn main() -> ExitCode {
    let (mut hex, mut names_only, mut max) = (false, false, 8usize);
    let mut positional = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--hex") => hex = true,
            Some("-l") => names_only = true,
            Some("--max") => match args.next().and_then(|n| n.to_str()?.parse().ok()) {
                Some(n) if n > 0 => max = n,
                _ => {
                    eprintln!("s64find: --max takes a number of offsets, 1 or more\n{USAGE}");
                    return ExitCode::from(TROUBLE);
                }
            },
            Some(o) if o.starts_with('-') && o.len() > 1 && positional.is_empty() => {
                eprintln!("s64find: unknown option {o}\n{USAGE}");
                return ExitCode::from(TROUBLE);
            }
            _ => positional.push(arg),
        }
    }
    let Some((query, inputs)) = positional
        .split_first()
        .filter(|(_, rest)| !rest.is_empty())
    else {
        eprintln!("{USAGE}");
        return ExitCode::from(TROUBLE);
    };
    let query = query.to_string_lossy();
    let pattern = if hex {
        Pattern::hex(&query)
    } else {
        Pattern::text(&query)
    };
    let pattern = match pattern {
        Ok(p) => p,
        Err(e) => {
            eprintln!("s64find: {e}");
            return ExitCode::from(TROUBLE);
        }
    };
    let (mut found, mut trouble) = (false, false);
    for input in inputs {
        let path = Path::new(input);
        match common::load_any(path) {
            Ok(loaded) => {
                found |= search(&pattern, hex, names_only, max, path, loaded);
            }
            Err(e) => {
                eprintln!("s64find: {e}");
                trouble = true;
            }
        }
    }
    if trouble {
        ExitCode::from(TROUBLE)
    } else if found {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Search one disk or archive, printing a line per file that matches.
/// Returns whether any did.
fn search(
    pattern: &Pattern,
    hex: bool,
    names_only: bool,
    max: usize,
    path: &Path,
    loaded: Loaded,
) -> bool {
    let shown = path.display();
    // On a disk, a match's offset maps to the block that holds it.
    let (files, chains) = match loaded {
        Loaded::Disk(disk) => {
            let chains = disk.block_map().chains;
            match sector64::listing::disk_files(&disk) {
                Ok(files) => (files, chains),
                Err(e) => {
                    eprintln!("s64find: {shown}: {e}");
                    return false;
                }
            }
        }
        Loaded::Archive(_, files) => (files, Vec::new()),
    };
    let mut found = false;
    for (i, f) in files.iter().enumerate() {
        let in_name = !hex && pattern.in_name(&f.name);
        let data = match &f.data {
            Ok(d) => d,
            Err(e) => {
                eprintln!("s64find: {shown}: \"{}\" can't be read: {e}", f.name);
                continue;
            }
        };
        let offsets = pattern.find(data, max);
        if offsets.is_empty() && !in_name {
            continue;
        }
        found = true;
        let quoted = format!("\"{}\"", f.name);
        if names_only {
            outln!("{shown}: {quoted}");
            continue;
        }
        let kind = if f.kind.is_empty() { "" } else { f.kind };
        let mut line = format!("{shown}: {quoted:<18} {kind:<3}");
        if in_name {
            line += "  name";
        }
        if !offsets.is_empty() {
            let total = if offsets.len() < max {
                offsets.len()
            } else {
                pattern.count(data)
            };
            let list: Vec<String> = offsets.iter().map(|o| format!("${o:04X}")).collect();
            let more = if total > offsets.len() {
                format!(" (+{} more)", total - offsets.len())
            } else {
                String::new()
            };
            let matches = if total == 1 { "match" } else { "matches" };
            line += &format!("  {total} {matches} at {}{more}", list.join(" "));
            let block = chains
                .get(i)
                .and_then(|c| block_of_offset(c, data.len(), offsets[0]));
            if let Some((t, s, at)) = block {
                line += &format!("; first in track {t}, sector {s}, byte {at}");
            }
        }
        outln!("{line}");
    }
    found
}
