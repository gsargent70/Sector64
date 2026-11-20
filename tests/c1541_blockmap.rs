//! Disk map against disks written by VICE's `c1541`, an independent DOS
//! implementation: every block its BAM allocates must belong to a file or the
//! DOS, and every file must own exactly the blocks its entry counts.
//!
//! Skipped (not failed) when `c1541` is not on `PATH`.

use std::fs;
use std::process::Command;

use sector64::{BlockStatus, Disk};

fn c1541_available() -> bool {
    Command::new("c1541")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

#[test]
fn c1541_disks_map_without_orphans_or_conflicts() {
    if !c1541_available() {
        eprintln!("c1541 not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("s64-blockmap-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let sizes = [1usize, 254, 255, 5000, 30_000, 777];
    for fmt in ["d64", "d71", "d81"] {
        let img = dir.join(format!("map.{fmt}"));
        let mut cmd = Command::new("c1541");
        cmd.args(["-format", &format!("map,{}", &fmt[1..]), fmt])
            .arg(&img);
        for (i, &size) in sizes.iter().enumerate() {
            let p = dir.join(format!("f{i}.prg"));
            fs::write(&p, vec![i as u8; size]).unwrap();
            cmd.arg("-write").arg(&p).arg(format!("file{i}"));
        }
        // Scratch one so its freed blocks must read as free, not orphaned.
        cmd.args(["-delete", "file2"]);
        assert!(cmd.output().expect("run c1541").status.success(), "{fmt}");

        let disk = Disk::load(fs::read(&img).unwrap()).unwrap();
        let map = disk.block_map();
        assert!(map.problems.is_empty(), "{fmt}: {:?}", map.problems);
        let (free, file, _system, orphan, conflict) = map.counts();
        assert_eq!((orphan, conflict), (0, 0), "{fmt}");
        // "Blocks free" leaves out the directory track.
        let dir_track = disk.geometry().directory_track() as usize;
        let free_on_dir = map.tracks[dir_track - 1]
            .iter()
            .filter(|b| b.status() == BlockStatus::Free)
            .count();
        assert_eq!(
            free - free_on_dir,
            disk.blocks_free().unwrap() as usize,
            "{fmt}: free blocks"
        );
        assert_eq!(map.files.len(), sizes.len() - 1, "{fmt}");
        let mut owned = 0;
        for (i, e) in map.files.iter().enumerate() {
            let blocks = map.blocks_of(i);
            assert_eq!(blocks.len(), e.blocks as usize, "{fmt} {}", e.name);
            assert!(blocks
                .iter()
                .all(|&(t, s)| map.get(t, s).unwrap().status() == BlockStatus::File(i)));
            owned += blocks.len();
        }
        assert_eq!(file, owned, "{fmt}");
    }
    let _ = fs::remove_dir_all(&dir);
}
