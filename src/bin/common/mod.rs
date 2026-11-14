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
#[allow(unused_macros)] // s64extract only writes raw bytes
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
