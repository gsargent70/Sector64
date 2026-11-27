//! `s64cmp` — compare two disks or archives, file by file.
//!
//! Usage: `s64cmp [--all] <left> <right>`
//!
//! Each side can be any disk image or archive Sector64 reads. Files are
//! matched by name; the ones that differ, or are on only one side, are
//! listed (`--all` lists the matching ones too), then a summary line. Like
//! `cmp` and `diff`, it exits with 0 when the two hold the same files, 1
//! when they don't, and 2 when they couldn't be compared.

#[macro_use]
mod common;

use std::path::Path;
use std::process::ExitCode;

use sector64::compare::{compare, summarize, Item, Status};
use sector64::listing::ListedFile;

const USAGE: &str = "usage: s64cmp [--all] <left> <right>";

/// Couldn't compare: bad arguments, or a file that can't be read.
const TROUBLE: u8 = 2;

fn main() -> ExitCode {
    let mut all = false;
    let mut positional = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.to_str().filter(|a| a.starts_with("--")) {
            Some("--all") => all = true,
            Some(o) => {
                eprintln!("s64cmp: unknown option {o}\n{USAGE}");
                return ExitCode::from(TROUBLE);
            }
            None => positional.push(arg),
        }
    }
    let [left, right] = &positional[..] else {
        eprintln!("{USAGE}");
        return ExitCode::from(TROUBLE);
    };
    let mut sides = Vec::with_capacity(2);
    for path in [left, right] {
        match load(Path::new(path)) {
            Ok(files) => sides.push(files),
            Err(e) => {
                eprintln!("s64cmp: {e}");
                return ExitCode::from(TROUBLE);
            }
        }
    }
    let (left, right) = (&sides[0], &sides[1]);
    let rows = compare(&items(left), &items(right));
    for row in &rows {
        if row.status == Status::Same && !all {
            continue;
        }
        let l = row.left.map(|i| &left[i]);
        let r = row.right.map(|i| &right[i]);
        let quoted = format!("\"{}\"", row.name);
        outln!(
            "{} {quoted:<18} {:>8} {:>8}  {}",
            row.status.mark(),
            size(l),
            size(r),
            row.status.describe([l.map(|f| f.kind), r.map(|f| f.kind)])
        );
    }
    let summary = summarize(&rows);
    outln!("{summary}");
    if summary.identical() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn items(files: &[ListedFile]) -> Vec<Item<'_>> {
    files
        .iter()
        .map(|f| Item {
            name: &f.name,
            kind: f.kind,
            data: f.data.as_deref().ok(),
        })
        .collect()
}

/// A file's size in bytes, `?` if it can't be read, `-` if it isn't there.
fn size(file: Option<&ListedFile>) -> String {
    match file.map(|f| &f.data) {
        Some(Ok(d)) => d.len().to_string(),
        Some(Err(_)) => "?".into(),
        None => "-".into(),
    }
}

/// The files in the disk image or archive at `path`. Any part of a Zip-Code
/// set stands for the whole set.
fn load(path: &Path) -> Result<Vec<ListedFile>, String> {
    common::load_any(path)?.files()
}
