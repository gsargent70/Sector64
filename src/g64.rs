//! Reader for **G64** images — the GCR bitstream-level 1541 format — by
//! decoding their tracks into a standard sector-level D64.
//!
//! A D64 stores already-decoded 256-byte sectors; a G64 stores the raw GCR
//! bitstream of each track exactly as it appears on the magnetic medium, sync
//! marks and all. That is what makes G64 able to represent copy protection, and
//! what makes it harder to read: every sector must be found by scanning the
//! bitstream for a sync, then GCR-decoded (each 5 recorded bits carry 4 data
//! bits) and checksum-verified.
//!
//! [`to_d64`] does exactly that for the standard 35-track layout: it walks the
//! track table, decodes every sector it can find and validate, and places it in
//! a 174848-byte D64 image. Sectors it cannot find or validate (including
//! deliberately corrupt protection tracks) are left as zero — a normal disk's
//! directory and files decode cleanly, which [`crate::Disk::load`] then reads
//! with the ordinary sector-level machinery.
//!
//! Every access into the image is bounds-checked and the track scan always
//! makes progress, so no malformed input can panic or loop forever.

use crate::error::{Error, Result};
use crate::geometry;

/// G64 signature for a 1541 image.
const MAGIC: &[u8; 8] = b"GCR-1541";
/// Tracks in the D64 this decoder targets.
const D64_TRACKS: u8 = 35;
/// Decoded sector payload size.
const SECTOR: usize = 256;
/// A sync is at least this many consecutive `1` bits (the 1541 detects 10).
const SYNC_BITS: usize = 10;
/// Header block id byte (GCR-decoded).
const HEADER_ID: u8 = 0x08;
/// Data block id byte (GCR-decoded).
const DATA_ID: u8 = 0x07;

/// Build the GCR 5-bit → 4-bit decode table (`0xFF` marks an invalid code).
const fn gcr_decode_table() -> [u8; 32] {
    // 4-bit nibble value → its 5-bit GCR code.
    let enc: [u8; 16] = [
        0x0a, 0x0b, 0x12, 0x13, 0x0e, 0x0f, 0x16, 0x17, 0x09, 0x19, 0x1a, 0x1b, 0x0d, 0x1d, 0x1e,
        0x15,
    ];
    let mut dec = [0xffu8; 32];
    let mut n = 0;
    while n < 16 {
        dec[enc[n] as usize] = n as u8;
        n += 1;
    }
    dec
}

const GCR: [u8; 32] = gcr_decode_table();

/// A circular bit view over one track's GCR bytes (a physical track is a loop,
/// so a block may wrap past the end).
struct Bits<'a> {
    data: &'a [u8],
    nbits: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits {
            data,
            nbits: data.len() * 8,
        }
    }

    /// The bit at `i`, counted MSB-first within each byte, wrapping at the end.
    #[inline]
    fn bit(&self, i: usize) -> u8 {
        let i = i % self.nbits;
        (self.data[i >> 3] >> (7 - (i & 7))) & 1
    }

    /// Decode `count` GCR bytes starting at bit `pos`. Returns the bytes and the
    /// bit position just past them, or `None` if any 5-bit code is invalid.
    fn gcr_bytes(&self, mut pos: usize, count: usize) -> Option<(Vec<u8>, usize)> {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let mut hi = 0u8;
            for _ in 0..5 {
                hi = (hi << 1) | self.bit(pos);
                pos += 1;
            }
            let mut lo = 0u8;
            for _ in 0..5 {
                lo = (lo << 1) | self.bit(pos);
                pos += 1;
            }
            let (h, l) = (GCR[hi as usize], GCR[lo as usize]);
            if h == 0xff || l == 0xff {
                return None;
            }
            out.push((h << 4) | l);
        }
        Some((out, pos))
    }
}

