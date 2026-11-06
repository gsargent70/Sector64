//! Reader for **TAP** images — the raw datasette pulse stream of a Commodore
//! cassette.
//!
//! A TAP is the lowest-level tape format: it records the length of every pulse
//! (half-wave) the datasette produced, in clock cycles, with no interpretation.
//! That is deliberately all it is — the *bit* encoding on a tape is defined by
//! whichever loader wrote it. The KERNAL (ROM) loader has a known three-level
//! scheme, but most commercial tapes use custom **turbo** loaders with their
//! own pulse encodings, so there is no general way to recover files from a TAP
//! (emulators play the pulses into an emulated datasette rather than decode
//! them). This module therefore parses the container and decodes the pulse
//! stream — the layer that is actually universal and checkable — and does not
//! attempt file extraction.
//!
//! Every access is bounds-checked and the decode always advances, so no
//! malformed input can panic or loop forever.
//!
//! Reference: the TAP format (version 0/1/2) as documented for VICE.

use crate::error::{Error, Result};

/// TAP signature.
const MAGIC: &[u8; 12] = b"C64-TAPE-RAW";
/// PAL C64 system clock, Hz (for a duration estimate).
pub const CLOCK_PAL: u32 = 985_248;
/// NTSC C64 system clock, Hz.
pub const CLOCK_NTSC: u32 = 1_022_727;

/// A decoded TAP image: header fields plus every pulse length in cycles.
#[derive(Debug, Clone)]
pub struct TapImage {
    /// Format version (0, 1 or 2).
    pub version: u8,
    /// Platform byte (0 = C64, 1 = VIC-20, 2 = C16/plus4, …).
    pub platform: u8,
    /// Video standard (0 = PAL, 1 = NTSC, 2 = old NTSC).
    pub video: u8,
    /// Each pulse's length in system clock cycles, in order.
    pub pulses: Vec<u32>,
}

impl TapImage {
    /// Total tape length in clock cycles.
    pub fn total_cycles(&self) -> u64 {
        self.pulses.iter().map(|&c| c as u64).sum()
    }

    /// Approximate play time in seconds, using the image's video standard.
    pub fn duration_secs(&self) -> f64 {
        let clock = if self.video == 1 { CLOCK_NTSC } else { CLOCK_PAL };
        self.total_cycles() as f64 / clock as f64
    }
}

/// Parse a TAP image into its header and pulse stream.
pub fn parse_tap(bytes: &[u8]) -> Result<TapImage> {
    if bytes.len() < 20 || &bytes[..12] != MAGIC {
        return Err(Error::InvalidContainer);
    }
    let version = bytes[12];
    if version > 2 {
        return Err(Error::InvalidContainer);
    }
    let platform = bytes[13];
    let video = bytes[14];

    // The declared data length; fall back to the rest of the file if zero or
    // longer than what is present.
    let declared = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
    let avail = bytes.len() - 20;
    let data_len = if declared == 0 || declared > avail {
        avail
    } else {
        declared
    };
    let data = &bytes[20..20 + data_len];

    // Version 0: a zero byte is an overflow pulse with no recorded length.
    // Versions 1 and 2: a zero byte is followed by a 24-bit little-endian
    // cycle count. A non-zero byte is the cycle count divided by 8.
    let mut pulses = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b != 0 {
            pulses.push((b as u32) * 8);
            i += 1;
        } else if version == 0 {
            pulses.push(0); // overflow, length unknown
            i += 1;
        } else {
            // Need three more bytes for the explicit length.
            if i + 4 > data.len() {
                break;
            }
            let c = u32::from_le_bytes([data[i + 1], data[i + 2], data[i + 3], 0]);
            pulses.push(c);
            i += 4;
        }
    }

    Ok(TapImage {
        version,
        platform,
        video,
        pulses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(version: u8, body: &[u8]) -> Vec<u8> {
        let mut b = MAGIC.to_vec();
        b.push(version);
        b.extend_from_slice(&[0, 0, 0]); // platform, video, reserved
        b.extend_from_slice(&(body.len() as u32).to_le_bytes());
        b.extend_from_slice(body);
        b
    }

    #[test]
    fn decodes_short_and_overflow_pulses() {
        // two short pulses (0x30 -> 384 cycles) and one explicit long pulse.
        let body = [0x30, 0x30, 0x00, 0x99, 0x1f, 0x05];
        let tap = parse_tap(&build(1, &body)).unwrap();
        assert_eq!(tap.version, 1);
        assert_eq!(tap.pulses, vec![384, 384, 0x051f99]);
        assert_eq!(tap.total_cycles(), 384 + 384 + 0x051f99);
    }

    #[test]
    fn version0_zero_is_overflow() {
        let body = [0x20, 0x00, 0x20];
        let tap = parse_tap(&build(0, &body)).unwrap();
        assert_eq!(tap.pulses, vec![256, 0, 256]);
    }

    #[test]
    fn rejects_non_tap_and_bad_version() {
        assert!(parse_tap(b"not a tape image!!!!").is_err());
        assert!(parse_tap(&[]).is_err());
        let mut bad = MAGIC.to_vec();
        bad.push(9); // version 9
        bad.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]);
        assert!(parse_tap(&bad).is_err());
    }
}
