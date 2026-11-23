//! Repairing a disk: what a check finds wrong, as a list of fixes to choose
//! from, and applying the chosen ones.
//!
//! This does what the 1541's VALIDATE does (rebuild the block map from the
//! files, free lost blocks) and more: it ends broken chains at the last
//! readable block rather than giving up, gives cross-linked files their own
//! copies of the blocks they share, and mends a broken directory chain.
//!
//! [`Disk::repair_plan`] only reads. [`Disk::repair`] applies a plan whole or
//! not at all. A fix that could lose data someone wanted starts switched off.

use crate::blockmap::{BamState, BlockMap, Owner};
use crate::directory::{Entry, FileType, SLOTS_PER_SECTOR, SLOT_SIZE};
use crate::disk::Disk;
use crate::error::{Error, Result};
use crate::geometry::Geometry;

/// One thing that can be fixed. Files are named by their index in the
/// directory (as it is once [`Fix::EndDirectory`] has been applied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// The directory's chain of blocks is broken: end it at its last
    /// readable block.
    EndDirectory { blocks: usize, reason: String },
    /// A file's chain is broken: end it after `keep` blocks.
    EndFile {
        file: usize,
        name: String,
        keep: usize,
        reason: String,
    },
    /// Remove a file's directory entry, and free blocks only it used.
    RemoveFile {
        file: usize,
        name: String,
        reason: String,
    },
    /// A file shares blocks with another: give it its own copy of them.
    SplitFile {
        file: usize,
        name: String,
        with: String,
        blocks: usize,
    },
    /// Blocks in use but marked free: mark them used.
    MarkUsed { blocks: usize },
    /// Blocks marked used that nothing uses: free them.
    FreeLost { blocks: Vec<(u8, u8)> },
    /// Tracks whose free count doesn't match their bitmap: recount.
    Recount { tracks: usize },
}

impl std::fmt::Display for Fix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fix::EndDirectory { blocks, reason } => write!(
                f,
                "End the directory after {} ({reason})",
                count(*blocks, "block")
            ),
            Fix::EndFile {
                name, keep, reason, ..
            } => write!(
                f,
                "\"{name}\": keep the first {} ({reason})",
                count(*keep, "block")
            ),
            Fix::RemoveFile { name, reason, .. } => {
                write!(f, "\"{name}\": remove from the directory ({reason})")
            }
            Fix::SplitFile {
                name, with, blocks, ..
            } => write!(
                f,
                "\"{name}\": copy the {} it shares with \"{with}\"",
                count(*blocks, "block")
            ),
            Fix::MarkUsed { blocks } => {
                write!(
                    f,
                    "Mark {} in use but listed as free",
                    count(*blocks, "block")
                )
            }
            Fix::FreeLost { blocks } => {
                write!(f, "Free {} that nothing uses", count(blocks.len(), "block"))
            }
            Fix::Recount { tracks } => {
                write!(f, "Correct the free count of {}", count(*tracks, "track"))
            }
        }
    }
}

/// A fix, and whether to apply it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub fix: Fix,
    pub apply: bool,
}

/// Everything a check found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepairPlan {
    pub choices: Vec<Choice>,
    /// Things that were found but can't be fixed, or were left alone.
    pub notes: Vec<String>,
}

impl RepairPlan {
    /// Whether both plans hold the same fixes, however they're switched.
    pub fn same_fixes(&self, other: &RepairPlan) -> bool {
        self.choices.len() == other.choices.len()
            && self
                .choices
                .iter()
                .zip(&other.choices)
                .all(|(a, b)| a.fix == b.fix)
    }

    fn add(&mut self, fix: Fix, apply: bool) {
        self.choices.push(Choice { fix, apply });
    }

    fn chosen(&self) -> impl Iterator<Item = &Fix> {
        self.choices.iter().filter(|c| c.apply).map(|c| &c.fix)
    }
}