/// Decode a G64 image into a 35-track D64 (174848 bytes).
///
/// Only version-0 1541 images are handled. Tracks and sectors that cannot be
/// found and checksum-validated are left zero rather than reported as errors,
/// so a protected disk still yields everything readable on it.
pub fn to_d64(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() < 12 || &bytes[..8] != MAGIC {
        return Err(Error::InvalidContainer);
    }
    if bytes[8] != 0 {
        return Err(Error::InvalidContainer); // only version 0
    }
    let num_entries = bytes[9] as usize;
    // Half-track images (the usual 84-entry form) step two entries per track;
    // whole-track images step one.
    let stride = if num_entries > 42 { 2 } else { 1 };

    let mut d64 = vec![0u8; geometry::total_blocks(D64_TRACKS) as usize * SECTOR];

    for t in 1..=D64_TRACKS {
        let entry = (t as usize - 1) * stride;
        if entry >= num_entries {
            break;
        }
        let tpos = 12 + entry * 4;
        let Some(ob) = bytes.get(tpos..tpos + 4) else {
            break;
        };
        let off = u32::from_le_bytes([ob[0], ob[1], ob[2], ob[3]]) as usize;
        if off == 0 {
            continue; // track not present in the image
        }
        let Some(lb) = bytes.get(off..off + 2) else {
            continue;
        };
        let gcr_len = u16::from_le_bytes([lb[0], lb[1]]) as usize;
        let Some(track) = bytes.get(off + 2..off + 2 + gcr_len) else {
            continue;
        };
        if track.len() >= 2 {
            decode_track(track, t, &mut d64);
        }
    }
    Ok(d64)
}

/// Scan one track's GCR bitstream, decoding each `header, data` sector pair into
/// `d64`.
fn decode_track(gcr: &[u8], track: u8, d64: &mut [u8]) {
    let bits = Bits::new(gcr);
    let n = bits.nbits;
    let spt = match geometry::sectors_per_track(track, D64_TRACKS) {
        Some(s) => s,
        None => return,
    };

    // The sector a data block belongs to is announced by the header that
    // immediately precedes it.
    let mut pending: Option<u8> = None;
    let mut i = 0usize;
    while i < n {
        if bits.bit(i) == 0 {
            i += 1;
            continue;
        }
        // Measure the run of 1-bits starting here.
        let mut run = 0usize;
        while run < n && bits.bit(i + run) == 1 {
            run += 1;
        }
        let block_start = i + run; // first bit after the sync
        i = block_start.max(i + 1); // always advance past this run

        if run < SYNC_BITS {
            continue;
        }

        // Peek the block id by decoding its first GCR byte.
        let Some((head, _)) = bits.gcr_bytes(block_start, 1) else {
            continue;
        };
        match head[0] {
            HEADER_ID => {
                if let Some((blk, _)) = bits.gcr_bytes(block_start, 8) {
                    let (csum, sec, trk, id2, id1) = (blk[1], blk[2], blk[3], blk[4], blk[5]);
                    pending = if (sec ^ trk ^ id2 ^ id1) == csum && trk == track && sec < spt {
                        Some(sec)
                    } else {
                        None
                    };
                }
            }
            DATA_ID => {
                if let Some(sec) = pending.take() {
                    // 1 id + 256 data + 1 checksum + 2 trailing = 260 bytes.
                    if let Some((blk, _)) = bits.gcr_bytes(block_start, 260) {
                        let mut cs = 0u8;
                        for &b in &blk[1..257] {
                            cs ^= b;
                        }
                        if cs == blk[257] {
                            if let Ok(o) = geometry::ts_to_offset(track, sec, D64_TRACKS) {
                                if let Some(dst) = d64.get_mut(o..o + SECTOR) {
                                    dst.copy_from_slice(&blk[1..257]);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_g64() {
        assert_eq!(to_d64(b"not a g64 image").err(), Some(Error::InvalidContainer));
        assert_eq!(to_d64(&[]).err(), Some(Error::InvalidContainer));
    }

    #[test]
    fn rejects_bad_version() {
        let mut b = MAGIC.to_vec();
        b.push(9); // version 9
        b.extend_from_slice(&[84, 0xf8, 0x1e]);
        assert_eq!(to_d64(&b).err(), Some(Error::InvalidContainer));
    }

    #[test]
    fn empty_track_table_yields_blank_d64() {
        // Valid header, 84 entries all zero (no tracks present).
        let mut b = MAGIC.to_vec();
        b.push(0); // version
        b.push(84); // entries
        b.extend_from_slice(&[0xf8, 0x1e]); // max track size
        b.extend_from_slice(&vec![0u8; 84 * 4 * 2]); // offset + speed tables
        let d64 = to_d64(&b).unwrap();
        assert_eq!(d64.len(), 174848);
        assert!(d64.iter().all(|&x| x == 0));
    }

    #[test]
    fn gcr_table_is_complete() {
        // All 16 nibbles are represented exactly once; others are invalid.
        let valid = GCR.iter().filter(|&&x| x != 0xff).count();
        assert_eq!(valid, 16);
    }
}
