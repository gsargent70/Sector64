//! A block-by-block map of a disk: which file (or DOS structure) owns each
//! block, and whether the BAM agrees.
//!
//! [`Disk::block_map`] walks the directory, every file's block chain (plus REL
//! side sectors, GEOS info blocks and VLIR records, and 1581 partitions), and
//! the DOS's own blocks (header, BAM, directory), then reads the BAM state of
//! every block. Unlike [`Disk::validate`] it never fails on a damaged disk: a
//! chain that breaks or loops stops there, the blocks walked so far are still
//! mapped, and the fault is listed in [`BlockMap::problems`].
//!
//! [`Disk::validate`]: crate::Disk::validate

use crate::directory::{Entry, FileType};
use crate::disk::Disk;
use crate::geometry::Geometry;

/// Who a block belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// Nothing references the block.
    None,
    /// The DOS itself: header, BAM or directory.
    System,
    /// A file, by its index in [`BlockMap::files`] (directory order).
    File(usize),
}

/// What the BAM says about a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BamState {
    Free,
    Allocated,
    /// The BAM doesn't cover this track (e.g. tracks 36–40 of a 40-track disk
    /// whose DOS wasn't recognised).
    Unknown,
}

/// One block of the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapBlock {
    /// The first owner found; see [`Self::shared`] for any others.
    pub owner: Owner,
    pub bam: BamState,
    /// More than one file (or a file and the DOS) claims this block.
    pub shared: bool,
}

/// How a block should be shown: its owner and the BAM taken together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStatus {
    /// Unused and free in the BAM.
    Free,
    /// Used by a file and allocated in the BAM.
    File(usize),
    /// Used by the DOS (header, BAM, directory).
    System,
    /// Allocated in the BAM but nothing references it (lost blocks; also
    /// how some copy protections and boot loaders hide data).
    Orphan,
    /// In use but marked free in the BAM, or claimed twice: the next file
    /// written could overwrite it.
    Conflict,
}

impl MapBlock {
    /// The block's state for display.
    pub fn status(&self) -> BlockStatus {
        match (self.owner, self.bam) {
            _ if self.shared => BlockStatus::Conflict,
            (Owner::None, BamState::Allocated) => BlockStatus::Orphan,
            (Owner::None, _) => BlockStatus::Free,
            (Owner::System, BamState::Free) | (Owner::File(_), BamState::Free) => {
                BlockStatus::Conflict
            }
            (Owner::System, _) => BlockStatus::System,
            (Owner::File(i), _) => BlockStatus::File(i),
        }
    }
}

/// Every block of a disk, by track and sector.
#[derive(Debug, Clone)]
pub struct BlockMap {
    /// `tracks[t - 1][s]` is track `t`, sector `s`. Track lengths follow the
    /// disk's zones (e.g. 21 to 17 sectors on a 1541).
    pub tracks: Vec<Vec<MapBlock>>,
    /// The directory entries, in directory order; [`Owner::File`] indexes this.
    pub files: Vec<Entry>,
    /// Each file's blocks in the order they were reached: its data chain (or
    /// VLIR index then records), then any REL side sectors or GEOS info
    /// block. Includes blocks another file got to first.
    pub chains: Vec<Vec<(u8, u8)>>,
    /// Chain faults found while mapping, one line each (e.g. a link that
    /// points off the disk). Empty for a healthy disk.
    pub problems: Vec<String>,
}

impl BlockMap {
    /// The block at (track, sector), if the disk has it.
    pub fn get(&self, track: u8, sector: u8) -> Option<&MapBlock> {
        self.tracks
            .get((track as usize).checked_sub(1)?)?
            .get(sector as usize)
    }

    /// Number of blocks in each [`BlockStatus`] class, as
    /// (free, file, system, orphan, conflict).
    pub fn counts(&self) -> (usize, usize, usize, usize, usize) {
        let mut c = (0, 0, 0, 0, 0);
        for b in self.tracks.iter().flatten() {
            match b.status() {
                BlockStatus::Free => c.0 += 1,
                BlockStatus::File(_) => c.1 += 1,
                BlockStatus::System => c.2 += 1,
                BlockStatus::Orphan => c.3 += 1,
                BlockStatus::Conflict => c.4 += 1,
            }
        }
        c
    }

