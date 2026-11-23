//! Repairing damaged disks: each kind of fix on disks broken on purpose, a
//! cross-check of the rebuilt BAM against `c1541 -validate` (skipped when
//! `c1541` isn't on `PATH`), and random damage.

use std::process::Command;

use sector64::repair::{Fix, RepairPlan};
use sector64::{BlockStatus, Disk, FileType};

fn data(seed: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(7) ^ seed).collect()
}

fn fill(disk: &mut Disk) {
    for (i, len) in [600usize, 254, 2000, 1, 5000].into_iter().enumerate() {
        disk.write_file(&format!("F{i}"), FileType::Prg, &data(i as u8, len))
            .unwrap();
    }
}

fn disk() -> Disk {
    let mut d = Disk::format("REPAIR", "R1");
    fill(&mut d);
    d
}

fn chain(d: &Disk, name: &str) -> Vec<(u8, u8)> {
    let e = d.find_entry(name).unwrap().unwrap();
    d.file_block_chain(e.first_track, e.first_sector).unwrap()
}

fn read(d: &Disk, name: &str) -> Vec<u8> {
    d.read_entry(&d.find_entry(name).unwrap().unwrap()).unwrap()
}

fn link(d: &mut Disk, (t, s): (u8, u8), to: (u8, u8)) {
    d.block_mut(t, s).unwrap()[..2].copy_from_slice(&[to.0, to.1]);
}

/// The byte and mask of a 1541 block's BAM bit, within 18/0.
fn bam_bit((t, s): (u8, u8)) -> (usize, u8) {
    (4 + 4 * (t as usize - 1) + 1 + s as usize / 8, 1 << (s % 8))
}

fn flip(d: &mut Disk, b: (u8, u8)) {
    let (byte, bit) = bam_bit(b);
    d.block_mut(18, 0).unwrap()[byte] ^= bit;
}

fn texts(p: &RepairPlan) -> Vec<(String, bool)> {
    p.choices
        .iter()
        .map(|c| (c.fix.to_string(), c.apply))
        .collect()
}

fn want(p: &RepairPlan, expect: &[(&str, bool)]) {
    let got = texts(p);
    let got: Vec<_> = got.iter().map(|(s, b)| (s.as_str(), *b)).collect();
    assert_eq!(got, expect);
}

/// Set every fix to apply, or not, by `f`.
fn choose(p: &mut RepairPlan, f: impl Fn(&Fix) -> bool) {
    for c in &mut p.choices {
        c.apply = f(&c.fix);
    }
}

fn clean(d: &Disk) {
    let plan = d.repair_plan();
    assert!(plan.choices.is_empty() && plan.notes.is_empty(), "{plan:?}");
    assert!(d.validate().unwrap().is_clean());
}

#[test]
fn healthy_disks_need_nothing() {
    clean(&disk());
    let mut d = Disk::format_d71("R", "R1");
    fill(&mut d);
    clean(&d);
    let mut d = Disk::format_d81("R", "R1");
    fill(&mut d);
    clean(&d);
}

#[test]
fn rebuilds_the_bam() {
    let good = disk();
    let mut d = good.clone();
    let used = chain(&d, "F2");
    flip(&mut d, used[0]);
    flip(&mut d, used[3]);
    flip(&mut d, (30, 5)); // free to allocated
    d.block_mut(18, 0).unwrap()[4 + 4 * 20] = 3; // track 21's count
    let mut plan = d.repair_plan();
    want(
        &plan,
        &[
            ("Mark 2 blocks in use but listed as free", true),
            ("Free 1 block that nothing uses", true),
            ("Correct the free count of 3 tracks", true),
        ],
    );
    assert_eq!(d.repair(&plan).unwrap(), 3);
    assert_eq!(d.as_bytes(), good.as_bytes());

    // Only what's ticked is applied.
    flip(&mut d, (30, 5));
    plan = d.repair_plan();
    choose(&mut plan, |f| !matches!(f, Fix::FreeLost { .. }));
    d.repair(&plan).unwrap();
    assert_eq!(d.repair_plan().choices.len(), 1);
}

