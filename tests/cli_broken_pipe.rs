//! The CLI tools exit quietly with status 0 when the reader of their stdout
//! goes away (`s64ls big.d64 | head -1`), instead of panicking with
//! "failed printing to stdout: Broken pipe".

use std::io::Read;
use std::process::{Command, Stdio};

use sector64::{Disk, FileType};

/// A D64 with enough entries that the listing outgrows the pipe buffer.
fn crowded_disk(path: &std::path::Path) {
    let mut disk = Disk::format("CROWDED", "CR");
    for i in 0..144 {
        disk.write_file(
            &format!("FILE {i:03}"),
            FileType::Prg,
            &[0x01, 0x08, i as u8],
        )
        .unwrap();
    }
    std::fs::write(path, disk.as_bytes()).unwrap();
}

fn run_and_hang_up(tool: &str, args: &[&std::ffi::OsStr]) {
    let mut child = Command::new(tool)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Hang up straight away: the tool's first write then hits a closed pipe.
    drop(child.stdout.take());
    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "{tool} exited with {status}; stderr: {err}"
    );
    assert!(err.is_empty(), "{tool} wrote to stderr: {err}");
}

#[test]
fn listing_into_a_closed_pipe_exits_cleanly() {
    let dir = std::env::temp_dir().join(format!("s64pipe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let img = dir.join("crowded.d64");
    crowded_disk(&img);
    // Repeat a few times; it must never fail.
    for _ in 0..5 {
        run_and_hang_up(env!("CARGO_BIN_EXE_s64ls"), &[img.as_os_str()]);
        run_and_hang_up(env!("CARGO_BIN_EXE_s64val"), &[img.as_os_str()]);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