    /// The (track, sector) blocks owned by file `index` (including ones it
    /// shares), in disk order.
    pub fn blocks_of(&self, index: usize) -> Vec<(u8, u8)> {
        let mut out = Vec::new();
        for (t, row) in self.tracks.iter().enumerate() {
            for (s, b) in row.iter().enumerate() {
                if b.owner == Owner::File(index) {
                    out.push((t as u8 + 1, s as u8));
                }
            }
        }
        out
    }

    /// The character [`Self::to_text`] draws a block with: `.` free, `=`
    /// the DOS, `?` allocated but unused, `!` a conflict, and a file's own
    /// symbol ([`file_symbol`]) for its blocks.
    pub fn block_char(block: &MapBlock) -> char {
        match block.status() {
            BlockStatus::Free => '.',
            BlockStatus::System => '=',
            BlockStatus::Orphan => '?',
            BlockStatus::Conflict => '!',
            BlockStatus::File(i) => file_symbol(i),
        }
    }

    /// The map as text: one line per track (wrapped every 64 sectors),
    /// sector numbers across the top, then a legend naming each file's
    /// symbol, the totals, and any problems.
    pub fn to_text(&self) -> String {
        let max = self.tracks.iter().map(Vec::len).max().unwrap_or(0);
        let width = max.clamp(1, 64);
        let mut out = String::new();
        // Sector numbers, as tens over units.
        let tens: String = (0..width)
            .map(|s| {
                if s % 10 == 0 {
                    char::from(b'0' + (s / 10 % 10) as u8)
                } else {
                    ' '
                }
            })
            .collect();
        out.push_str("       ");
        out.push_str(tens.trim_end());
        out.push_str("\n  trk  ");
        out.extend((0..width).map(|s| char::from(b'0' + (s % 10) as u8)));
        out.push('\n');
        for (t, row) in self.tracks.iter().enumerate() {
            for (part, chunk) in row.chunks(width).enumerate() {
                if part == 0 {
                    out.push_str(&format!("  {:>3}  ", t + 1));
                } else {
                    out.push_str(&format!(" +{:<3}  ", part * width));
                }
                out.extend(chunk.iter().map(Self::block_char));
                out.push('\n');
            }
        }
        out.push_str("\n  .  free               =  DOS (header, BAM, directory)\n");
        out.push_str(
            "  ?  allocated, unused   !  conflict (in use but free in the BAM, or shared)\n",
        );
        for (i, e) in self.files.iter().enumerate() {
            let blocks = self.blocks_of(i).len();
            out.push_str(&format!(
                "  {}  {:<18} {}  {blocks} block{}\n",
                file_symbol(i),
                format!("\"{}\"", e.name),
                e.file_type.mnemonic(),
                if blocks == 1 { "" } else { "s" }
            ));
        }
        let (free, file, sys, orphan, conflict) = self.counts();
        out.push_str(&format!(
            "\n{} blocks: {file} in files, {sys} DOS, {free} free, {orphan} allocated but unused, {conflict} in conflict\n",
            free + file + sys + orphan + conflict
        ));
        for p in &self.problems {
            out.push_str(&format!("problem: {p}\n"));
        }
        out
    }

    fn claim(&mut self, track: u8, sector: u8, owner: Owner) {
        let Some(b) = (track as usize)
            .checked_sub(1)
            .and_then(|t| self.tracks.get_mut(t))
            .and_then(|row| row.get_mut(sector as usize))
        else {
            return;
        };
        match b.owner {
            Owner::None => b.owner = owner,
            o if o == owner => {} // a file revisiting its own block is a cycle, reported separately
            _ => b.shared = true,
        }
    }
}