#[test]
fn ends_a_broken_chain_and_keeps_what_can_be_read() {
    for bad in [(99, 0), (1, 21), chain(&disk(), "F4")[0]] {
        let mut d = disk();
        let before = read(&d, "F4");
        let blocks = chain(&d, "F4");
        link(&mut d, blocks[9], bad);
        let plan = d.repair_plan();
        assert!(
            matches!(&plan.choices[0].fix, Fix::EndFile { name, keep: 10, .. } if name == "F4"),
            "{plan:?}"
        );
        // The rest of the file is now lost blocks, kept by default.
        assert!(matches!(&plan.choices[1].fix, Fix::FreeLost { blocks } if blocks.len() == 10));
        assert!(!plan.choices[1].apply);
        d.repair(&plan).unwrap();
        assert_eq!(read(&d, "F4"), &before[..10 * 254]);
        assert_eq!(d.find_entry("F4").unwrap().unwrap().blocks, 10);
        let plan = d.repair_plan();
        want(&plan, &[("Free 10 blocks that nothing uses", true)]);
        d.repair(&plan).unwrap();
        clean(&d);
    }
}

#[test]
fn a_wiped_block_is_not_taken_for_the_end_of_a_file() {
    // Seen on a real disk: a file's first block was all zeros, so its chain
    // was sound but one block long, and the rest looked unused.
    let mut d = disk();
    let blocks = chain(&d, "F4");
    let (t, s) = blocks[0];
    d.block_mut(t, s).unwrap().fill(0);
    let plan = d.repair_plan();
    want(&plan, &[("Free 19 blocks that nothing uses", false)]);
    assert!(plan.notes[0].starts_with("\"F4\" lists 20 blocks but only 1 can be reached"));
}

#[test]
fn a_chain_into_the_directory_ends_before_it() {
    let mut d = disk();
    let blocks = chain(&d, "F2");
    link(&mut d, blocks[2], (18, 1));
    let mut dir = d.block(18, 1).unwrap().to_vec();
    dir[2 * 32 + 30] = 3; // only F2's block count changes
    d.repair(&d.repair_plan()).unwrap();
    assert_eq!(chain(&d, "F2"), &blocks[..3]);
    assert_eq!(d.block(18, 1).unwrap(), dir);
}

#[test]
fn removes_an_entry_that_starts_off_the_disk() {
    let mut d = disk();
    d.block_mut(18, 1).unwrap()[3] = 77; // F0's start track
    let mut plan = d.repair_plan();
    assert!(matches!(&plan.choices[0].fix, Fix::RemoveFile { name, .. } if name == "F0"));
    assert!(plan.choices[0].apply);
    d.repair(&plan).unwrap();
    assert_eq!(d.directory().unwrap().len(), 4);
    plan = d.repair_plan();
    d.repair(&plan).unwrap();
    clean(&d);
    assert_eq!(read(&d, "F4"), data(4, 5000));
}

#[test]
fn unclosed_files_are_kept_unless_chosen() {
    let mut d = disk();
    d.block_mut(18, 1).unwrap()[2] = 0x02; // F0: PRG, not closed
    let mut plan = d.repair_plan();
    want(
        &plan,
        &[(
            "\"F0\": remove from the directory (it was never closed)",
            false,
        )],
    );
    choose(&mut plan, |_| true);
    d.repair(&plan).unwrap();
    assert_eq!(d.directory().unwrap().len(), 4);
    clean(&d); // its blocks were freed
}

#[test]
fn splits_cross_linked_files() {
    let mut d = disk();
    let (f2, f4) = (chain(&d, "F2"), chain(&d, "F4"));
    // F4 runs into F2's tail after 5 blocks.
    link(&mut d, f4[4], f2[3]);
    let f2_bytes = read(&d, "F2");
    let f4_bytes = read(&d, "F4");
    let plan = d.repair_plan();
    assert!(texts(&plan).contains(&(
        "\"F4\": copy the 5 blocks it shares with \"F2\"".into(),
        true
    )));
    d.repair(&plan).unwrap();
    assert_eq!(read(&d, "F2"), f2_bytes);
    assert_eq!(read(&d, "F4"), f4_bytes);
    assert_eq!(chain(&d, "F2"), f2);
    assert_eq!(chain(&d, "F4")[..5], f4[..5]);
    // All that's left are the blocks F4 used to lead to, kept by default.
    want(
        &d.repair_plan(),
        &[("Free 15 blocks that nothing uses", false)],
    );
    assert!(d.validate().unwrap().is_clean());
}