impl Disk {
    /// Check the disk and list what can be fixed, without changing anything.
    pub fn repair_plan(&self) -> RepairPlan {
        let mut plan = RepairPlan::default();
        // Plan the files on the disk as it'll be with its directory mended.
        let mut disk = self.clone();
        let dir = self.geometry().directory_track();
        let (dir_chain, fault) = self.walk(dir, self.geometry().directory_first_sector());
        let dir_broken = fault.is_some();
        if let Some(reason) = fault {
            if disk.end_chain(&dir_chain).is_err() {
                plan.notes
                    .push(format!("the directory can't be read: {reason}"));
                return plan;
            }
            plan.add(
                Fix::EndDirectory {
                    blocks: dir_chain.len(),
                    reason,
                },
                true,
            );
            plan.notes
                .push("files listed after the break in the directory can't be found".into());
        }

        let map = disk.block_map();
        let mut broken = false;
        for (i, e) in map.files.iter().enumerate() {
            if disk.is_placeholder(e) {
                continue;
            }
            broken |= disk.plan_file(&map, i, e, &mut plan);
        }

        let (mut used, mut lost) = (0, Vec::new());
        let keep = disk.boot_blocks();
        let mut kept = 0;
        for (t, s, b) in blocks(&map) {
            if !disk.bam_in_use(t) {
                continue;
            }
            match (b.owner, b.bam) {
                (Owner::None, BamState::Allocated) if keep.contains(&(t, s)) => kept += 1,
                (Owner::None, BamState::Allocated) => lost.push((t, s)),
                (Owner::System | Owner::File(_), BamState::Free) => used += 1,
                _ => {}
            }
        }
        if used > 0 {
            plan.add(Fix::MarkUsed { blocks: used }, true);
        }
        if kept > 0 {
            plan.notes.push(format!(
                "{} of C128 boot code left alone",
                count(kept, "block")
            ));
        }
        if !lost.is_empty() {
            match disk.hidden_files() {
                Some(why) => plan.notes.push(format!(
                    "{} that nothing seems to use left alone: {why}",
                    count(lost.len(), "block")
                )),
                // After damage, these may be the rest of a broken file or of
                // files that fell off the directory.
                None => {
                    // So may they be when a file's chain is sound but stops
                    // short of the size in the directory (a block that was
                    // wiped looks like the end of a file).
                    let mut short = false;
                    for (i, e) in map.files.iter().enumerate() {
                        let reached = map.chains[i].len();
                        if e.closed && reached < usize::from(e.blocks) && !lost_already(&plan, i) {
                            short = true;
                            plan.notes.push(format!(
                                "\"{}\" lists {} but only {} can be reached, so the unused \
                                 blocks may be the rest of it",
                                e.name,
                                count(usize::from(e.blocks), "block"),
                                reached
                            ));
                        }
                    }
                    let safe = !broken && !dir_broken && !short;
                    plan.add(Fix::FreeLost { blocks: lost }, safe);
                }
            }
        }
        let tracks = disk.miscounted_tracks().len();
        if tracks > 0 {
            plan.add(Fix::Recount { tracks }, true);
        }
        plan
    }

