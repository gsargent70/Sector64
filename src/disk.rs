//! The in-memory D64 image and its safe accessors.

use crate::bam::{self, Bam, DosFormat};
use crate::directory::{Entry, FileType, SLOTS_PER_SECTOR, SLOT_SIZE};
use crate::error::{Error, Result};
use crate::geometry::{
    self, BLOCK_SIZE, DIRECTORY_FIRST_SECTOR, DIRECTORY_TRACK,
};
use crate::petscii;

/// A loaded D64 disk image.
pub struct Disk {
    data: Vec<u8>,
    track_count: u8,
    /// Length of the data area in bytes (excludes any trailing error-info map).
    data_len: usize,
    /// BAM format; detected at load for 40-track images, overridable.
    dos: DosFormat,
}

/// The disk header fields from track 18, sector 0.
#[derive(Debug, Clone)]
pub struct Header {
    pub name: String,
    pub id: String,
    pub dos_type: String,
}

/// Result of a read-only consistency check ([`Disk::validate`]).
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    /// Free blocks the BAM claims (tracks 1–35).
    pub blocks_free: u16,
    /// Distinct blocks actually occupied by directory file chains.
    pub blocks_used: usize,
    /// Blocks that appear in more than one file's chain (cross-linked).
    pub cross_linked: Vec<(u8, u8)>,
    /// Blocks a file uses but the BAM marks free (tracks 1–35 only).
    pub used_but_free: Vec<(u8, u8)>,
}

impl ValidationReport {
    /// True when no inconsistencies were found.
    pub fn is_clean(&self) -> bool {
        self.cross_linked.is_empty() && self.used_but_free.is_empty()
    }
}

/// A relative file's fixed-length records, read via its side sectors.
#[derive(Debug, Clone)]
pub struct RelFile {
    /// Record length declared in the directory entry.
    pub record_len: u8,
    /// The records, each `record_len` bytes (a short final record indicates a
    /// truncated image).
    pub records: Vec<Vec<u8>>,
}

/// Side-sector data-block pointers start at this offset within a side sector.
const SIDE_SECTOR_POINTER_BASE: usize = 16;

impl Disk {
    /// Load an image from raw bytes, inferring geometry from the length.
    ///
    /// Accepts the four standard sizes (35/40 tracks, with or without the
    /// trailing per-block error map). The error map, when present, is ignored
    /// for data reads.
    pub fn load(bytes: impl Into<Vec<u8>>) -> Result<Disk> {
        let data = bytes.into();
        let track_count = match data.len() {
            174848 | 175531 => 35,
            196608 | 197376 => 40,
            n => return Err(Error::UnknownImageSize(n)),
        };
        let data_len = geometry::total_blocks(track_count) as usize * BLOCK_SIZE;
        let mut disk = Disk {
            data,
            track_count,
            data_len,
            dos: DosFormat::Cbm,
        };
        // For 40-track images, guess the extended-BAM format from the sector.
        if track_count > bam::BAM_TRACKS {
            if let Ok(sector) = disk.block(DIRECTORY_TRACK, 0) {
                disk.dos = bam::detect_dos(sector, track_count);
            }
        }
        Ok(disk)
    }

    pub fn track_count(&self) -> u8 {
        self.track_count
    }

    /// The BAM format in use (detected at load; see [`Disk::set_dos`]).
    pub fn dos(&self) -> DosFormat {
        self.dos
    }

    /// Override the BAM format, for when the 40-track DOS is known out of band
    /// and detection guessed wrong (or declined to guess).
    pub fn set_dos(&mut self, dos: DosFormat) {
        self.dos = dos;
    }

    /// Borrow one 256-byte block by track/sector.
    ///
    /// Both the start offset *and* the full block length are checked against
    /// the data area — the explicit fix for the original's start-only gate.
    pub fn block(&self, track: u8, sector: u8) -> Result<&[u8]> {
        let offset = geometry::ts_to_offset(track, sector, self.track_count)?;
        let end = offset
            .checked_add(BLOCK_SIZE)
            .filter(|&e| e <= self.data_len)
            .ok_or(Error::Truncated {
                track,
                sector,
                offset,
                data_len: self.data_len,
            })?;
        Ok(&self.data[offset..end])
    }

