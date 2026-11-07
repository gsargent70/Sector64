//! Reader and decompressor for **Wraptor 3** (`.wr3`) archives.
//!
//! A Wraptor-3 archive is a sequence of entries, each introduced by the
//! four-byte marker `FF "BL" FF` followed by the file's NUL-terminated name, a
//! one-byte file-type field, and then the file's compressed bitstream. Two
//! padding bytes separate one entry's bitstream from the next marker. The
//! marker does not occur inside the compressed streams (every one in real
//! archives is followed by a clean name), so the entries are recovered by
//! scanning for markers and validating the name that must follow each one.
//!
//! ## Compression
//!
//! Each file is stored as an LZ-style bitstream read **most-significant bit
//! first**, one input byte at a time, against a **4 KiB sliding window**. The
//! grammar, recovered by reverse-engineering the original 6502 `Wraptor-3.0`
//! decoder (its self-decruncher was emulated to recover the real program, whose
//! decode loop was then read directly):
//!
//! * a `0` flag bit → an 8-bit **literal** byte;
//! * a `1` flag bit → read an **index** of `iw` bits (`iw` starts at 8):
//!   * index `0` → read one more bit: `0` ends the stream, `1` widens `iw` by
//!     one bit (the window has filled past the current index width);
//!   * index `v != 0` → read a 5-bit **length** `L` and copy `L` bytes from
//!     window position `v - 1` (the window is circular; copied bytes are
//!     appended to it, so an overlapping copy expands a run).
//!
//! The window **persists across the archive's members** (only the write cursor
//! and `iw` reset at each member), so members must be decoded in order. Each
//! decoded stream begins with a nine-byte per-file header (load address / BASIC
//! start / type bytes); the extracted file is everything after it. This was
//! validated byte-for-byte against all seventeen plaintext members supplied for
//! a real Loadstar archive. Every access is bounds-checked and decoding never
//! reads past the input.

use crate::error::{Error, Result};

/// The per-entry marker: `0xFF 'B' 'L' 0xFF`.
const MARKER: [u8; 4] = [0xFF, 0x42, 0x4C, 0xFF];
/// A name may be at most this many bytes before its terminating NUL.
const MAX_NAME: usize = 24;
/// The sliding-window size, in bytes.
const WINDOW: usize = 4096;
/// Each decoded member begins with this many header bytes before the file's
/// real content.
const FILE_HEADER_LEN: usize = 9;

/// One archive member: its name and its raw (still compressed) data.
#[derive(Debug, Clone)]
pub struct Wr3Entry {
    /// The member's file name.
    pub name: String,
    /// The member's stored data: a one-byte file-type field followed by the
    /// compressed bitstream (and two trailing pad bytes before the next
    /// marker). Use [`decompress_wr3`] to decode.
    pub compressed: Vec<u8>,
}

/// One decompressed archive member.
#[derive(Debug, Clone)]
pub struct Wr3File {
    /// The member's file name.
    pub name: String,
    /// The decompressed file content.
    pub content: Vec<u8>,
}

/// A most-significant-bit-first reader over a byte slice. Reads past the end
/// yield zero bits (and set [`BitReader::eof`]) rather than panicking.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    cur: u8,
    nbits: u8,
    eof: bool,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0, cur: 0, nbits: 0, eof: false }
    }

    fn bit(&mut self) -> u32 {
        if self.nbits == 0 {
            match self.data.get(self.pos) {
                Some(&b) => {
                    self.cur = b;
                    self.pos += 1;
                    self.nbits = 8;
                }
                None => {
                    self.eof = true;
                    return 0;
                }
            }
        }
        let b = (self.cur >> 7) & 1;
        self.cur <<= 1;
        self.nbits -= 1;
        b as u32
    }

    fn bits(&mut self, k: u32) -> u32 {
        let mut v = 0u32;
        for _ in 0..k {
            v = (v << 1) | self.bit();
        }
        v
    }
}

