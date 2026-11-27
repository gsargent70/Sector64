//! Directory editing checked against VICE's `c1541`.
//!
//! Renames the disk, locks a file, reorders the directory and adds
//! separators, then has `c1541` (an independent implementation) list the
//! disk and validate it. The listing must show the edits, and validating
//! must leave every byte as it was: nothing we wrote looks like a lost or
//! mis-allocated block to the drive's own rules.
//!
//! Skipped (not failed) when `c1541` is not on `PATH`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use sector64::{Disk, FileType, SEPARATOR};

fn c1541_available() -> bool {
    Command::new("c1541")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

#[test]
fn c1541_lists_and_validates_an_edited_directory() {
    if !c1541_available() {
        eprintln!("skipping: c1541 (VICE) not found on PATH");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c1541_diredit");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    for (ext, mut disk) in [
        ("d64", Disk::format("old name", "01")),
        ("d81", Disk::format_d81("old name", "01")),
    ] {
        // Ten files, so the directory spans two blocks on a D64.
        for i in 0..10 {
            let data: Vec<u8> = (0..300 * (i + 1)).map(|b| b as u8).collect();
            disk.write_file(&format!("file{i}"), FileType::Prg, &data)
                .unwrap();
        }
        disk.set_header("new name", "n2").unwrap();
        disk.set_locked(3, true).unwrap();
        disk.move_entry(9, 0).unwrap(); // file9 first
        disk.add_separator(SEPARATOR, Some(0)).unwrap(); // after file9
        disk.add_separator(SEPARATOR, Some(10)).unwrap(); // last

        let img = dir.join(format!("edited.{ext}"));
        fs::write(&img, disk.as_bytes()).unwrap();

        let out = Command::new("c1541")
            .arg(&img)
            .arg("-dir")
            .output()
            .unwrap();
        let listing = String::from_utf8_lossy(&out.stdout).to_lowercase();
        // The header line and the entries are the lines with a quoted name
        // (c1541 also prints status lines around them).
        let lines: Vec<&str> = listing.lines().filter(|l| l.contains('"')).collect();
        assert!(
            lines[0].contains("\"new name ") && lines[0].contains("n2"),
            "{listing}"
        );
        let order: Vec<String> = lines[1..]
            .iter()
            .filter_map(|l| l.split('"').nth(1).map(str::to_string))
            .collect();
        let want: Vec<&str> = [
            "file9",
            "----------------",
            "file0",
            "file1",
            "file2",
            "file3",
            "file4",
            "file5",
            "file6",
            "file7",
            "file8",
            "----------------",
        ]
        .to_vec();
        assert_eq!(order, want, "{listing}");
        let locked = lines.iter().find(|l| l.contains("\"file3\"")).unwrap();
        assert!(locked.contains("prg<"), "file3 is locked: {listing}");
        assert_eq!(
            lines.iter().filter(|l| l.contains('<')).count(),
            1,
            "{listing}"
        );
        let sep = lines.iter().find(|l| l.contains("\"----")).unwrap();
        assert!(
            sep.trim_start().starts_with('0') && sep.contains("del"),
            "{listing}"
        );

        // c1541's validate rebuilds the BAM from the directory: it must find
        // nothing to change.
        let checked = dir.join(format!("validated.{ext}"));
        fs::copy(&img, &checked).unwrap();
        let v = Command::new("c1541")
            .arg(&checked)
            .arg("-validate")
            .output()
            .unwrap();
        assert!(v.status.success(), "{}", String::from_utf8_lossy(&v.stderr));
        assert!(
            fs::read(&checked).unwrap() == disk.as_bytes(),
            "c1541 -validate changed the {ext} image"
        );
    }
}