    /// Check one file. Returns whether it's broken.
    fn plan_file(&self, map: &BlockMap, i: usize, e: &Entry, plan: &mut RepairPlan) -> bool {
        let (file, name) = (i, e.name.clone());
        let remove = |reason: &str| Fix::RemoveFile {
            file,
            name: name.clone(),
            reason: reason.into(),
        };
        if !self.is_plain(e) {
            // REL, GEOS, partitions, subdirectories: more structure than a
            // single chain, so they're only ever removed.
            let prefix = format!("\"{}\": ", e.name);
            let faults: Vec<&str> = map
                .problems
                .iter()
                .filter_map(|p| p.strip_prefix(&prefix))
                .collect();
            if let Some(f) = faults.first() {
                plan.add(remove(f), false);
            }
            if self.shared_with(map, i).is_some() {
                plan.notes.push(format!(
                    "\"{name}\" shares blocks with another file; its kind can't be split"
                ));
            }
            return !faults.is_empty();
        }

        let (chain, fault) = self.walk(e.first_track, e.first_sector);
        // A chain that wanders into the DOS's own blocks breaks off there.
        let into_dos = chain.iter().position(|&b| self.is_dos_block(map, b));
        let end = match (into_dos, fault) {
            (Some(k), _) => Some((k, "its chain runs into the directory or BAM".to_string())),
            (None, Some(f)) => Some((chain.len(), f)),
            (None, None) => None,
        };
        match &end {
            // Art in a directory often points at the directory itself.
            Some((0, reason)) if into_dos.is_some() => plan.add(remove(reason), false),
            Some((0, reason)) => plan.add(remove(reason), true),
            Some((keep, reason)) => plan.add(
                Fix::EndFile {
                    file,
                    name: name.clone(),
                    keep: *keep,
                    reason: reason.clone(),
                },
                true,
            ),
            None => {}
        }
        if !e.closed {
            if end.as_ref().is_none_or(|(keep, _)| *keep > 0) {
                plan.add(remove("it was never closed"), false);
            }
        } else if end.is_none() {
            if let Some((at, other)) = self.shared_with(map, i) {
                let with = map.files[other].name.clone();
                let blocks = chain.len() - at;
                let alias = map.files[other].first_track == e.first_track
                    && map.files[other].first_sector == e.first_sector;
                let room = blocks_free_for_copies(self, map) >= blocks;
                // Two names for one file is usually meant.
                plan.add(
                    Fix::SplitFile {
                        file,
                        name,
                        with,
                        blocks,
                    },
                    !alias && room,
                );
            }
        }
        end.is_some()
    }

    /// Apply the chosen fixes of a plan made for this disk. Either all of
    /// them are applied, or (on an error) the disk is left unchanged.
    /// Returns how many fixes were applied.
    pub fn repair(&mut self, plan: &RepairPlan) -> Result<usize> {
        let mut disk = self.clone();
        let mut applied = 0;

        if plan.chosen().any(|f| matches!(f, Fix::EndDirectory { .. })) {
            let dir = disk.geometry().directory_track();
            let (chain, fault) = disk.walk(dir, disk.geometry().directory_first_sector());
            if fault.is_some() {
                disk.end_chain(&chain)?;
            }
            applied += 1;
        }
        // Without a readable directory nothing else can be done safely.
        let slots = disk.entry_slots()?;
        let slot = |file: usize| slots.get(file).ok_or(Error::FileNotFound);

        let removed: Vec<usize> = plan
            .chosen()
            .filter_map(|f| match f {
                Fix::RemoveFile { file, .. } => Some(*file),
                _ => None,
            })
            .collect();
        for fix in plan.chosen() {
            let Fix::EndFile { file, keep, .. } = fix else {
                continue;
            };
            if removed.contains(file) {
                continue;
            }
            let (at, entry) = slot(*file)?;
            let (mut chain, _) = disk.walk(entry.first_track, entry.first_sector);
            chain.truncate(*keep);
            disk.end_chain(&chain)?;
            let n = (chain.len() as u16).to_le_bytes();
            disk.slot_mut(*at)?[30..32].copy_from_slice(&n);
            applied += 1;
        }
        if !removed.is_empty() {
            let before = disk.block_map();
            for &file in &removed {
                disk.slot_mut(slot(file)?.0)?[2] = 0;
                applied += 1;
            }
            let after = disk.block_map();
            for &file in &removed {
                for &(t, s) in &before.chains[file] {
                    let free = after.get(t, s).is_some_and(|b| b.owner == Owner::None);
                    if free && disk.bam_in_use(t) && !disk.boot_blocks().contains(&(t, s)) {
                        disk.bam_set(t, s, false);
                    }
                }
            }
        }
        for fix in plan.chosen() {
            match fix {
                Fix::SplitFile { file, .. } if !removed.contains(file) => {
                    disk.split_file(*file, &removed, slot(*file)?.0)?;
                    applied += 1;
                }
                Fix::MarkUsed { .. } => {
                    for (t, s, b) in blocks(&disk.block_map()) {
                        if b.owner != Owner::None && b.bam == BamState::Free && disk.bam_in_use(t) {
                            disk.bam_set(t, s, true);
                        }
                    }
                    applied += 1;
                }
                Fix::FreeLost { blocks } => {
                    let map = disk.block_map();
                    for &(t, s) in blocks {
                        if map.get(t, s).is_some_and(|b| b.owner == Owner::None) {
                            disk.bam_set(t, s, false);
                        }
                    }
                    applied += 1;
                }
                _ => {}
            }
        }
        // Last, as the fixes above step the counts up and down.
        if plan.chosen().any(|f| matches!(f, Fix::Recount { .. })) {
            for t in disk.miscounted_tracks() {
                disk.recount_track(t);
            }
            applied += 1;
        }
        *self = disk;
        Ok(applied)
    }

