//! Directory entry layout and parsing.
//!
//! Each directory sector is 256 bytes = 8 slots of 32 bytes. Slot 0's first two
//! bytes are the track/sector link to the next directory sector (zero in the
//! other slots); every slot's file fields begin at slot offset 2:
//!
//! ```text
//! slot[0..2]   next-dir track/sector (slot 0 only)
//! slot[2]      file type / flags
//! slot[3]      first data track
//! slot[4]      first data sector
//! slot[5..21]  filename, 16 bytes, padded with 0xA0
//! slot[21]     REL side-sector track
//! slot[22]     REL side-sector sector
//! slot[23]     REL record length
//! slot[24..30] GEOS / unused
//! slot[30]     block count low
//! slot[31]     block count high
//! ```

use crate::petscii;

/// Size of one directory slot.
pub const SLOT_SIZE: usize = 32;
/// Slots per directory sector.
pub const SLOTS_PER_SECTOR: usize = 8;

/// CBM file type (low nibble of the type byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Del,
    Seq,
    Prg,
    Usr,
    Rel,
    /// Any other low-nibble value (e.g. CBM partition 5, or unknown).
    Other(u8),
}

impl FileType {
    fn from_nibble(n: u8) -> FileType {
        match n & 0x0F {
            0 => FileType::Del,
            1 => FileType::Seq,
            2 => FileType::Prg,
            3 => FileType::Usr,
            4 => FileType::Rel,
            other => FileType::Other(other),
        }
    }

    /// The low-nibble type code stored in a directory entry.
    pub fn to_nibble(self) -> u8 {
        match self {
            FileType::Del => 0,
            FileType::Seq => 1,
            FileType::Prg => 2,
            FileType::Usr => 3,
            FileType::Rel => 4,
            FileType::Other(n) => n & 0x0F,
        }
    }

    /// Three-letter mnemonic as shown in a classic directory listing.
    pub fn mnemonic(self) -> &'static str {
        match self {
            FileType::Del => "del",
            FileType::Seq => "seq",
            FileType::Prg => "prg",
            FileType::Usr => "usr",
            FileType::Rel => "rel",
            FileType::Other(_) => "???",
        }
    }
}

/// One parsed directory entry.
#[derive(Debug, Clone)]
pub struct Entry {
    pub file_type: FileType,
    /// Bit 7 of the type byte: a properly closed file.
    pub closed: bool,
    /// Bit 6 of the type byte: locked (shown as `<`).
    pub locked: bool,
    pub name: String,
    pub name_raw: [u8; 16],
    pub first_track: u8,
    pub first_sector: u8,
    /// REL only: track of the first side-sector block (0 if not a REL file).
    pub side_track: u8,
    /// REL only: sector of the first side-sector block.
    pub side_sector: u8,
    pub rel_record_len: u8,
    pub blocks: u16,
}

impl Entry {
    /// Parse one 32-byte slot. `slot` must be exactly [`SLOT_SIZE`] bytes.
    pub fn parse(slot: &[u8; SLOT_SIZE]) -> Entry {
        let type_byte = slot[2];
        let mut name_raw = [0u8; 16];
        name_raw.copy_from_slice(&slot[5..21]);
        Entry {
            file_type: FileType::from_nibble(type_byte),
            closed: type_byte & 0x80 != 0,
            locked: type_byte & 0x40 != 0,
            name: petscii::field_to_string(&name_raw),
            name_raw,
            first_track: slot[3],
            first_sector: slot[4],
            side_track: slot[21],
            side_sector: slot[22],
            rel_record_len: slot[23],
            blocks: u16::from_le_bytes([slot[30], slot[31]]),
        }
    }

    /// A slot is "used" when its type nibble is non-zero or it is explicitly a
    /// closed DEL entry. Empty/scratched slots (type byte 0x00) are skipped by
    /// the listing but still reachable via the raw walk.
    pub fn is_used(&self) -> bool {
        !matches!(self.file_type, FileType::Del) || self.closed
    }

    /// Listing flags around the type mnemonic: `*` for an unclosed (splat)
    /// file, `<` for a locked file.
    pub fn flags(&self) -> (char, char) {
        (if self.closed { ' ' } else { '*' }, if self.locked { '<' } else { ' ' })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_prg_entry() {
        let mut slot = [0u8; SLOT_SIZE];
        slot[2] = 0x82; // closed PRG
        slot[3] = 17;
        slot[4] = 0;
        slot[5..9].copy_from_slice(b"GAME");
        slot[9..21].fill(petscii::PAD);
        slot[30] = 10;
        slot[31] = 0;
        let e = Entry::parse(&slot);
        assert_eq!(e.file_type, FileType::Prg);
        assert!(e.closed && !e.locked);
        assert_eq!(e.name, "GAME");
        assert_eq!((e.first_track, e.first_sector), (17, 0));
        assert_eq!(e.blocks, 10);
    }
}