/// Decode one member's bitstream into the shared `window`, returning the full
/// decoded stream (including the nine-byte per-file header). `iw` and the write
/// cursor reset per member; `window` carries over from earlier members.
fn decode_member(window: &mut [u8; WINDOW], bitstream: &[u8]) -> Vec<u8> {
    let mut br = BitReader::new(bitstream);
    let mut out: Vec<u8> = Vec::new();
    let mut wp = 0usize; // write cursor, 0..WINDOW
    let mut iw = 8u32; // current index width in bits

    loop {
        if br.eof {
            break;
        }
        if br.bit() == 0 {
            // Literal byte.
            let b = br.bits(8) as u8;
            out.push(b);
            window[wp] = b;
            wp = (wp + 1) & (WINDOW - 1);
        } else {
            let v = br.bits(iw);
            if v == 0 {
                if br.bit() == 0 {
                    break; // end of stream
                }
                iw += 1; // widen the index
            } else {
                let len = br.bits(5);
                let src = (v as usize - 1) & (WINDOW - 1);
                for j in 0..len as usize {
                    let b = window[(src + j) & (WINDOW - 1)];
                    out.push(b);
                    window[wp] = b;
                    wp = (wp + 1) & (WINDOW - 1);
                }
            }
        }
    }
    out
}

/// Decompress every member of a Wraptor-3 archive, in order, into its file
/// name and content. Members share one sliding window, so the whole archive is
/// decoded together.
pub fn decompress_wr3(bytes: &[u8]) -> Result<Vec<Wr3File>> {
    let entries = parse_wr3(bytes)?;
    let mut window = [0u8; WINDOW];
    let mut files = Vec::with_capacity(entries.len());
    for e in &entries {
        // Skip the one-byte file-type field; the bitstream follows it.
        let bitstream = e.compressed.get(1..).unwrap_or_default();
        let decoded = decode_member(&mut window, bitstream);
        let content = decoded.get(FILE_HEADER_LEN..).unwrap_or_default().to_vec();
        files.push(Wr3File { name: e.name.clone(), content });
    }
    Ok(files)
}

/// Is there a valid marker at `pos` followed by a printable, NUL-terminated
/// name? Returns the offset just past the name's NUL.
fn name_after_marker(bytes: &[u8], pos: usize) -> Option<usize> {
    if bytes.get(pos..pos + 4) != Some(&MARKER) {
        return None;
    }
    let name_start = pos + 4;
    let end = (name_start + MAX_NAME).min(bytes.len());
    let rel = bytes.get(name_start..end)?.iter().position(|&b| b == 0)?;
    if rel == 0 {
        return None; // empty name
    }
    if bytes[name_start..name_start + rel].iter().all(|&b| (0x20..=0x7E).contains(&b)) {
        Some(name_start + rel + 1)
    } else {
        None
    }
}