    /// Give a file its own copies of the blocks it shares, from the first
    /// shared one to its end.
    fn split_file(&mut self, file: usize, removed: &[usize], at: Slot) -> Result<()> {
        // Files removed before this one have moved it up the directory.
        let index = file - removed.iter().filter(|&&r| r < file).count();
        let map = self.block_map();
        let Some((from, _)) = self.shared_with(&map, index) else {
            return Ok(()); // the file it shared with is gone
        };
        let chain = &map.chains[index];
        let shared = &chain[from..];
        let mut free = free_for_copies(self, &map);
        let copies: Vec<(u8, u8)> = free.by_ref().take(shared.len()).collect();
        if copies.len() < shared.len() {
            return Err(Error::DiskFull);
        }
        drop(free);
        for (i, &(t, s)) in shared.iter().enumerate() {
            let mut data = self.block(t, s)?.to_vec();
            if let Some(&(nt, ns)) = copies.get(i + 1) {
                (data[0], data[1]) = (nt, ns);
            }
            let (ct, cs) = copies[i];
            self.block_mut(ct, cs)?.copy_from_slice(&data);
            self.bam_set(ct, cs, true);
        }
        let (t, s) = copies[0];
        if from == 0 {
            self.slot_mut(at)?[3..5].copy_from_slice(&[t, s]);
        } else {
            let (pt, ps) = chain[from - 1];
            self.block_mut(pt, ps)?[..2].copy_from_slice(&[t, s]);
        }
        Ok(())
    }

    /// Where a file's chain first meets a block an earlier file owns, and
    /// that file.
    fn shared_with(&self, map: &BlockMap, file: usize) -> Option<(usize, usize)> {
        map.chains[file]
            .iter()
            .enumerate()
            .find_map(|(k, &(t, s))| match map.get(t, s)?.owner {
                Owner::File(o) if o != file => Some((k, o)),
                _ => None,
            })
    }

    /// An ordinary file: one chain of blocks and nothing else.
    fn is_plain(&self, e: &Entry) -> bool {
        let kind = matches!(
            e.file_type,
            FileType::Del | FileType::Seq | FileType::Prg | FileType::Usr
        );
        kind && !(self.is_geos() && (e.side_track != 0 || e.rel_record_len != 0))
    }

    /// Whether a block is the directory's, the BAM's or the header's. (The
    /// rest of a 1571's track 53 is only reserved, so a file there is odd
    /// but harmless.)
    fn is_dos_block(&self, map: &BlockMap, (t, s): (u8, u8)) -> bool {
        let reserved = self.geometry() == Geometry::Cbm1571 && t == 53 && s != 0;
        !reserved && map.get(t, s).is_some_and(|b| b.owner == Owner::System)
    }

    /// Whether a track's BAM means anything. A 1571 disk formatted single
    /// sided has nothing on its second side.
    fn bam_in_use(&self, track: u8) -> bool {
        self.geometry() != Geometry::Cbm1571
            || track <= 35
            || self.block(18, 0).is_ok_and(|b| b[3] & 0x80 != 0)
    }

    /// A C128 boot sector, and the blocks it loads after itself.
    fn boot_blocks(&self) -> Vec<(u8, u8)> {
        match self.block(1, 0) {
            Ok(b) if b.starts_with(b"CBM") => self.run(1, 0, 1 + u16::from(b[6])),
            _ => Vec::new(),
        }
    }