#[test]
fn two_names_for_one_file_are_left_unless_chosen() {
    let mut d = disk();
    let (t, s) = chain(&d, "F2")[0];
    d.write_file("ALIAS", FileType::Prg, b"x").unwrap();
    let old = chain(&d, "ALIAS")[0];
    d.block_mut(18, 1).unwrap()[5 * 32 + 3..5 * 32 + 5].copy_from_slice(&[t, s]);
    flip(&mut d, old); // its own block, now lost
    let mut plan = d.repair_plan();
    let i = plan
        .choices
        .iter()
        .position(|c| matches!(&c.fix, Fix::SplitFile { .. }))
        .unwrap();
    assert!(!plan.choices[i].apply);
    plan.choices[i].apply = true;
    d.repair(&plan).unwrap();
    assert_eq!(read(&d, "ALIAS"), read(&d, "F2"));
    assert_ne!(chain(&d, "ALIAS"), chain(&d, "F2"));
    clean(&d);
}

#[test]
fn splitting_without_room_changes_nothing() {
    let mut d = disk();
    let (f2, f4) = (chain(&d, "F2"), chain(&d, "F4"));
    link(&mut d, f4[4], f2[3]);
    let free = d.blocks_free().unwrap() as usize;
    d.write_file("BIG", FileType::Prg, &vec![0; free * 254])
        .unwrap();
    let mut plan = d.repair_plan();
    let split = plan
        .choices
        .iter()
        .find(|c| matches!(c.fix, Fix::SplitFile { .. }));
    assert!(!split.unwrap().apply);
    choose(&mut plan, |f| matches!(f, Fix::SplitFile { .. }));
    let before = d.as_bytes().to_vec();
    assert!(d.repair(&plan).is_err());
    assert_eq!(d.as_bytes(), before);
}

#[test]
fn removing_a_file_spares_blocks_it_shared() {
    let mut d = disk();
    let (f2, f4) = (chain(&d, "F2"), chain(&d, "F4"));
    link(&mut d, f4[4], f2[3]);
    d.block_mut(18, 1).unwrap()[4 * 32 + 2] = 0x02; // F4 unclosed
    let f2_bytes = read(&d, "F2");
    let mut plan = d.repair_plan();
    choose(&mut plan, |f| matches!(f, Fix::RemoveFile { .. }));
    d.repair(&plan).unwrap();
    assert_eq!(read(&d, "F2"), f2_bytes);
    let map = d.block_map();
    assert!(f2
        .iter()
        .all(|&(t, s)| matches!(map.get(t, s).unwrap().status(), BlockStatus::File(_))));
}

#[test]
fn ends_a_broken_directory_without_freeing_what_it_lost() {
    let mut d = Disk::format("DIR", "D1");
    for i in 0..20 {
        d.write_file(&format!("F{i}"), FileType::Prg, &[i]).unwrap();
    }
    let (t, s) = (18, d.block(18, 1).unwrap()[1]); // second directory block
    link(&mut d, (t, s), (77, 7));
    assert!(d.directory().is_err());
    let mut plan = d.repair_plan();
    assert!(matches!(
        plan.choices[0].fix,
        Fix::EndDirectory { blocks: 2, .. }
    ));
    let lost = plan
        .choices
        .iter()
        .find(|c| matches!(c.fix, Fix::FreeLost { .. }))
        .unwrap();
    assert!(!lost.apply);
    assert!(!plan.notes.is_empty());
    let before = d.clone();
    d.repair(&plan).unwrap();
    assert_eq!(d.directory().unwrap().len(), 16);
    assert_eq!(read(&d, "F15"), [15]);
    // Nothing else can be done while the directory is left broken.
    d = before.clone();
    choose(&mut plan, |f| matches!(f, Fix::FreeLost { .. }));
    assert!(d.repair(&plan).is_err());
    assert_eq!(d.as_bytes(), before.as_bytes());
}