impl Disk {
    /// Map every block of the disk to its owner and BAM state. Never fails:
    /// damage is mapped as far as it can be and listed in
    /// [`BlockMap::problems`].
    pub fn block_map(&self) -> BlockMap {
        let geom = self.geometry();
        let tracks = (1..=geom.tracks())
            .map(|t| {
                (0..geom.sector_count(t).unwrap_or(0))
                    .map(|s| MapBlock {
                        owner: Owner::None,
                        bam: match self.bam_is_allocated(t, s as u8) {
                            Ok(true) => BamState::Allocated,
                            Ok(false) => BamState::Free,
                            Err(_) => BamState::Unknown,
                        },
                        shared: false,
                    })
                    .collect()
            })
            .collect();
        let mut map = BlockMap {
            tracks,
            files: Vec::new(),
            chains: Vec::new(),
            problems: Vec::new(),
        };

        // The DOS's own blocks.
        let dir = geom.directory_track();
        let system: Vec<(u8, u8)> = match geom {
            Geometry::Cbm1541 { .. } => vec![(dir, 0)],
            // The 1571 reserves all of track 53 (its BAM is in 53/0).
            Geometry::Cbm1571 => std::iter::once((dir, 0))
                .chain((0..geom.sector_count(53).unwrap_or(0)).map(|s| (53, s as u8)))
                .collect(),
            Geometry::Cbm1581 => vec![(dir, 0), (dir, 1), (dir, 2)],
            // CMD native: boot block, header and BAM fill 1/0 to 1/33.
            Geometry::CmdNative { .. } => (0..34).map(|s| (1, s)).collect(),
        };
        for (t, s) in system {
            map.claim(t, s, Owner::System);
        }
        let (chain, fault) = self.walk(dir, geom.directory_first_sector());
        for &(t, s) in &chain {
            map.claim(t, s, Owner::System);
        }
        if let Some(f) = fault {
            map.problems.push(format!("directory: {f}"));
        }

        let header = self.block(dir, geom.header_sector()).ok();
        let geos = self.is_geos();
        if geos {
            // The GEOS border block holds files dragged off the desktop. Some
            // disks leave it free in the BAM (seen on a 1581 GEOS disk whose
            // border held only a deleted entry); count it only when allocated.
            if let Some(h) = header {
                let (t, s) = (h[0xAB], h[0xAC]);
                if t != 0 && self.bam_is_allocated(t, s) == Ok(true) {
                    map.claim(t, s, Owner::System);
                }
            }
        }

        map.files = self.directory().unwrap_or_default();
        map.chains = vec![Vec::new(); map.files.len()];
        for i in 0..map.files.len() {
            let e = map.files[i].clone();
            if self.is_placeholder(&e) {
                continue;
            }
            let owner = Owner::File(i);
            let mut faults = Vec::new();
            let mut take = |map: &mut BlockMap, (chain, fault): (Vec<(u8, u8)>, Option<String>)| {
                for (t, s) in chain {
                    map.claim(t, s, owner);
                    map.chains[i].push((t, s));
                }
                faults.extend(fault);
            };

            if e.file_type == FileType::Other(5) && matches!(geom, Geometry::Cbm1581) {
                // A 1581 partition: `blocks` contiguous blocks from its start.
                take(
                    &mut map,
                    (self.run(e.first_track, e.first_sector, e.blocks), None),
                );
            } else if geos && e.rel_record_len == 1 && e.file_type != FileType::Rel {
                // GEOS VLIR: the start block is an index of record chains.
                take(&mut map, (vec![(e.first_track, e.first_sector)], None));
                if let Ok(index) = self.block(e.first_track, e.first_sector) {
                    let index = index.to_vec();
                    for pair in index[2..].chunks_exact(2) {
                        if pair[0] == 0 {
                            if pair[1] == 0 {
                                break; // end of records
                            }
                            continue; // an empty record
                        }
                        take(&mut map, self.walk(pair[0], pair[1]));
                    }
                } else {
                    let off = format!(
                        "VLIR index {}/{} is off the disk",
                        e.first_track, e.first_sector
                    );
                    take(&mut map, (Vec::new(), Some(off)));
                }
            } else {
                take(&mut map, self.walk(e.first_track, e.first_sector));
            }

            if e.file_type == FileType::Rel {
                if e.side_track != 0 {
                    take(&mut map, self.walk(e.side_track, e.side_sector));
                    // Drives past the 1571 start with a super side sector,
                    // which lists the groups of side sectors. Its own link
                    // leads to the first group.
                    let groups = match self.block(e.side_track, e.side_sector) {
                        Ok(b) if b[2] == 0xFE => b[3..255].to_vec(),
                        _ => Vec::new(),
                    };
                    for pair in groups.chunks_exact(2).skip(1) {
                        if pair[0] == 0 {
                            break;
                        }
                        take(&mut map, self.walk(pair[0], pair[1]));
                    }
                }
            } else if geos && e.side_track != 0 {
                // The GEOS info block (icon, description).
                take(&mut map, (vec![(e.side_track, e.side_sector)], None));
            }
            for f in faults {
                map.problems.push(format!("\"{}\": {f}", e.name));
            }
        }
        // Placeholders own only what's plainly theirs: a sound chain of
        // allocated blocks that nothing else uses.
        for i in 0..map.files.len() {
            let e = &map.files[i];
            if !self.is_placeholder(e) {
                continue;
            }
            let (chain, fault) = self.walk(e.first_track, e.first_sector);
            let theirs = |&(t, s): &(u8, u8)| {
                map.get(t, s)
                    .is_some_and(|b| b.owner == Owner::None && b.bam == BamState::Allocated)
            };
            if fault.is_none() && chain.iter().all(theirs) {
                for &(t, s) in &chain {
                    map.claim(t, s, Owner::File(i));
                }
                map.chains[i] = chain;
            }
        }
        map
    }

