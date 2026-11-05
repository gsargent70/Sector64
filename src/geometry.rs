//! 1541 disk geometry and bounds-checked track/sector addressing.
//!
//! This is the module that, in the Windows original, lacked any range check:
//! its offset function computed `(track_start[track] + sector) * 256` with no
//! validation of `track` or `sector`, so a crafted directory link could point
//! the read anywhere. Here every conversion is total and checked.

use crate::error::{Error, Result};

/// Bytes per block (sector) on a 1541 disk.
pub const BLOCK_SIZE: usize = 256;

/// The directory lives on track 18.
pub const DIRECTORY_TRACK: u8 = 18;
/// The BAM / disk header is track 18, sector 0; directory entries start at 18/1.
pub const DIRECTORY_FIRST_SECTOR: u8 = 1;

/// Sectors on a 1-indexed `track`, given a disk of `track_count` tracks.
///
/// Returns `None` when `track` is 0 or beyond `track_count`. The speed-zone
/// layout is the standard 1541 one; the 36–40 zone (17 sectors) covers the
/// 40-track SpeedDOS/DolphinDOS extension.
pub fn sectors_per_track(track: u8, track_count: u8) -> Option<u8> {
    if track == 0 || track > track_count {
        return None;
    }
    Some(match track {
        1..=17 => 21,
        18..=24 => 19,
        25..=30 => 18,
        _ => 17, // 31..=40
    })
}

/// Total data blocks on a disk of `track_count` tracks (35 → 683, 40 → 768).
pub fn total_blocks(track_count: u8) -> u32 {
    (1..=track_count)
        .map(|t| sectors_per_track(t, track_count).expect("t in range") as u32)
        .sum()
}

/// Block index (0-based, in disk order) of (track, sector), bounds-checked.
pub fn ts_to_block(track: u8, sector: u8, track_count: u8) -> Result<u32> {
    let spt =
        sectors_per_track(track, track_count).ok_or(Error::InvalidTrackSector { track, sector })?;
    if sector >= spt {
        return Err(Error::InvalidTrackSector { track, sector });
    }
    let blocks_before: u32 = (1..track)
        .map(|t| sectors_per_track(t, track_count).expect("t < track <= count") as u32)
        .sum();
    Ok(blocks_before + sector as u32)
}

/// Byte offset of the first byte of (track, sector) within the data area,
/// bounds-checked. This is the safe replacement for the original's unclamped
/// `(table[track] + sector) * 256`.
pub fn ts_to_offset(track: u8, sector: u8, track_count: u8) -> Result<usize> {
    Ok(ts_to_block(track, sector, track_count)? as usize * BLOCK_SIZE)
}

/// The layout of a disk image: track/sector addressing and where the directory
/// lives. One value per supported drive family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Geometry {
    /// 1541 D64: 35 or 40 tracks, speed-zoned, directory on track 18.
    Cbm1541 { tracks: u8 },
    /// 1571 D71: 70 tracks — two 1541 sides — directory on track 18.
    Cbm1571,
    /// 1581 D81: 80 tracks of 40 sectors, directory on track 40.
    Cbm1581,
}

impl Geometry {
    /// Total track count.
    pub fn tracks(self) -> u8 {
        match self {
            Geometry::Cbm1541 { tracks } => tracks,
            Geometry::Cbm1571 => 70,
            Geometry::Cbm1581 => 80,
        }
    }

    /// Sectors on a 1-indexed track, or `None` when out of range.
    pub fn sectors_per_track(self, track: u8) -> Option<u8> {
        match self {
            Geometry::Cbm1541 { tracks } => sectors_per_track(track, tracks),
            // Side 1 (tracks 36–70) mirrors side 0's speed zones.
            Geometry::Cbm1571 => {
                if track == 0 || track > 70 {
                    None
                } else {
                    sectors_per_track(if track <= 35 { track } else { track - 35 }, 35)
                }
            }
            Geometry::Cbm1581 => (1..=80).contains(&track).then_some(40),
        }
    }

