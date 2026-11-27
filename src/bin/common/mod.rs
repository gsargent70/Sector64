//! Shared output helpers for the command-line tools.
//!
//! `println!` panics when stdout is a pipe whose reader has gone away (e.g.
//! `s64ls disk.d64 | head -1`). The tools instead stop quietly with status 0
//! in that case, as a normal Unix tool killed by SIGPIPE would appear to the
//! user, without needing `unsafe` signal handling. Any other stdout error is
//! reported on stderr with status 1.
//!
//! Cargo only builds `src/bin/*.rs` and `src/bin/*/main.rs` as binaries, so
//! this directory module is shared, not a tool of its own.

#![allow(dead_code)] // not every tool uses every helper

use std::io::{self, Write};

/// `println!` that treats a closed stdout pipe as a quiet, successful exit.
#[allow(unused_macros)] // not every tool prints text
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if let Err(e) = writeln!(std::io::stdout(), $($arg)*) {
            $crate::common::stdout_failed(e)
        }
    }};
}

/// Write raw bytes (an extracted file) to stdout, with the same handling.
pub fn write_stdout(data: &[u8]) {
    let mut out = io::stdout().lock();
    if let Err(e) = out.write_all(data).and_then(|()| out.flush()) {
        stdout_failed(e)
    }
}

/// End the process after a failed stdout write.
pub fn stdout_failed(e: io::Error) -> ! {
    if is_broken_pipe(&e) {
        std::process::exit(0);
    }
    let tool = std::env::args()
        .next()
        .as_deref()
        .and_then(|a| {
            std::path::Path::new(a)
                .file_stem()?
                .to_str()
                .map(String::from)
        })
        .unwrap_or_else(|| "sector64".into());
    eprintln!("{tool}: cannot write to stdout: {e}");
    std::process::exit(1);
}

fn is_broken_pipe(e: &io::Error) -> bool {
    // Windows reports a closed pipe as ERROR_NO_DATA (232), which std maps to
    // BrokenPipe, or sometimes ERROR_PIPE_NOT_CONNECTED (233), which it doesn't.
    e.kind() == io::ErrorKind::BrokenPipe
        || (cfg!(windows) && matches!(e.raw_os_error(), Some(232 | 233)))
}

/// A disk image or archive read from a path, for the tools that take
/// either.
pub enum Loaded {
    /// A disk image (D64, D71, D81, G64, D2M, or a Zip-Code set joined up).
    Disk(sector64::Disk),
    /// An archive's files, with the archive's file name (no extension) for
    /// naming whatever is made from it.
    Archive(String, Vec<sector64::listing::ListedFile>),
}

impl Loaded {
    /// Every file, as a flat list.
    pub fn files(self) -> Result<Vec<sector64::listing::ListedFile>, String> {
        match self {
            Loaded::Disk(d) => sector64::listing::disk_files(&d).map_err(|e| e.to_string()),
            Loaded::Archive(_, files) => Ok(files),
        }
    }
}

/// Read `path` as a disk image, a Zip-Code set (any of its four parts
/// stands for the set), or an archive. Errors name the path.
pub fn load_any(path: &std::path::Path) -> Result<Loaded, String> {
    use sector64::listing::{read_archive, zipcode_parts};
    use std::ffi::OsStr;
    let shown = path.display();
    let read = |p: &std::path::Path| {
        std::fs::read(p).map_err(|e| format!("cannot read {}: {e}", p.display()))
    };
    let disk = |bytes: Vec<u8>| {
        sector64::Disk::load(bytes)
            .map(Loaded::Disk)
            .map_err(|e| format!("{shown}: {e}"))
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
    let disk_error = match sector64::Disk::load(bytes.as_slice()) {
        Ok(_) => return disk(bytes),
        Err(e) => e,
    };
    let ext = path.extension().and_then(OsStr::to_str).unwrap_or_default();
    match read_archive(ext, &bytes) {
        Some(Ok(archive)) => {
            let title = path
                .file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or("archive")
                .to_string();
            Ok(Loaded::Archive(title, archive.files))
        }
        Some(Err(e)) => Err(format!("{shown}: {e}")),
        None => Err(format!(
            "{shown}: not a disk image or archive ({disk_error})"
        )),
    }
}
