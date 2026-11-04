//! Block Availability Map (BAM) reader, including the 40-track extensions.
//!
//! Tracks 1–35 use the standard layout in track 18, sector 0: after the
//! directory link and DOS version byte, each track gets a 4-byte entry from
//! offset `$04`:
//!
//! ```text
//! entry[0]    free-sector count for the track
//! entry[1..4] 24-bit allocation bitmap, LSB = sector 0; a SET bit means FREE
//! ```
//!
//! For 40-track images the extra tracks 36–40 are stored in the tail of the
//! same sector, but **where** depends on the DOS that wrote the disk:
//! DolphinDOS puts them at `$AC`, SpeedDOS at `$C0`, each as five more 4-byte
//! entries in the same format.
//!
//! The extended offsets here are taken from the documented formats, not
//! verified against hardware-written images, so the [`DosFormat`] is explicit
//! in the API and [`detect_dos`] is a best-effort, clearly-fallible heuristic.

use crate::error::{Error, Result};
use crate::geometry::sectors_per_track;
use crate::geometry::DIRECTORY_TRACK;

/// Highest track in the standard BAM region.
pub const BAM_TRACKS: u8 = 35;

/// How a disk stores its BAM. For 35-track images this is always [`Cbm`].
///
/// [`Cbm`]: DosFormat::Cbm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DosFormat {
    /// Standard CBM DOS: tracks 1–35 only.
    Cbm,
    /// 40-track DolphinDOS: tracks 36–40 BAM at offset `$AC`.
    Dolphin,
    /// 40-track SpeedDOS: tracks 36–40 BAM at offset `$C0`.
    Speed,
}

impl DosFormat {
    /// Byte offset of the tracks-36–40 BAM region, or `None` for [`Cbm`].
    fn extended_base(self) -> Option<usize> {
        match self {
            DosFormat::Cbm => None,
            DosFormat::Dolphin => Some(0xAC),
            DosFormat::Speed => Some(0xC0),
        }
    }
}

/// A parsed BAM sector plus the geometry/DOS needed to interpret it.
pub struct Bam {
    sector: [u8; 256],
    track_count: u8,
    dos: DosFormat,
}

impl Bam {
    /// Parse the 256-byte track-18/sector-0 block for a disk of `track_count`
    /// tracks using `dos`.
    pub fn parse(sector: &[u8], track_count: u8, dos: DosFormat) -> Bam {
        let mut s = [0u8; 256];
        s.copy_from_slice(&sector[..256]);
        Bam {
            sector: s,
            track_count,
            dos,
        }
    }

    pub fn dos(&self) -> DosFormat {
        self.dos
    }

    /// The 4-byte BAM entry for `track`, or `None` when the track is out of
    /// range or its BAM is not represented (tracks 36–40 under [`DosFormat::Cbm`]).
    fn entry(&self, track: u8) -> Option<&[u8]> {
        if track == 0 || track > self.track_count {
            return None;
        }
        let base = if track <= BAM_TRACKS {
            4 + (track as usize - 1) * 4
        } else {
            self.dos.extended_base()? + (track as usize - 36) * 4
        };
        self.sector.get(base..base + 4)
    }

    /// The free-sector count the BAM records for `track`.
    pub fn free_count(&self, track: u8) -> Option<u8> {
        self.entry(track).map(|e| e[0])
    }

    /// Whether (track, sector) is allocated per the bitmap, bounds-checked.
    ///
    /// Returns an error for an out-of-range track/sector, or for an extended
    /// track whose BAM this [`DosFormat`] does not locate.
    pub fn is_allocated(&self, track: u8, sector: u8) -> Result<bool> {
        let spt = sectors_per_track(track, self.track_count)
            .ok_or(Error::InvalidTrackSector { track, sector })?;
        if sector >= spt {
            return Err(Error::InvalidTrackSector { track, sector });
        }
        let entry = self
            .entry(track)
            .ok_or(Error::InvalidTrackSector { track, sector })?;
        let byte = entry[1 + (sector / 8) as usize];
        let bit = 1u8 << (sector % 8);
        // A set bit means free, so "allocated" is the clear bit.
        Ok(byte & bit == 0)
    }

    /// Total free blocks across every track the BAM covers, excluding the
    /// directory track. Tracks whose BAM is not represented are skipped.
    pub fn free_blocks(&self) -> u16 {
        let mut free = 0u16;
        for track in 1..=self.track_count {
            if track != DIRECTORY_TRACK {
                if let Some(c) = self.free_count(track) {
                    free = free.saturating_add(c as u16);
                }
            }
        }
        free
    }
}