    /// Total data blocks on the disk.
    pub fn total_blocks(self) -> u32 {
        (1..=self.tracks())
            .map(|t| self.sectors_per_track(t).expect("t in range") as u32)
            .sum()
    }

    /// Block index (0-based, disk order) of (track, sector), bounds-checked.
    pub fn ts_to_block(self, track: u8, sector: u8) -> Result<u32> {
        let spt = self
            .sectors_per_track(track)
            .ok_or(Error::InvalidTrackSector { track, sector })?;
        if sector >= spt {
            return Err(Error::InvalidTrackSector { track, sector });
        }
        let before: u32 = (1..track)
            .map(|t| self.sectors_per_track(t).expect("t < track") as u32)
            .sum();
        Ok(before + sector as u32)
    }

    /// Byte offset of (track, sector) within the data area, bounds-checked.
    pub fn ts_to_offset(self, track: u8, sector: u8) -> Result<usize> {
        Ok(self.ts_to_block(track, sector)? as usize * BLOCK_SIZE)
    }

    /// Track holding the directory and BAM.
    pub fn directory_track(self) -> u8 {
        match self {
            Geometry::Cbm1541 { .. } | Geometry::Cbm1571 => 18,
            Geometry::Cbm1581 => 40,
        }
    }

    /// First directory-entry sector (the BAM sits before it).
    pub fn directory_first_sector(self) -> u8 {
        match self {
            Geometry::Cbm1541 { .. } | Geometry::Cbm1571 => 1,
            Geometry::Cbm1581 => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_sizes() {
        assert_eq!(sectors_per_track(1, 35), Some(21));
        assert_eq!(sectors_per_track(18, 35), Some(19));
        assert_eq!(sectors_per_track(25, 35), Some(18));
        assert_eq!(sectors_per_track(35, 35), Some(17));
        assert_eq!(sectors_per_track(0, 35), None);
        assert_eq!(sectors_per_track(36, 35), None);
        assert_eq!(sectors_per_track(40, 40), Some(17));
    }

    #[test]
    fn totals() {
        assert_eq!(total_blocks(35), 683);
        assert_eq!(total_blocks(40), 768);
    }

    #[test]
    fn known_offsets() {
        // Track 1, sector 0 is block 0.
        assert_eq!(ts_to_offset(1, 0, 35).unwrap(), 0);
        // Track 18, sector 0 is block 357 (sum of tracks 1..17 = 17*21 = 357).
        assert_eq!(ts_to_block(18, 0, 35).unwrap(), 357);
        assert_eq!(ts_to_offset(18, 1, 35).unwrap(), 358 * BLOCK_SIZE);
    }

    #[test]
    fn d81_geometry() {
        let g = Geometry::Cbm1581;
        assert_eq!(g.tracks(), 80);
        assert_eq!(g.sectors_per_track(1), Some(40));
        assert_eq!(g.sectors_per_track(80), Some(40));
        assert_eq!(g.sectors_per_track(81), None);
        assert_eq!(g.total_blocks(), 3200);
        assert_eq!(g.directory_track(), 40);
        assert_eq!(g.directory_first_sector(), 3);
        // Linear LSN addressing: track 2 sector 0 is block 40.
        assert_eq!(g.ts_to_block(2, 0).unwrap(), 40);
        assert!(g.ts_to_offset(1, 40).is_err());
    }

    #[test]
    fn d64_geometry_matches_free_functions() {
        let g = Geometry::Cbm1541 { tracks: 35 };
        assert_eq!(g.total_blocks(), 683);
        assert_eq!(g.directory_track(), 18);
        assert_eq!(g.ts_to_offset(18, 1).unwrap(), 358 * BLOCK_SIZE);
    }

    #[test]
    fn rejects_out_of_range() {
        // The exact failure the audit flagged: an over-large sector number.
        assert_eq!(
            ts_to_offset(35, 17, 35),
            Err(Error::InvalidTrackSector { track: 35, sector: 17 })
        );
        assert!(ts_to_offset(99, 0, 35).is_err());
        assert!(ts_to_offset(36, 0, 35).is_err());
    }
}
