//! Reads a real relative file minted by the emulated 1541 DOS.
//!
//! `fixtures/rel.d64` was produced by running `fixtures/makerel.bas` under VICE
//! `x64sc` (see the project notes): it OPENs a REL file with record length 8 and
//! writes 50 records `"R 1"`, `"R 2"`, … `"R 50"`. The emulated Commodore DOS —
//! an implementation entirely independent of this crate — laid out the side
//! sectors and data blocks. Notably, `c1541` cannot read or extract REL files,
//! so this fixture is the only way to check the side-sector path against real
//! DOS output.

use d64::{Disk, FileType};

#[test]
fn reads_emulator_minted_rel() {
    let disk = Disk::load(include_bytes!("fixtures/rel.d64").to_vec()).unwrap();

    let entry = disk.find_entry("data").unwrap().expect("DATA present");
    assert_eq!(entry.file_type, FileType::Rel);
    assert_eq!(entry.rel_record_len, 8);

    // A real side-sector chain and the data blocks it indexes.
    let side = disk.side_sector_chain(&entry).unwrap();
    assert!(!side.is_empty(), "expected at least one side sector");
    let data_blocks = disk.rel_data_blocks(&entry).unwrap();
    assert!(data_blocks.len() >= 2, "50 records span more than one data block");

    let rel = disk.read_rel(&entry).unwrap();
    assert_eq!(rel.record_len, 8);
    assert!(rel.records.len() >= 50);

    // The 50 records we wrote come back verbatim: "R" + STR$(n) + CR, NUL-padded.
    for n in 1..=50usize {
        let rec = &rel.records[n - 1];
        let expected = format!("R {n}\r");
        assert_eq!(
            &rec[..expected.len()],
            expected.as_bytes(),
            "record {n} content"
        );
        assert!(
            rec[expected.len()..].iter().all(|&b| b == 0),
            "record {n} padding"
        );
    }
}