    /// An entry that's there to be seen rather than loaded, such as a line of
    /// directory art: no blocks, and either type DEL or pointing at the
    /// header or directory. Where it points means nothing.
    pub(crate) fn is_placeholder(&self, e: &Entry) -> bool {
        let geom = self.geometry();
        let in_dir = e.first_track == geom.directory_track()
            && (e.first_sector == geom.header_sector()
                || e.first_sector == geom.directory_first_sector());
        e.blocks == 0 && (e.file_type == FileType::Del || in_dir)
    }

    /// Follow a T/S chain leniently: the blocks reached, and why the walk
    /// stopped early (if it did).
    pub(crate) fn walk(&self, track: u8, sector: u8) -> (Vec<(u8, u8)>, Option<String>) {
        let mut chain: Vec<(u8, u8)> = Vec::new();
        if track == 0 {
            return (chain, None);
        }
        let max = self.geometry().total_blocks() as usize;
        let (mut t, mut s) = (track, sector);
        loop {
            if chain.contains(&(t, s)) {
                return (chain, Some(format!("chain loops back to {t}/{s}")));
            }
            if chain.len() >= max {
                return (chain, Some("chain is longer than the disk".into()));
            }
            let Ok(block) = self.block(t, s) else {
                return (chain, Some(format!("link to {t}/{s} is off the disk")));
            };
            chain.push((t, s));
            if block[0] == 0 {
                return (chain, None);
            }
            (t, s) = (block[0], block[1]);
        }
    }

    /// `count` blocks in disk order from (track, sector), stopping at the end
    /// of the disk.
    pub(crate) fn run(&self, track: u8, sector: u8, count: u16) -> Vec<(u8, u8)> {
        let geom = self.geometry();
        let mut out = Vec::new();
        let (mut t, mut s) = (track, sector as u16);
        while out.len() < count as usize {
            let Some(n) = geom.sector_count(t) else { break };
            if s >= n {
                t = t.wrapping_add(1);
                s = 0;
                continue;
            }
            out.push((t, s as u8));
            s += 1;
        }
        out
    }
}

