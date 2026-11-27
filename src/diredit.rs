//! Directory editing: the disk's name and ID, locking files, reordering the
//! directory, and separator ("directory art") entries.
//!
//! None of these touch file data. Reordering moves whole directory entries
//! between the slots the directory already uses, so its block chain, the free
//! slots, and the BAM stay as they are.

use crate::directory::{Entry, SLOT_SIZE};
use crate::disk::Disk;
use crate::error::{Error, Result};
use crate::geometry::Geometry;
use crate::{petscii, FileType};

/// The 16 name bytes of a plain separator line: sixteen `-`.
pub const SEPARATOR: [u8; 16] = [b'-'; 16];

impl Disk {
    /// Where each used directory entry lives, in directory order: (track,
    /// sector, slot).
    fn slots(&self) -> Result<Vec<(u8, u8, usize)>> {
        Ok(self.entry_slots()?.into_iter().map(|(at, _)| at).collect())
    }

    /// The 30 bytes of entry `index` (in directory order) after the slot's
    /// two link bytes, and where they are.
    fn entry_bytes(&self, index: usize) -> Result<((u8, u8, usize), [u8; SLOT_SIZE - 2])> {
        let at = *self.slots()?.get(index).ok_or(Error::FileNotFound)?;
        let o = at.2 * SLOT_SIZE;
        let mut b = [0u8; SLOT_SIZE - 2];
        b.copy_from_slice(&self.block(at.0, at.1)?[o + 2..o + SLOT_SIZE]);
        Ok((at, b))
    }

    fn put_entry_bytes(&mut self, at: (u8, u8, usize), bytes: &[u8; SLOT_SIZE - 2]) -> Result<()> {
        let o = at.2 * SLOT_SIZE;
        self.block_mut(at.0, at.1)?[o + 2..o + SLOT_SIZE].copy_from_slice(bytes);
        Ok(())
    }

    /// Move directory entry `from` to position `to` (both in directory
    /// order), shifting the entries between them by one. Only the entries
    /// move: the slots they sit in, and the directory's blocks, stay put.
    pub fn move_entry(&mut self, from: usize, to: usize) -> Result<()> {
        let slots = self.slots()?;
        if from >= slots.len() || to >= slots.len() {
            return Err(Error::FileNotFound);
        }
        let mut entries = Vec::with_capacity(slots.len());
        for i in 0..slots.len() {
            entries.push(self.entry_bytes(i)?.1);
        }
        let moved = entries.remove(from);
        entries.insert(to, moved);
        let (lo, hi) = (from.min(to), from.max(to));
        for i in lo..=hi {
            self.put_entry_bytes(slots[i], &entries[i])?;
        }
        Ok(())
    }

    /// Lock (`<` in a listing, so the drive won't scratch it) or unlock
    /// directory entry `index`.
    pub fn set_locked(&mut self, index: usize, locked: bool) -> Result<()> {
        let (at, mut b) = self.entry_bytes(index)?;
        // b[0] is the slot's type byte (offset 2); bit 6 is the lock.
        if locked {
            b[0] |= 0x40;
        } else {
            b[0] &= !0x40;
        }
        self.put_entry_bytes(at, &b)
    }

    /// Add a separator entry with these 16 name bytes (PETSCII, as stored)
    /// just after entry `after` (or first, for `None`). Returns its index.
    ///
    /// A separator is a closed DEL file of 0 blocks whose start points at
    /// the disk header, the usual form of "directory art": it owns no blocks,
    /// so nothing (the BAM, a validate, a copy) treats it as a file.
    pub fn add_separator(&mut self, name: [u8; 16], after: Option<usize>) -> Result<usize> {
        let (dt, ds, slot) = self.find_dir_slot()?;
        let header = (self.dir_track(), self.geometry().header_sector());
        let b = self.block_mut(dt, ds)?;
        let o = slot * SLOT_SIZE;
        b[o + 2..o + SLOT_SIZE].fill(0);
        b[o + 2] = 0x80; // closed DEL
        b[o + 3] = header.0;
        b[o + 4] = header.1;
        b[o + 5..o + 21].copy_from_slice(&name);
        let slots = self.slots()?;
        let new = slots
            .iter()
            .position(|&s| s == (dt, ds, slot))
            .ok_or(Error::FileNotFound)?;
        let to = after.map_or(0, |a| if a < new { a + 1 } else { a });
        self.move_entry(new, to)?;
        Ok(to)
    }

    /// Change the disk's name (up to 16 characters) and ID (2), as a
    /// directory listing's header line shows them. On a 1581 the ID's copies
    /// in the two BAM blocks change too.
    pub fn set_header(&mut self, name: &str, id: &str) -> Result<()> {
        let name = petscii::encode_field(name, 16);
        let id = petscii::encode_field(id, 2);
        let geom = self.geometry();
        let (t, s) = (self.dir_track(), geom.header_sector());
        let (name_at, id_at) = match geom {
            Geometry::Cbm1541 { .. } | Geometry::Cbm1571 => (0x90, 0xA2),
            Geometry::Cbm1581 | Geometry::CmdNative { .. } => (0x04, 0x16),
        };
        let b = self.block_mut(t, s)?;
        b[name_at..name_at + 16].copy_from_slice(&name);
        b[id_at..id_at + 2].copy_from_slice(&id);
        if geom == Geometry::Cbm1581 {
            for bam in [1, 2] {
                self.block_mut(t, bam)?[4..6].copy_from_slice(&id);
            }
        }
        Ok(())
    }
}