    /// Parse the disk header (name, id, DOS type) from track 18, sector 0.
    pub fn header(&self) -> Result<Header> {
        let bam = self.block(DIRECTORY_TRACK, 0)?;
        Ok(Header {
            name: petscii::field_to_string(&bam[0x90..0xA0]),
            id: petscii::field_to_string(&bam[0xA2..0xA4]),
            dos_type: petscii::field_to_string(&bam[0xA5..0xA7]),
        })
    }

    /// Parse the BAM from track 18, sector 0, using this disk's DOS format.
    pub fn bam(&self) -> Result<Bam> {
        Ok(Bam::parse(
            self.block(DIRECTORY_TRACK, 0)?,
            self.track_count,
            self.dos,
        ))
    }

    /// Blocks free, summed from the standard 35-track BAM (track 18 excluded,
    /// as on real hardware). Extended-track BAMs vary by DOS and are not summed.
    pub fn blocks_free(&self) -> Result<u16> {
        Ok(self.bam()?.free_blocks())
    }

    /// Walk the directory chain from 18/1 and return every used entry.
    ///
    /// The walk is hardened against the two things a crafted image can do that
    /// the original did not guard: it bounds-checks each link through
    /// [`Disk::block`], and it rejects a chain that revisits a sector (a cycle)
    /// or runs longer than the disk could possibly hold.
    pub fn directory(&self) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();
        let mut visited: Vec<(u8, u8)> = Vec::new();
        let max_sectors = geometry::total_blocks(self.track_count) as usize;

        let mut track = DIRECTORY_TRACK;
        let mut sector = DIRECTORY_FIRST_SECTOR;