#[test]
fn directory_art_is_left_alone() {
    let mut d = disk();
    for (k, (t, s)) in [(18, 0), (99, 9), (1, 0), (0, 0), (25, 3)]
        .into_iter()
        .enumerate()
    {
        let b = d.block_mut(18, 1).unwrap();
        // Only three slots are left in 18/1.
        let Some(slot) = b.get_mut((5 + k) * 32..(6 + k) * 32) else {
            continue;
        };
        slot[2] = 0x80; // DEL, closed
        slot[3..5].copy_from_slice(&[t, s]);
        slot[5..21].copy_from_slice(b"----------------");
    }
    clean(&d);
}

#[test]
fn boot_code_is_not_lost_blocks() {
    let mut d = Disk::format("BOOT", "B1");
    for s in 0..3 {
        flip(&mut d, (1, s));
    }
    d.block_mut(18, 0).unwrap()[4] -= 3;
    d.block_mut(1, 0).unwrap()[..7].copy_from_slice(b"CBM\0\x0b\0\x01");
    let plan = d.repair_plan();
    assert!(
        matches!(&plan.choices[0].fix, Fix::FreeLost { blocks } if blocks == &[(1, 2)]),
        "{plan:?}"
    );
    assert_eq!(plan.notes, ["2 blocks of C128 boot code left alone"]);
}

#[test]
fn hidden_files_keep_their_blocks() {
    // A GEOS disk with a file on the border.
    let mut d = disk();
    let h = d.block_mut(18, 0).unwrap();
    h[0xAB..0xAD].copy_from_slice(&[19, 0]);
    h[0xAD..0xB8].copy_from_slice(b"GEOS format");
    flip(&mut d, (19, 0));
    d.block_mut(18, 0).unwrap()[4 + 4 * 18] -= 1;
    clean(&d);
    d.block_mut(19, 0).unwrap()[2] = 0x83;
    flip(&mut d, (20, 0));
    d.block_mut(18, 0).unwrap()[4 + 4 * 19] -= 1;
    let plan = d.repair_plan();
    assert!(plan.choices.is_empty(), "{plan:?}");
    assert!(plan.notes[0].contains("GEOS border"), "{plan:?}");

    // A single-sided disk in a D71: nothing on the second side counts.
    let mut d = Disk::format_d71("R", "R1");
    d.block_mut(18, 0).unwrap()[3] = 0;
    for b in &mut d.block_mut(18, 0).unwrap()[0xDD..] {
        *b = 0;
    }
    d.block_mut(53, 0).unwrap().fill(0);
    clean(&d);
}

#[test]
fn super_side_sectors_own_every_group() {
    let mut d = Disk::format_d81("REL", "R1");
    d.write_file("REL", FileType::Prg, &[1; 300]).unwrap();
    d.write_file("SIDE", FileType::Seq, &[0; 700]).unwrap();
    let side = chain(&d, "SIDE");
    // Turn SIDE's three blocks into a super side sector and two groups.
    let mut sup = vec![0u8; 256];
    (sup[0], sup[1], sup[2]) = (side[1].0, side[1].1, 0xFE);
    for (i, &(t, s)) in side[1..].iter().enumerate() {
        sup[3 + i * 2..5 + i * 2].copy_from_slice(&[t, s]);
    }
    d.block_mut(side[0].0, side[0].1)
        .unwrap()
        .copy_from_slice(&sup);
    link(&mut d, side[1], (0, 0xFF));
    let dir = d.block_mut(40, 3).unwrap();
    dir[2] = 0x84;
    dir[21..24].copy_from_slice(&[side[0].0, side[0].1, 10]);
    dir[32 + 2] = 0; // SIDE's own entry
    let plan = d.repair_plan();
    assert!(plan.choices.is_empty(), "{plan:?}");
}