/// A file's symbol in [`BlockMap::to_text`]: `A`–`Z`, `a`–`z`, `0`–`9` in
/// directory order, then `*` for the 63rd file on.
pub fn file_symbol(index: usize) -> char {
    const SYMBOLS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    SYMBOLS.get(index).map_or('*', |&b| char::from(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directory::FileType;

    #[test]
    fn text_map_marks_each_files_blocks_with_its_symbol() {
        let mut disk = Disk::format("text map", "tm");
        disk.write_file("ONE", FileType::Prg, &[1; 600]).unwrap();
        disk.write_file("TWO", FileType::Seq, b"x").unwrap();
        let text = disk.block_map().to_text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "       0         1         2");
        assert_eq!(lines[1], "  trk  012345678901234567890");
        // This crate's write_file fills from track 1.
        assert_eq!(lines[2], format!("    1  AAAB{}", ".".repeat(17)));
        // Track 18: header/BAM and the first directory block, then free.
        assert_eq!(lines[19], format!("   18  =={}", ".".repeat(17)));
        // Track 25 has 18 sectors, so its line is shorter.
        assert_eq!(lines[26].len(), 7 + 18);
        let count = |c: char| {
            text.lines()
                .take(37)
                .flat_map(str::chars)
                .filter(|&x| x == c)
                .count()
        };
        assert_eq!((count('A'), count('B'), count('=')), (3, 1, 2));
        assert!(text.contains("  A  \"ONE\"              prg  3 blocks\n"));
        assert!(text.contains("  B  \"TWO\"              seq  1 block\n"));
        assert!(text.contains(
            "\n683 blocks: 4 in files, 2 DOS, 677 free, 0 allocated but unused, 0 in conflict\n"
        ));
        assert_eq!(
            [
                file_symbol(0),
                file_symbol(25),
                file_symbol(26),
                file_symbol(61),
                file_symbol(62)
            ],
            ['A', 'Z', 'a', '9', '*']
        );
    }

    #[test]
    fn blank_disk_is_free_except_the_dos_blocks() {
        let map = Disk::format("blank", "01").block_map();
        assert_eq!(map.tracks.len(), 35);
        assert_eq!(map.tracks[0].len(), 21);
        assert_eq!(map.tracks[34].len(), 17);
        assert_eq!(map.get(18, 0).unwrap().status(), BlockStatus::System);
        assert_eq!(map.get(18, 1).unwrap().status(), BlockStatus::System);
        assert_eq!(map.get(1, 0).unwrap().status(), BlockStatus::Free);
        assert!(map.get(36, 0).is_none() && map.get(0, 0).is_none());
        assert_eq!(map.counts(), (683 - 2, 0, 2, 0, 0));
        assert!(map.problems.is_empty());
    }

    #[test]
    fn files_own_their_chains() {
        let mut disk = Disk::format("files", "01");
        disk.write_file("ONE", FileType::Prg, &[1u8; 600]).unwrap(); // 3 blocks
        disk.write_file("TWO", FileType::Seq, &[2u8; 10]).unwrap(); // 1 block
        let map = disk.block_map();
        assert_eq!(map.files.len(), 2);
        let one = map.blocks_of(0);
        assert_eq!(one.len(), 3);
        assert_eq!(map.chains[0].len(), 3);
        assert_eq!(
            map.chains[0][0],
            (map.files[0].first_track, map.files[0].first_sector)
        );
        assert_eq!(map.blocks_of(1).len(), 1);
        for (t, s) in one {
            assert_eq!(map.get(t, s).unwrap().status(), BlockStatus::File(0));
        }
        assert_eq!(map.counts(), (683 - 6, 4, 2, 0, 0));
    }

    #[test]
    fn bam_disagreements_show_as_orphans_and_conflicts() {
        let mut disk = Disk::format("bad", "01");
        disk.write_file("A", FileType::Prg, &[1u8; 10]).unwrap();
        let (t, s) = disk.block_map().blocks_of(0)[0];
        // Free the file's block in the BAM, and allocate an unused one.
        disk.bam_set(t, s, false);
        disk.bam_set(1, 5, true);
        let map = disk.block_map();
        assert_eq!(map.get(t, s).unwrap().status(), BlockStatus::Conflict);
        assert_eq!(map.get(1, 5).unwrap().status(), BlockStatus::Orphan);
    }

    #[test]
    fn cross_links_and_broken_chains_are_mapped_not_fatal() {
        let mut disk = Disk::format("broken", "01");
        disk.write_file("A", FileType::Prg, &[1u8; 600]).unwrap();
        disk.write_file("B", FileType::Prg, &[2u8; 600]).unwrap();
        let map = disk.block_map();
        let (a, b) = (map.blocks_of(0), map.blocks_of(1));
        // Point A's first block at B's second: they now share B's tail.
        let blk = disk.block_mut(a[0].0, a[0].1).unwrap();
        blk[0] = b[1].0;
        blk[1] = b[1].1;
        // And B's last block at a sector that doesn't exist.
        let last = disk.block_mut(b[2].0, b[2].1).unwrap();
        last[0] = 18;
        last[1] = 99;
        let map = disk.block_map();
        assert_eq!(
            map.get(b[1].0, b[1].1).unwrap().status(),
            BlockStatus::Conflict
        );
        assert_eq!(map.problems.len(), 2, "{:?}", map.problems);
        assert!(map.problems.iter().all(|p| p.contains("18/99")));
        assert!(
            disk.validate().is_err(),
            "validate gives up where the map doesn't"
        );
    }

    #[test]
    fn d71_reserves_track_53_and_d81_its_bam() {
        let map = Disk::format_d71("d71", "01").block_map();
        assert_eq!(map.tracks.len(), 70);
        assert!(map.tracks[52]
            .iter()
            .all(|b| b.status() == BlockStatus::System));
        let map = Disk::format_d81("d81", "01").block_map();
        assert_eq!(map.tracks.len(), 80);
        for s in 0..4 {
            assert_eq!(
                map.get(40, s).unwrap().status(),
                BlockStatus::System,
                "40/{s}"
            );
        }
        assert!(map.problems.is_empty());
    }
}
