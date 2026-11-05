#![no_main]
//! Fuzz every disk read path: no input may panic.

use libfuzzer_sys::fuzz_target;

use d64::Disk;

fuzz_target!(|data: &[u8]| {
    let Ok(disk) = Disk::load(data.to_vec()) else {
        return;
    };
    let _ = disk.header();
    let _ = disk.blocks_free();
    let _ = disk.validate();
    if let Ok(dir) = disk.directory() {
        for entry in dir.iter().take(64) {
            let _ = disk.read_entry(entry);
            let _ = disk.file_block_chain(entry.first_track, entry.first_sector);
            let _ = disk.side_sector_chain(entry);
            let _ = disk.read_rel(entry);
        }
    }
});