        loop {
            if visited.contains(&(track, sector)) {
                return Err(Error::DirectoryCycle { track, sector });
            }
            if visited.len() >= max_sectors {
                return Err(Error::DirectoryTooLong {
                    visited: visited.len(),
                });
            }
            visited.push((track, sector));

            let block = self.block(track, sector)?;
            let next_track = block[0];
            let next_sector = block[1];

            for i in 0..SLOTS_PER_SECTOR {
                let start = i * SLOT_SIZE;
                let mut slot = [0u8; SLOT_SIZE];
                slot.copy_from_slice(&block[start..start + SLOT_SIZE]);
                let entry = Entry::parse(&slot);
                if entry.is_used() {
                    entries.push(entry);
                }
            }

            if next_track == 0 {
                break; // end of chain
            }
            track = next_track;
            sector = next_sector;
        }
        Ok(entries)
    }

    /// Read the full contents of a file by the start block from its directory
    /// entry.
    ///
    /// A 1541 file is a track/sector linked list: each data block's first two
    /// bytes link to the next block and the remaining 254 are data. The final
    /// block has link-track 0, and its link-sector is the offset of the last
    /// valid byte, so the tail block contributes `link_sector - 1` bytes.
    ///
    /// Like the directory walk, this is hardened: every link is bounds-checked
    /// through [`Disk::block`], a visited-set rejects cycles, and a hard cap
    /// rejects a chain longer than the disk. A start track of 0 (a scratched or
    /// empty entry) yields an empty file rather than an error.
    pub fn read_file(&self, first_track: u8, first_sector: u8) -> Result<Vec<u8>> {
        let chain = self.file_block_chain(first_track, first_sector)?;
        let mut out = Vec::new();
        for (i, &(track, sector)) in chain.iter().enumerate() {
            let block = self.block(track, sector)?;
            if i + 1 == chain.len() {
                // Final block: block[1] indexes the last valid byte, so the data
                // is block[2..=block[1]]. Clamp defensively.
                let end = (block[1] as usize).saturating_add(1).min(BLOCK_SIZE);
                if end > 2 {
                    out.extend_from_slice(&block[2..end]);
                }
            } else {
                out.extend_from_slice(&block[2..BLOCK_SIZE]); // 254 data bytes
            }
        }
        Ok(out)
    }

    /// The ordered list of (track, sector) blocks a file occupies, following
    /// its chain from the start block. A start track of 0 gives an empty chain.
    ///
    /// The returned `Vec` doubles as the visited-set, so a repeated block is a
    /// cycle; the chain is also capped at the disk's block count and every link
    /// is bounds-checked through [`Disk::block`].
    pub fn file_block_chain(&self, first_track: u8, first_sector: u8) -> Result<Vec<(u8, u8)>> {
        let mut chain: Vec<(u8, u8)> = Vec::new();
        if first_track == 0 {
            return Ok(chain);
        }
        let max_blocks = geometry::total_blocks(self.track_count) as usize;
        let (mut track, mut sector) = (first_track, first_sector);
        loop {
            if chain.contains(&(track, sector)) {
                return Err(Error::FileChainCycle { track, sector });
            }
            if chain.len() >= max_blocks {
                return Err(Error::FileTooLong { blocks: chain.len() });
            }
            let block = self.block(track, sector)?;
            let (next_track, next_sector) = (block[0], block[1]);
            chain.push((track, sector));
            if next_track == 0 {
                break;
            }
            track = next_track;
            sector = next_sector;
        }
        Ok(chain)
    }

    /// Convenience: read the contents of a parsed directory [`Entry`].
    pub fn read_entry(&self, entry: &Entry) -> Result<Vec<u8>> {
        self.read_file(entry.first_track, entry.first_sector)
    }

    /// Find the first used directory entry whose display name matches `name`
    /// (case-insensitive).
    pub fn find_entry(&self, name: &str) -> Result<Option<Entry>> {
        let target = name.to_ascii_uppercase();
        Ok(self
            .directory()?
            .into_iter()
            .find(|e| e.name.to_ascii_uppercase() == target))
    }

    /// Read-only consistency check: walk every file's block chain and compare
    /// it against the BAM.
    ///
    /// Reports blocks claimed by more than one file (cross-linked) and blocks a
    /// file occupies that the BAM marks free. Allocation is only compared for
    /// tracks 1–35, since the extended-track BAM is not parsed. A structural
    /// fault that stops a walk (an out-of-range link or a cycle) surfaces as an
    /// `Err` rather than a report entry.
    pub fn validate(&self) -> Result<ValidationReport> {
        let bam = self.bam()?;
        let mut used: Vec<(u8, u8)> = Vec::new();
        let mut report = ValidationReport {
            blocks_free: bam.free_blocks(),
            ..Default::default()
        };

        for entry in self.directory()? {
            for bs in self.file_block_chain(entry.first_track, entry.first_sector)? {
                if used.contains(&bs) {
                    if !report.cross_linked.contains(&bs) {
                        report.cross_linked.push(bs);
                    }
                } else {
                    used.push(bs);
                }
            }
        }

        for &(track, sector) in &used {
            // A block used by a file but marked free is an inconsistency. Tracks
            // the BAM does not cover (extended tracks under an unknown DOS)
            // return an error from is_allocated and are skipped here.
            if bam.is_allocated(track, sector) == Ok(false) {
                report.used_but_free.push((track, sector));
            }
        }

        report.blocks_used = used.len();
        Ok(report)
    }

    /// The side-sector block chain of a REL file (track 18 directory entry).
    ///
    /// Guarded exactly like the data chain: each link is bounds-checked and a
    /// visited-set rejects a cycle.
    pub fn side_sector_chain(&self, entry: &Entry) -> Result<Vec<(u8, u8)>> {
        if entry.file_type != FileType::Rel {
            return Err(Error::NotRelativeFile);
        }
        if entry.side_track == 0 {
            return Err(Error::MissingSideSectors);
        }
        let mut chain: Vec<(u8, u8)> = Vec::new();
        let max_blocks = geometry::total_blocks(self.track_count) as usize;
        let (mut track, mut sector) = (entry.side_track, entry.side_sector);
        loop {
            if chain.contains(&(track, sector)) {
                return Err(Error::FileChainCycle { track, sector });
            }
            if chain.len() >= max_blocks {
                return Err(Error::FileTooLong { blocks: chain.len() });
            }
            let block = self.block(track, sector)?;
            let (next_track, next_sector) = (block[0], block[1]);
            chain.push((track, sector));
            if next_track == 0 {
                break;
            }
            track = next_track;
            sector = next_sector;
        }
        Ok(chain)
    }

    /// The ordered data-block pointers a REL file's side sectors index.
    ///
    /// Each side sector lists up to 120 (track, sector) data-block pointers
    /// from offset 16; a zero track byte terminates the list. The returned
    /// pointers are capped at the disk's block count but are only verified when
    /// read.
    pub fn rel_data_blocks(&self, entry: &Entry) -> Result<Vec<(u8, u8)>> {
        let side = self.side_sector_chain(entry)?;
        let mut blocks: Vec<(u8, u8)> = Vec::new();
        let max_blocks = geometry::total_blocks(self.track_count) as usize;
        'outer: for &(t, s) in &side {
            let ss = self.block(t, s)?;
            let mut off = SIDE_SECTOR_POINTER_BASE;
            while off + 1 < BLOCK_SIZE {
                let (dt, ds) = (ss[off], ss[off + 1]);
                if dt == 0 {
                    break 'outer; // end of the data-block list
                }
                if blocks.len() >= max_blocks {
                    return Err(Error::FileTooLong { blocks: blocks.len() });
                }
                blocks.push((dt, ds));
                off += 2;
            }
        }
        Ok(blocks)
    }

    /// Read a relative file's records, located through its side sectors.
    ///
    /// Assembles the packed record stream from the indexed data blocks (254
    /// bytes per block, the final block truncated at its last-used byte) and
    /// splits it into `record_len`-byte records.
    pub fn read_rel(&self, entry: &Entry) -> Result<RelFile> {
        if entry.file_type != FileType::Rel {
            return Err(Error::NotRelativeFile);
        }
        let record_len = entry.rel_record_len;
        if record_len == 0 {
            return Err(Error::InvalidRecordLength);
        }
        let blocks = self.rel_data_blocks(entry)?;
        let mut stream: Vec<u8> = Vec::new();
        for (i, &(t, s)) in blocks.iter().enumerate() {
            let b = self.block(t, s)?;
            let end = if i + 1 == blocks.len() && b[0] == 0 {
                (b[1] as usize).saturating_add(1).clamp(2, BLOCK_SIZE)
            } else {
                BLOCK_SIZE
            };
            stream.extend_from_slice(&b[2..end]);
        }
        let records = stream.chunks(record_len as usize).map(<[u8]>::to_vec).collect();
        Ok(RelFile { record_len, records })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid 35-track image with a header and one directory
    /// sector holding a single PRG entry, chain-terminated.
    fn synthetic() -> Vec<u8> {
        let mut img = vec![0u8; 174848];
        let put = |img: &mut [u8], t: u8, s: u8, f: &dyn Fn(&mut [u8])| {
            let off = geometry::ts_to_offset(t, s, 35).unwrap();
            f(&mut img[off..off + BLOCK_SIZE]);
        };
        // BAM / header at 18/0.
        put(&mut img, 18, 0, &|b| {
            b[0] = 18;
            b[1] = 1; // first dir sector
            b[2] = b'A';
            b[0x90..0xA0].copy_from_slice(b"MY DISK\xA0\xA0\xA0\xA0\xA0\xA0\xA0\xA0\xA0");
            b[0xA2] = b'I';
            b[0xA3] = b'D';
            b[0xA5] = b'2';
            b[0xA6] = b'A';
            // Give track 1 five free blocks so blocks_free has something to sum.
            b[4] = 5;
        });
        // Directory sector 18/1: terminated chain, one PRG in slot 0.
        put(&mut img, 18, 1, &|b| {
            b[0] = 0; // no next dir sector
            b[1] = 0xFF;
            b[2] = 0x82; // closed PRG
            b[3] = 17;
            b[4] = 0;
            b[5..9].copy_from_slice(b"GAME");
            b[9..21].fill(petscii::PAD);
            b[30] = 10;
        });
        img
    }

    #[test]
    fn loads_and_reads_header() {
        let disk = Disk::load(synthetic()).unwrap();
        assert_eq!(disk.track_count(), 35);
        let h = disk.header().unwrap();
        assert_eq!(h.name, "MY DISK");
        assert_eq!(h.id, "ID");
        assert_eq!(disk.blocks_free().unwrap(), 5);
    }

    #[test]
    fn walks_directory() {
        let disk = Disk::load(synthetic()).unwrap();
        let dir = disk.directory().unwrap();
        assert_eq!(dir.len(), 1);
        assert_eq!(dir[0].name, "GAME");
        assert_eq!(dir[0].blocks, 10);
    }

    #[test]
    fn rejects_bad_size() {
        assert_eq!(Disk::load(vec![0u8; 123]).err(), Some(Error::UnknownImageSize(123)));
    }

    #[test]
    fn detects_directory_cycle() {
        let mut img = synthetic();
        // Make 18/1 link back to itself.
        let off = geometry::ts_to_offset(18, 1, 35).unwrap();
        img[off] = 18;
        img[off + 1] = 1;
        let disk = Disk::load(img).unwrap();
        assert!(matches!(disk.directory(), Err(Error::DirectoryCycle { .. })));
    }

    #[test]
    fn rejects_out_of_range_link() {
        let mut img = synthetic();
        // Point the chain at a non-existent track 99.
        let off = geometry::ts_to_offset(18, 1, 35).unwrap();
        img[off] = 99;
        img[off + 1] = 9;
        let disk = Disk::load(img).unwrap();
        assert!(matches!(
            disk.directory(),
            Err(Error::InvalidTrackSector { track: 99, sector: 9 })
        ));
    }

    fn put(img: &mut [u8], t: u8, s: u8, f: &dyn Fn(&mut [u8])) {
        let off = geometry::ts_to_offset(t, s, 35).unwrap();
        f(&mut img[off..off + BLOCK_SIZE]);
    }

    #[test]
    fn reads_file_chain() {
        let mut img = synthetic();
        // GAME (in synthetic) starts at 17/0; build a two-block chain there.
        put(&mut img, 17, 0, &|b| {
            b[0] = 17;
            b[1] = 1; // link to 17/1
            b[2..].fill(0xAA); // 254 data bytes
        });
        put(&mut img, 17, 1, &|b| {
            b[0] = 0; // final block
            b[1] = 5; // last valid byte index -> block[2..6] = 4 bytes
            b[2..6].copy_from_slice(b"END!");
        });
        let disk = Disk::load(img).unwrap();
        let data = disk.read_file(17, 0).unwrap();
        assert_eq!(data.len(), 254 + 4);
        assert!(data[..254].iter().all(|&x| x == 0xAA));
        assert_eq!(&data[254..], b"END!");
    }

    #[test]
    fn find_and_read_entry() {
        let mut img = synthetic();
        // Single final block at 17/0 holding "HI".
        put(&mut img, 17, 0, &|b| {
            b[0] = 0;
            b[1] = 3; // block[2..4] = 2 bytes
            b[2..4].copy_from_slice(b"HI");
        });
        let disk = Disk::load(img).unwrap();
        let entry = disk.find_entry("game").unwrap().expect("GAME present");
        assert_eq!(disk.read_entry(&entry).unwrap(), b"HI");
    }

    #[test]
    fn file_chain_cycle_rejected() {
        let mut img = synthetic();
        put(&mut img, 17, 0, &|b| {
            b[0] = 17;
            b[1] = 0; // self-loop
        });
        let disk = Disk::load(img).unwrap();
        assert!(matches!(
            disk.read_file(17, 0),
            Err(Error::FileChainCycle { track: 17, sector: 0 })
        ));
    }

    #[test]
    fn empty_start_is_empty_file() {
        let disk = Disk::load(synthetic()).unwrap();
        assert!(disk.read_file(0, 0).unwrap().is_empty());
    }

    #[test]
    fn validate_flags_unallocated_block() {
        let mut img = synthetic();
        // GAME (at 17/0) occupies one block.
        put(&mut img, 17, 0, &|b| {
            b[0] = 0;
            b[1] = 3;
            b[2..4].copy_from_slice(b"HI");
        });
        // Mark track 17, sector 0 as FREE in the BAM (bit set) so the file's
        // block contradicts the map.
        let bam_off = geometry::ts_to_offset(18, 0, 35).unwrap();
        let entry17 = bam_off + 4 + (17 - 1) * 4;
        img[entry17 + 1] |= 0x01; // sector 0 bit -> free
        let disk = Disk::load(img).unwrap();
        let report = disk.validate().unwrap();
        assert_eq!(report.blocks_used, 1);
        assert!(report.cross_linked.is_empty());
        assert_eq!(report.used_but_free, vec![(17, 0)]);
        assert!(!report.is_clean());
    }

    /// A 40-track image whose extended BAM is in the DolphinDOS location.
    fn synthetic40() -> Vec<u8> {
        let mut img = vec![0u8; 196608];
        let off = geometry::ts_to_offset(18, 0, 40).unwrap();
        let b = &mut img[off..off + BLOCK_SIZE];
        b[0] = 18;
        b[1] = 1;
        b[2] = b'A';
        b[0x90..0xA0].copy_from_slice(b"BIG DISK\xA0\xA0\xA0\xA0\xA0\xA0\xA0\xA0");
        // Dolphin extended BAM at 0xAC: tracks 36-40 all free (17 sectors each).
        for t in 0..5 {
            let base = 0xAC + t * 4;
            b[base] = 17;
            b[base + 1] = 0xFF;
            b[base + 2] = 0xFF;
            b[base + 3] = 0x01;
        }
        // Empty, terminated directory.
        let doff = geometry::ts_to_offset(18, 1, 40).unwrap();
        img[doff] = 0;
        img[doff + 1] = 0xFF;
        img
    }

    #[test]
    fn detects_and_counts_40_track() {
        let disk = Disk::load(synthetic40()).unwrap();
        assert_eq!(disk.track_count(), 40);
        assert_eq!(disk.dos(), DosFormat::Dolphin);
        // Tracks 1-35 all zero free; 36-40 contribute 17 each -> 85 free.
        assert_eq!(disk.blocks_free().unwrap(), 85);
        let bam = disk.bam().unwrap();
        assert_eq!(bam.is_allocated(40, 0), Ok(false)); // marked free
        assert_eq!(bam.free_count(40), Some(17));
    }

    #[test]
    fn set_dos_overrides_detection() {
        let mut disk = Disk::load(synthetic40()).unwrap();
        disk.set_dos(DosFormat::Cbm); // pretend we don't trust detection
        assert_eq!(disk.blocks_free().unwrap(), 0); // extended tracks no longer counted
        assert!(disk.bam().unwrap().is_allocated(40, 0).is_err());
    }

    /// Build a REL file: record length 4, two records "AAAA"/"BBBB" in one data
    /// block at 1/0, indexed by a single side sector at 1/1.
    fn rel_entry_and_image() -> (Vec<u8>, Entry) {
        let mut img = synthetic();
        // Data block 1/0: final block, 8 bytes of record data.
        put(&mut img, 1, 0, &|b| {
            b[0] = 0; // last data block
            b[1] = (2 + 8 - 1) as u8; // last valid byte index = 9
            b[2..10].copy_from_slice(b"AAAABBBB");
        });
        // Side sector 1/1: ss #0, record length 4, one data-block pointer (1/0).
        put(&mut img, 1, 1, &|b| {
            b[0] = 0; // no next side sector
            b[1] = 0xFF;
            b[2] = 0; // side-sector number
            b[3] = 4; // record length
            b[4] = 1; // side-sector group: ss0 at 1/1
            b[5] = 1;
            b[SIDE_SECTOR_POINTER_BASE] = 1; // data block pointer: 1/0
            b[SIDE_SECTOR_POINTER_BASE + 1] = 0;
        });
        let entry = Entry {
            file_type: FileType::Rel,
            closed: true,
            locked: false,
            name: "DATA".into(),
            name_raw: [0; 16],
            first_track: 1,
            first_sector: 0,
            side_track: 1,
            side_sector: 1,
            rel_record_len: 4,
            blocks: 2,
        };
        (img, entry)
    }

    #[test]
    fn reads_rel_side_sectors_and_records() {
        let (img, entry) = rel_entry_and_image();
        let disk = Disk::load(img).unwrap();
        assert_eq!(disk.side_sector_chain(&entry).unwrap(), vec![(1, 1)]);
        assert_eq!(disk.rel_data_blocks(&entry).unwrap(), vec![(1, 0)]);
        let rel = disk.read_rel(&entry).unwrap();
        assert_eq!(rel.record_len, 4);
        assert_eq!(rel.records, vec![b"AAAA".to_vec(), b"BBBB".to_vec()]);
    }

    #[test]
    fn rel_on_non_rel_is_rejected() {
        let disk = Disk::load(synthetic()).unwrap();
        let prg = disk.find_entry("game").unwrap().unwrap(); // GAME is a PRG
        assert_eq!(disk.read_rel(&prg).err(), Some(Error::NotRelativeFile));
    }
}
