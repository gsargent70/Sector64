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