    /// Why the disk may hold files the directory doesn't list, if it may.
    fn hidden_files(&self) -> Option<&'static str> {
        let subdirs = matches!(self.geometry(), Geometry::CmdNative { .. })
            && self
                .directory()
                .is_ok_and(|d| d.iter().any(|e| e.file_type == FileType::Other(6)));
        if subdirs {
            return Some("they may belong to files in subdirectories");
        }
        if self.is_geos() {
            let dir = self.geometry().directory_track();
            let h = self.block(dir, self.geometry().header_sector()).ok()?;
            let border = (h[0xAB] != 0).then(|| self.block(h[0xAB], h[0xAC]).ok())??;
            if (0..SLOTS_PER_SECTOR).any(|i| border[i * SLOT_SIZE + 2] != 0) {
                return Some("they may belong to files on the GEOS border");
            }
        }
        None
    }

    fn miscounted_tracks(&self) -> Vec<u8> {
        (1..=self.geometry().tracks())
            .filter(|&t| self.bam_in_use(t) && self.track_miscounted(t))
            .collect()
    }

    /// Make the last block of `chain` the end of its file, all bytes used.
    fn end_chain(&mut self, chain: &[(u8, u8)]) -> Result<()> {
        let &(t, s) = chain.last().ok_or(Error::FileNotFound)?;
        self.block_mut(t, s)?[..2].copy_from_slice(&[0, 0xFF]);
        Ok(())
    }

    /// Every used directory entry with its place, in [`Disk::directory`]'s
    /// order.
    fn entry_slots(&self) -> Result<Vec<(Slot, Entry)>> {
        self.directory()?; // the chain is sound
        let dir = self.geometry().directory_track();
        let (chain, _) = self.walk(dir, self.geometry().directory_first_sector());
        let mut out = Vec::new();
        for (t, s) in chain {
            for i in 0..SLOTS_PER_SECTOR {
                let mut raw = [0u8; SLOT_SIZE];
                raw.copy_from_slice(&self.block(t, s)?[i * SLOT_SIZE..][..SLOT_SIZE]);
                let entry = Entry::parse(&raw);
                if entry.is_used() {
                    out.push(((t, s, i), entry));
                }
            }
        }
        Ok(out)
    }

    fn slot_mut(&mut self, (t, s, i): Slot) -> Result<&mut [u8]> {
        Ok(&mut self.block_mut(t, s)?[i * SLOT_SIZE..][..SLOT_SIZE])
    }
}

/// A directory entry's place: track, sector, and which of its 8 slots.
type Slot = (u8, u8, usize);

/// Every block with its track and sector.
fn blocks(map: &BlockMap) -> impl Iterator<Item = (u8, u8, &crate::blockmap::MapBlock)> {
    map.tracks.iter().enumerate().flat_map(|(t, row)| {
        row.iter()
            .enumerate()
            .map(move |(s, b)| (t as u8 + 1, s as u8, b))
    })
}

/// Blocks off the directory track that are both unused and marked free.
/// (Going by the BAM alone could hand out a block that's in use.)
fn free_for_copies<'a>(disk: &Disk, map: &'a BlockMap) -> impl Iterator<Item = (u8, u8)> + 'a {
    let dir = disk.geometry().directory_track();
    blocks(map)
        .filter(move |(t, _, b)| *t != dir && b.owner == Owner::None && b.bam == BamState::Free)
        .map(|(t, s, _)| (t, s))
}

fn blocks_free_for_copies(disk: &Disk, map: &BlockMap) -> usize {
    free_for_copies(disk, map).count()
}

/// Whether the plan already reports this file as cut short or removed.
fn lost_already(plan: &RepairPlan, i: usize) -> bool {
    plan.choices.iter().any(|c| match &c.fix {
        Fix::EndFile { file, .. } | Fix::RemoveFile { file, .. } => *file == i,
        _ => false,
    })
}

fn count(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}