/// Is the five-entry extended region at `base` self-consistent *and* carries
/// real data (each free count ≤ 17 and equal to the popcount of its 17-bit
/// bitmap)? Used only to tell DolphinDOS from SpeedDOS.
fn extended_region_populated(sector: &[u8], base: usize) -> bool {
    let Some(region) = sector.get(base..base + 20) else {
        return false;
    };
    let mut any_data = false;
    for t in 0..5 {
        let e = &region[t * 4..t * 4 + 4];
        let free = e[0] as u32;
        if free > 17 {
            return false;
        }
        let bits = ((e[1] as u32) | ((e[2] as u32) << 8) | ((e[3] as u32) << 16)) & 0x1_FFFF;
        if bits.count_ones() != free {
            return false;
        }
        if free != 0 || e[1] != 0 || e[2] != 0 || e[3] != 0 {
            any_data = true;
        }
    }
    any_data
}

/// Best-effort guess of the DOS format from the BAM sector.
///
/// For 35-track disks this is always [`DosFormat::Cbm`]. For 40-track disks it
/// checks which of the two candidate regions is self-consistent and populated;
/// if exactly one is, that DOS is returned, otherwise [`DosFormat::Cbm`] (so
/// the extended tracks are simply not counted). This is a heuristic on a
/// documented-but-unverified layout — pass the format explicitly when known.
pub fn detect_dos(sector: &[u8], track_count: u8) -> DosFormat {
    if track_count <= BAM_TRACKS {
        return DosFormat::Cbm;
    }
    let dolphin = extended_region_populated(sector, 0xAC);
    let speed = extended_region_populated(sector, 0xC0);
    match (dolphin, speed) {
        (true, false) => DosFormat::Dolphin,
        (false, true) => DosFormat::Speed,
        _ => DosFormat::Cbm, // ambiguous or neither: do not guess
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_entry(sector: &mut [u8], base: usize, free: u8, bitmap: u32) {
        sector[base] = free;
        sector[base + 1] = bitmap as u8;
        sector[base + 2] = (bitmap >> 8) as u8;
        sector[base + 3] = (bitmap >> 16) as u8;
    }

    #[test]
    fn standard_free_bit_means_free() {
        let mut s = [0u8; 256];
        // Track 1: sector 0 allocated, sectors 1..20 free (21 total).
        write_entry(&mut s, 4, 20, 0x1F_FFFE);
        let bam = Bam::parse(&s, 35, DosFormat::Cbm);
        assert_eq!(bam.free_count(1), Some(20));
        assert_eq!(bam.is_allocated(1, 0), Ok(true));
        assert_eq!(bam.is_allocated(1, 1), Ok(false));
        assert!(bam.is_allocated(1, 21).is_err());
    }

    #[test]
    fn cbm_rejects_extended_tracks() {
        let s = [0u8; 256];
        let bam = Bam::parse(&s, 35, DosFormat::Cbm);
        assert!(bam.is_allocated(40, 0).is_err()); // 40 > track_count 35
        let bam40 = Bam::parse(&s, 40, DosFormat::Cbm);
        assert!(bam40.is_allocated(40, 0).is_err()); // no extended region for Cbm
        assert_eq!(bam40.free_count(40), None);
    }

    #[test]
    fn dolphin_extended_region() {
        let mut s = [0u8; 256];
        // Track 40 at Dolphin base 0xAC + (40-36)*4 = 0xBC: sector 0 allocated.
        write_entry(&mut s, 0xBC, 16, 0x1_FFFE);
        let bam = Bam::parse(&s, 40, DosFormat::Dolphin);
        assert_eq!(bam.free_count(40), Some(16));
        assert_eq!(bam.is_allocated(40, 0), Ok(true));
        assert_eq!(bam.is_allocated(40, 1), Ok(false));
        assert!(bam.is_allocated(40, 17).is_err()); // track 40 has 17 sectors (0..16)
    }

    #[test]
    fn detects_dolphin_vs_speed() {
        let mut dol = [0u8; 256];
        for t in 0..5 {
            write_entry(&mut dol, 0xAC + t * 4, 17, 0x1_FFFF); // all free
        }
        assert_eq!(detect_dos(&dol, 40), DosFormat::Dolphin);

        let mut spd = [0u8; 256];
        for t in 0..5 {
            write_entry(&mut spd, 0xC0 + t * 4, 17, 0x1_FFFF);
        }
        assert_eq!(detect_dos(&spd, 40), DosFormat::Speed);

        // Nothing in either region -> don't guess.
        assert_eq!(detect_dos(&[0u8; 256], 40), DosFormat::Cbm);
        // Always Cbm for 35-track.
        assert_eq!(detect_dos(&dol, 35), DosFormat::Cbm);
    }
}