/// Whether `entry` is a separator or other directory art rather than a file:
/// a 0-block DEL entry.
pub fn is_separator(entry: &Entry) -> bool {
    entry.file_type == FileType::Del && entry.blocks == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(d: &Disk) -> Vec<String> {
        d.directory().unwrap().into_iter().map(|e| e.name).collect()
    }

    fn disk_with(n: usize) -> Disk {
        let mut d = Disk::format("TEST", "01");
        for i in 0..n {
            d.write_file(&format!("F{i}"), FileType::Prg, &[1, 8, i as u8])
                .unwrap();
        }
        d
    }

    #[test]
    fn moving_an_entry_keeps_its_file_and_the_others() {
        // 10 entries span two directory blocks.
        let mut d = disk_with(10);
        let before: Vec<Vec<u8>> = d
            .directory()
            .unwrap()
            .iter()
            .map(|e| d.read_entry(e).unwrap())
            .collect();
        let free = d.blocks_free().unwrap();
        d.move_entry(9, 0).unwrap();
        assert_eq!(names(&d)[..3], ["F9", "F0", "F1"]);
        d.move_entry(0, 9).unwrap();
        assert_eq!(
            names(&d),
            (0..10).map(|i| format!("F{i}")).collect::<Vec<_>>()
        );
        d.move_entry(2, 5).unwrap();
        assert_eq!(names(&d)[2..6], ["F3", "F4", "F5", "F2"]);
        let f2 = d.find_entry("F2").unwrap().unwrap();
        assert_eq!(d.read_entry(&f2).unwrap(), before[2]);
        assert_eq!(d.blocks_free().unwrap(), free);
        assert!(d.validate().unwrap().is_clean());
        assert!(d.move_entry(0, 10).is_err());
    }

    #[test]
    fn locking_sets_and_clears_only_the_lock() {
        let mut d = disk_with(2);
        d.set_locked(1, true).unwrap();
        let e = &d.directory().unwrap()[1];
        assert!(e.locked && e.closed && e.file_type == FileType::Prg);
        assert!(!d.directory().unwrap()[0].locked);
        d.set_locked(1, false).unwrap();
        assert!(!d.directory().unwrap()[1].locked);
    }

    #[test]
    fn a_separator_owns_no_blocks_and_goes_where_asked() {
        let mut d = disk_with(3);
        let free = d.blocks_free().unwrap();
        assert_eq!(d.add_separator(SEPARATOR, Some(0)).unwrap(), 1);
        assert_eq!(d.add_separator(SEPARATOR, None).unwrap(), 0);
        let dir = d.directory().unwrap();
        assert_eq!(dir.len(), 5);
        assert!(is_separator(&dir[0]) && is_separator(&dir[2]));
        assert_eq!(dir[2].name, "----------------");
        assert!(!is_separator(&dir[1]));
        assert_eq!(d.blocks_free().unwrap(), free);
        assert!(d.validate().unwrap().is_clean());
        assert!(d.repair_plan().choices.is_empty());
    }

    #[test]
    fn a_separator_reads_empty_and_copies_as_a_separator() {
        let mut d = disk_with(1);
        d.add_separator(SEPARATOR, Some(0)).unwrap();
        d.add_separator(SEPARATOR, Some(1)).unwrap();
        let dir = d.directory().unwrap();
        assert!(d.read_entry(&dir[1]).unwrap().is_empty());
        let mut to = disk_with(0);
        let free = to.blocks_free().unwrap();
        for e in &dir {
            to.copy_file_from(&d, e).unwrap();
        }
        let copied = to.directory().unwrap();
        assert_eq!(copied.len(), 3);
        assert!(is_separator(&copied[1]) && is_separator(&copied[2]));
        assert_eq!(to.blocks_free().unwrap(), free - 1, "only F0 takes a block");
    }

    #[test]
    fn separators_are_renamed_and_deleted_by_place_not_name() {
        let mut d = disk_with(1);
        d.add_separator(SEPARATOR, Some(0)).unwrap();
        d.add_separator(SEPARATOR, Some(1)).unwrap();
        let free = d.blocks_free().unwrap();
        d.rename_entry(2, "== SECOND ==").unwrap();
        assert_eq!(names(&d), ["F0", "----------------", "== SECOND =="]);
        // Deleting one frees nothing: its start is the directory's header.
        assert_eq!(d.scratch_entry(1).unwrap(), 0);
        assert_eq!(names(&d), ["F0", "== SECOND =="]);
        assert_eq!(d.blocks_free().unwrap(), free);
        assert!(d.validate().unwrap().is_clean());
    }

    #[test]
    fn the_header_changes_on_every_format() {
        for mut d in [
            Disk::format("OLD", "01"),
            Disk::format_d71("OLD", "01"),
            Disk::format_d81("OLD", "01"),
        ] {
            d.set_header("NEW NAME", "N2").unwrap();
            let h = d.header().unwrap();
            assert_eq!((h.name.as_str(), h.id.as_str()), ("NEW NAME", "N2"));
        }
        let mut d = Disk::format_d81("OLD", "01");
        d.set_header("X", "AB").unwrap();
        for bam in [1, 2] {
            assert_eq!(&d.block(40, bam).unwrap()[4..6], b"AB");
        }
    }
}