/// Parse a Wraptor-3 archive into its entries.
pub fn parse_wr3(bytes: &[u8]) -> Result<Vec<Wr3Entry>> {
    if !bytes.starts_with(&MARKER) {
        return Err(Error::InvalidContainer);
    }

    // Collect the positions of every real entry (a marker followed by a valid
    // name). A marker-like sequence inside compressed data is not followed by a
    // clean name, so it is ignored.
    let mut starts: Vec<(usize, usize)> = Vec::new(); // (marker pos, data start)
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if let Some(data_start) = name_after_marker(bytes, i) {
            starts.push((i, data_start));
            i = data_start; // resume scanning after this name
        } else {
            i += 1;
        }
    }
    if starts.is_empty() {
        return Err(Error::InvalidContainer);
    }

    let mut entries = Vec::with_capacity(starts.len());
    for k in 0..starts.len() {
        let (marker_pos, data_start) = starts[k];
        let data_end = starts.get(k + 1).map_or(bytes.len(), |&(next, _)| next);
        let name = String::from_utf8_lossy(&bytes[marker_pos + 4..data_start - 1]).into_owned();
        let compressed = bytes.get(data_start..data_end).unwrap_or_default().to_vec();
        entries.push(Wr3Entry { name, compressed });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, data: &[u8]) -> Vec<u8> {
        let mut v = MARKER.to_vec();
        v.extend_from_slice(name.as_bytes());
        v.push(0);
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn parses_entries() {
        let mut a = entry("STAR", &[1, 2, 3, 4]);
        a.extend_from_slice(&entry("HELLO", &[9, 9]));
        let e = parse_wr3(&a).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].name, "STAR");
        assert_eq!(e[0].compressed, vec![1, 2, 3, 4]);
        assert_eq!(e[1].name, "HELLO");
        assert_eq!(e[1].compressed, vec![9, 9]);
    }

    #[test]
    fn ignores_marker_inside_data() {
        // A marker-like run in the data is not followed by a clean NUL-named
        // string, so it does not split the entry.
        let mut data = vec![0xFF, 0x42, 0x4C, 0xFF, 0x00, 0x01]; // marker then NUL = empty name
        data.extend_from_slice(&[0xAA; 8]);
        let a = entry("ONE", &data);
        let e = parse_wr3(&a).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].name, "ONE");
        assert_eq!(e[0].compressed, data);
    }

    #[test]
    fn rejects_non_wr3() {
        assert!(parse_wr3(b"not a wraptor archive").is_err());
        assert!(parse_wr3(&[]).is_err());
    }

    /// Encode `content` with the Wraptor grammar as plain literals (flag `0` +
    /// 8 bits each) and a terminating end marker (flag `1`, index `0`, bit `0`),
    /// MSB first — the simplest valid stream a real decoder accepts.
    fn literal_bitstream(content: &[u8]) -> Vec<u8> {
        let mut bits: Vec<u8> = Vec::new();
        for &b in content {
            bits.push(0); // literal flag
            for i in (0..8).rev() {
                bits.push((b >> i) & 1);
            }
        }
        // End: flag 1, index 0 (8 bits of zero), then bit 0.
        bits.push(1);
        bits.extend(std::iter::repeat_n(0u8, 9)); // 8 index bits + end bit
        // Pack MSB first.
        let mut out = vec![0u8; bits.len().div_ceil(8)];
        for (i, &bit) in bits.iter().enumerate() {
            if bit != 0 {
                out[i / 8] |= 0x80 >> (i % 8);
            }
        }
        out
    }

    #[test]
    fn decodes_literal_member() {
        // Nine header bytes then the file content; a literal-only stream.
        let header = [1u8, 8, 0, 0, 0, 0, 0, 0, 0];
        let content = b"HELLO WRAPTOR";
        let mut stream = header.to_vec();
        stream.extend_from_slice(content);
        let bitstream = literal_bitstream(&stream);

        let mut archive = MARKER.to_vec();
        archive.extend_from_slice(b"FILE");
        archive.push(0);
        archive.push(0x02); // type byte
        archive.extend_from_slice(&bitstream);

        let files = decompress_wr3(&archive).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "FILE");
        assert_eq!(files[0].content, content);
    }

    #[test]
    fn match_copy_expands_a_run() {
        // Header + a literal 0xAA, then a match of length 5 from window pos 10
        // (the literal, at window index 9 after the 9-byte header → position
        // 10). An overlapping copy must repeat the byte.
        let mut bits: Vec<u8> = Vec::new();
        let push_byte = |bits: &mut Vec<u8>, b: u8| {
            bits.push(0);
            for i in (0..8).rev() {
                bits.push((b >> i) & 1);
            }
        };
        // Nine header bytes (all zero) then one literal 0xAA at output index 9.
        for _ in 0..9 {
            push_byte(&mut bits, 0);
        }
        push_byte(&mut bits, 0xAA);
        // Match: flag 1, index = 10 (8 bits) → window position 9 (the 0xAA),
        // length 5.
        bits.push(1);
        for i in (0..8).rev() {
            bits.push((10u8 >> i) & 1);
        }
        for i in (0..5).rev() {
            bits.push((5u8 >> i) & 1);
        }
        // End.
        bits.push(1);
        bits.extend(std::iter::repeat_n(0u8, 9)); // 8 index bits + end bit

        let mut stream = vec![0u8; bits.len().div_ceil(8)];
        for (i, &bit) in bits.iter().enumerate() {
            if bit != 0 {
                stream[i / 8] |= 0x80 >> (i % 8);
            }
        }

        let mut archive = MARKER.to_vec();
        archive.extend_from_slice(b"R");
        archive.push(0);
        archive.push(0x02);
        archive.extend_from_slice(&stream);

        let files = decompress_wr3(&archive).unwrap();
        // Output: 9 header bytes + 0xAA + five more 0xAA = content is six 0xAA
        // after dropping the nine-byte header.
        assert_eq!(files[0].content, vec![0xAAu8; 6]);
    }
}