#[test]
fn the_rebuilt_bam_matches_c1541s() {
    if Command::new("c1541").arg("-help").output().is_err() {
        eprintln!("c1541 not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("s64-repair-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let images = [
        ("d64", Disk::format("R", "R1")),
        ("d71", Disk::format_d71("R", "R1")),
        ("d81", Disk::format_d81("R", "R1")),
    ];
    for (ext, mut good) in images {
        fill(&mut good);
        let geom = good.geometry();
        let mut d = good.clone();
        // Scramble the bitmaps and counts of the tracks that hold files.
        let (bt, bs, at) = match ext {
            "d81" => (40, 1, 0x10),
            _ => (18, 0, 4),
        };
        for (i, b) in d.block_mut(bt, bs).unwrap()[at..at + 60]
            .iter_mut()
            .enumerate()
        {
            *b ^= [0x15, 0x2A, 0x07, 0x08][i % 4];
        }
        let path = dir.join(format!("r.{ext}"));
        std::fs::write(&path, d.as_bytes()).unwrap();
        let o = Command::new("c1541")
            .arg(&path)
            .arg("-validate")
            .output()
            .unwrap();
        assert!(o.status.success());
        let theirs = Disk::load(std::fs::read(&path).unwrap()).unwrap();
        let plan = d.repair_plan();
        assert!(!plan.choices.is_empty(), "{ext}");
        d.repair(&plan).unwrap();
        assert_eq!(d.blocks_free(), theirs.blocks_free(), "{ext}");
        let (ours, theirs) = (d.block_map(), theirs.block_map());
        for t in 1..=geom.tracks() {
            for s in 0..geom.sector_count(t).unwrap() {
                let (a, b) = (
                    ours.get(t, s as u8).unwrap(),
                    theirs.get(t, s as u8).unwrap(),
                );
                assert_eq!(a.bam, b.bam, "{ext} {t}/{s}");
            }
        }
        clean(&d);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Random damage: nothing panics, undamaged files keep their bytes, and
/// repairing until there's nothing left switched on reaches an end.
#[test]
fn random_damage() {
    let mut seed = 0x5EC7_0064_u64;
    let mut rnd = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 16) as usize % n
    };
    for round in 0..600 {
        let mut d = match round % 3 {
            0 => Disk::format("X", "X1"),
            1 => Disk::format_d71("X", "X1"),
            _ => Disk::format_d81("X", "X1"),
        };
        fill(&mut d);
        let files: Vec<_> = (0..5).map(|i| chain(&d, &format!("F{i}"))).collect();
        let dir = d.geometry().directory_track();
        for _ in 0..1 + rnd(6) {
            // A byte of a file's link, the BAM or the directory.
            let (t, s, at) = match rnd(4) {
                0 | 1 => {
                    let f = &files[rnd(5)];
                    let (t, s) = f[rnd(f.len())];
                    (t, s, rnd(2))
                }
                2 => (dir, rnd(3) as u8, rnd(256)),
                _ => (dir, d.geometry().directory_first_sector(), rnd(256)),
            };
            let v = match rnd(2) {
                0 => rnd(256) as u8,
                _ => rnd(42) as u8,
            };
            d.block_mut(t, s).unwrap()[at] = v;
        }
        // Files that are whole and share nothing must come through intact.
        let map = d.block_map();
        let whole: Vec<_> = map
            .files
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                e.closed
                    && matches!(e.file_type, FileType::Prg)
                    && !map.chains[*i].is_empty()
                    && d.read_entry(e).is_ok()
                    && map.chains[*i]
                        .iter()
                        .all(|&(t, s)| !map.get(t, s).unwrap().shared)
            })
            .map(|(_, e)| (e.name_raw, d.read_entry(e).unwrap()))
            .collect();
        let mut left = 10;
        loop {
            let plan = d.repair_plan();
            if !plan.choices.iter().any(|c| c.apply) {
                break;
            }
            d.repair(&plan).unwrap();
            left -= 1;
            assert!(left > 0, "round {round}: {plan:?}");
        }
        let after = d.directory().unwrap();
        for (name, bytes) in whole {
            assert!(
                after
                    .iter()
                    .any(|e| e.name_raw == name && d.read_entry(e).as_ref() == Ok(&bytes)),
                "round {round}: {name:?} changed"
            );
        }
        // Nothing in use is left free, so nothing can be overwritten.
        let unsafe_blocks = d
            .block_map()
            .tracks
            .iter()
            .flatten()
            .filter(|b| b.owner != sector64::Owner::None && b.bam == sector64::BamState::Free)
            .count();
        let single_sided = round % 3 == 1 && d.block(18, 0).unwrap()[3] & 0x80 == 0;
        assert!(unsafe_blocks == 0 || single_sided, "round {round}");
        // With every fix on, it may fail but not panic or half-apply.
        let mut plan = d.repair_plan();
        choose(&mut plan, |_| true);
        let before = d.as_bytes().to_vec();
        if d.repair(&plan).is_err() {
            assert_eq!(d.as_bytes(), before);
        }
    }
}
