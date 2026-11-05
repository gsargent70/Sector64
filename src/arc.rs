//! Reader for the Commodore **ARC** archive format (ARC230 and compatibles).
//!
//! This is the C64/C128 ARC format, which is unrelated to the MS-DOS ARC
//! format of the same name. An archive is a bare concatenation of entries with
//! no signature: each entry is a fixed header, the Pascal-style file name, and
//! the entry's packed data, block-aligned to 254-byte units.
//!
//! What this module does today, and does safely against hostile input:
//!
//! - **Walks the archive** ([`parse_arc`]) into [`ArcEntry`]s, validating every
//!   header field and the block arithmetic, rejecting cycles/overflow, and
//!   stopping at the trailing padding that ends a real archive.
//! - **Decodes stored entries** ([`ArcEntry::decode`], method 0): a byte copy.
//!
//! What it does *not* do yet: the compressed methods (RLE, Huffman, LZ — the
//! methods every real-world archive actually uses). [`ArcEntry::decode`]
//! returns [`Error::UnsupportedArcMethod`] for those. The per-entry 16-bit
//! [`ArcEntry::checksum`] is captured so that, once a decoder exists, its
//! output can be verified against the archive's own checksum — the only
//! independent oracle available, since no local tool dissolves ARC.
//!
//! Reference: the Commodore ARC format as documented from observed archives
//! (header: version, method, checksum, 3-byte original size, 2-byte packed size
//! in blocks, file-type character, Pascal file name; version-2 headers add a
//! record length and an MS-DOS date word).

use crate::directory::FileType;
use crate::error::{Error, Result};

/// A 1541 block's worth of data bytes — ARC sizes its entries in these units.
const BLOCK: usize = 254;

/// The compression method an entry's data is packed with.
///
/// The numbering is the Commodore ARC one (distinct from MS-DOS ARC). Version-1
/// headers use methods 0–2; version-2 adds 3–5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcMethod {
    /// 0: stored with no compression.
    Stored,
    /// 1: run-length encoding.
    Rle,
    /// 2: Huffman coding.
    Huffman,
    /// 3: LZ compression.
    Lz,
    /// 4: Huffman coding over an RLE stream.
    HuffmanRle,
    /// 5: single-pass LZ compression.
    LzOnePass,
    /// An unrecognized method byte (preserved so listing still works).
    Other(u8),
}

impl ArcMethod {
    fn from_byte(b: u8) -> Self {
        match b {
            0 => ArcMethod::Stored,
            1 => ArcMethod::Rle,
            2 => ArcMethod::Huffman,
            3 => ArcMethod::Lz,
            4 => ArcMethod::HuffmanRle,
            5 => ArcMethod::LzOnePass,
            other => ArcMethod::Other(other),
        }
    }

    /// The raw method byte.
    pub fn to_byte(self) -> u8 {
        match self {
            ArcMethod::Stored => 0,
            ArcMethod::Rle => 1,
            ArcMethod::Huffman => 2,
            ArcMethod::Lz => 3,
            ArcMethod::HuffmanRle => 4,
            ArcMethod::LzOnePass => 5,
            ArcMethod::Other(b) => b,
        }
    }

    /// A short human label.
    pub fn label(self) -> &'static str {
        match self {
            ArcMethod::Stored => "stored",
            ArcMethod::Rle => "rle",
            ArcMethod::Huffman => "huffman",
            ArcMethod::Lz => "lz",
            ArcMethod::HuffmanRle => "huffman+rle",
            ArcMethod::LzOnePass => "lz-1pass",
            ArcMethod::Other(_) => "unknown",
        }
    }
}

/// One entry in an ARC archive: its header fields and its packed bytes.
#[derive(Debug, Clone)]
pub struct ArcEntry {
    /// Header version (1 or 2).
    pub version: u8,
    /// How [`Self::packed`] is compressed.
    pub method: ArcMethod,
    /// The archive's stored 16-bit checksum of the *original* file data.
    pub checksum: u16,
    /// Size of the original (uncompressed) file data, in bytes.
    pub orig_size: u32,
    /// Size of the packed entry, in 254-byte blocks (header included).
    pub blocks: u16,
    /// The original file's CBM type.
    pub file_type: FileType,
    /// The original file name (PETSCII rendered to text).
    pub name: String,
    /// Record length (version-2 only; meaningful only for REL files).
    pub record_len: Option<u8>,
    /// MS-DOS packed date word (version-2 only), if present.
    pub date: Option<u16>,
    /// The raw packed data for this entry (no header).
    pub packed: Vec<u8>,
}

impl ArcEntry {
    /// Decode this entry to the original file bytes.
    ///
    /// Only [`ArcMethod::Stored`] is implemented; every compressed method
    /// returns [`Error::UnsupportedArcMethod`]. A decoded stored entry is
    /// truncated/validated to [`Self::orig_size`].
    pub fn decode(&self) -> Result<Vec<u8>> {
        match self.method {
            ArcMethod::Stored => {
                let n = self.orig_size as usize;
                if self.packed.len() < n {
                    return Err(Error::InvalidContainer);
                }
                Ok(self.packed[..n].to_vec())
            }
            m => Err(Error::UnsupportedArcMethod(m.to_byte())),
        }
    }

    /// The additive 16-bit checksum of `data`, for comparison with
    /// [`Self::checksum`]. The algorithm (a wrapping sum of every byte) is the
    /// conventional ARC checksum but is **not yet verified** against a known
    /// decoded file, so treat a match as corroboration, not proof.
    pub fn verify_checksum(&self, data: &[u8]) -> bool {
        checksum16(data) == self.checksum
    }
}

/// A wrapping 16-bit additive checksum (the conventional ARC algorithm).
pub fn checksum16(data: &[u8]) -> u16 {
    let mut sum: u16 = 0;
    for &b in data {
        sum = sum.wrapping_add(b as u16);
    }
    sum
}

/// Map an ARC file-type character (PETSCII `D`/`P`/`S`/`U`, either case) to a
/// [`FileType`]. Anything unrecognized is treated as USR.
fn file_type_of(c: u8) -> FileType {
    match c & 0xDF {
        b'D' => FileType::Del,
        b'S' => FileType::Seq,
        b'P' => FileType::Prg,
        b'U' => FileType::Usr,
        b'R' => FileType::Rel,
        _ => FileType::Usr,
    }
}

/// Render an ARC name field (PETSCII) to a display string.
fn arc_name(field: &[u8]) -> String {
    field.iter().map(|&b| crate::petscii::char_of(b)).collect()
}

/// The base header length up to and including the name-length byte.
const HEADER_BASE: usize = 11;

/// Parse a **self-dissolving archive** (`.SDA`): an ARC archive with a C64
/// BASIC+ML dissolver prepended.
///
/// The dissolver is a program that loads at `$0801`; per the ARC convention the
/// embedded archive starts at `(first BASIC line number − 6) × 254`, minus one
/// more byte when the `SYS` argument begins with the digit `7`. Everything past
/// that point is an ordinary ARC archive, so this strips the stub and defers to
/// [`parse_arc`].
pub fn parse_sda(bytes: &[u8]) -> Result<Vec<ArcEntry>> {
    let start = sda_archive_offset(bytes).ok_or(Error::InvalidContainer)?;
    let archive = bytes.get(start..).ok_or(Error::InvalidContainer)?;
    // The computed offset must land on an ARC header version byte.
    if archive.first().is_none_or(|&v| v != 1 && v != 2) {
        return Err(Error::InvalidContainer);
    }
    parse_arc(archive)
}

/// The offset of the embedded ARC archive within an SDA, from the BASIC loader.
fn sda_archive_offset(bytes: &[u8]) -> Option<usize> {
    // A C64 BASIC program loads at $0801.
    if bytes.len() < 16 || bytes[0] != 0x01 || bytes[1] != 0x08 {
        return None;
    }
    // bytes[2..4] is the next-line link; bytes[4..6] is the first line number.
    let line = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let mut off = line.checked_sub(6)?.checked_mul(254)?;
    // The ARC convention: a SYS argument starting with '7' means one byte less.
    if sys_argument(bytes).and_then(|a| a.first().copied()) == Some(b'7') {
        off = off.checked_sub(1)?;
    }
    Some(off)
}

/// The decimal `SYS` argument digits in a BASIC loader line (token `$9E`).
fn sys_argument(bytes: &[u8]) -> Option<&[u8]> {
    let p = bytes.iter().position(|&b| b == 0x9E)?;
    let rest = bytes.get(p + 1..)?;
    let ds = rest.iter().position(|b| b.is_ascii_digit())?;
    let d = &rest[ds..];
    let de = d.iter().position(|b| !b.is_ascii_digit()).unwrap_or(d.len());
    Some(&d[..de])
}

/// Parse an ARC archive into its entries.
///
/// `bytes` must start at the archive header (self-dissolving `.SDA`/`.SFX`
/// files prepend a BASIC loader that must be stripped first — not handled
/// here). The walk stops at the first byte that is not a valid header version,
/// which is how a real archive's trailing block padding is detected. Every
/// field access is bounds-checked; malformed input yields
/// [`Error::InvalidContainer`] rather than a panic.
pub fn parse_arc(bytes: &[u8]) -> Result<Vec<ArcEntry>> {
    let mut entries = Vec::new();
    let mut pos = 0usize;

    while pos + HEADER_BASE <= bytes.len() {
        let version = bytes[pos];
        // A byte that is not a header version marks the end (padding/EOF byte).
        if version != 1 && version != 2 {
            break;
        }

        let method = ArcMethod::from_byte(bytes[pos + 1]);
        let checksum = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]);
        let orig_size =
            u32::from_le_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], 0]);
        let blocks = u16::from_le_bytes([bytes[pos + 7], bytes[pos + 8]]);
        let file_type = file_type_of(bytes[pos + 9]);
        let name_len = bytes[pos + 10] as usize;

        // Name must fit.
        let name_start = pos + HEADER_BASE;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or(Error::InvalidContainer)?;
        if name_end > bytes.len() {
            return Err(Error::InvalidContainer);
        }
        let name = arc_name(&bytes[name_start..name_end]);

        // Version-2 adds a record-length byte and a two-byte MS-DOS date.
        let (record_len, date, header_end) = if version == 2 {
            let ext_end = name_end.checked_add(3).ok_or(Error::InvalidContainer)?;
            if ext_end > bytes.len() {
                return Err(Error::InvalidContainer);
            }
            (
                Some(bytes[name_end]),
                Some(u16::from_le_bytes([bytes[name_end + 1], bytes[name_end + 2]])),
                ext_end,
            )
        } else {
            (None, None, name_end)
        };

        // A zero block count can't advance the cursor; reject rather than loop.
        if blocks == 0 {
            return Err(Error::InvalidContainer);
        }

        // The whole entry (header + packed data) spans `blocks` 254-byte units
        // from the header start. The packed data is what's left after the
        // header, clamped to the end of the file for a final short entry.
        let entry_span = (blocks as usize)
            .checked_mul(BLOCK)
            .ok_or(Error::InvalidContainer)?;
        let next_pos = pos.checked_add(entry_span).ok_or(Error::InvalidContainer)?;
        let packed_end = next_pos.min(bytes.len());
        // header_end may exceed packed_end only on a corrupt tiny final entry.
        let packed = if packed_end > header_end {
            bytes[header_end..packed_end].to_vec()
        } else {
            Vec::new()
        };

        entries.push(ArcEntry {
            version,
            method,
            checksum,
            orig_size,
            blocks,
            file_type,
            name,
            record_len,
            date,
            packed,
        });

        // Guard against a non-advancing walk on degenerate input.
        if next_pos <= pos {
            break;
        }
        pos = next_pos;
    }

    if entries.is_empty() {
        return Err(Error::InvalidContainer);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_garbage() {
        assert!(parse_arc(&[]).is_err());
        assert!(parse_arc(&[0xff, 0x00, 0x00]).is_err());
    }

    #[test]
    fn parses_one_stored_v1_entry() {
        // v1, stored, checksum 0x0102, orig size 4, 1 block, type 'P', name "AB".
        let mut a = vec![
            1, 0, 0x02, 0x01, 4, 0, 0, 1, 0, b'P', 2, b'A', b'B',
        ];
        a.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]); // 4 stored bytes
        a.resize(BLOCK, 0); // pad to one block
        a.push(0x00); // terminator byte (not a version)
        let entries = parse_arc(&a).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.version, 1);
        assert_eq!(e.method, ArcMethod::Stored);
        assert_eq!(e.orig_size, 4);
        assert_eq!(e.name, "AB");
        assert_eq!(e.file_type, FileType::Prg);
        assert_eq!(e.decode().unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn compressed_methods_are_unsupported_not_panics() {
        let mut a = vec![2, 3, 0, 0, 4, 0, 0, 1, 0, b'P', 1, b'X'];
        a.extend_from_slice(&[0xfe, 0x21, 0x00]); // v2 record-len + date
        a.resize(BLOCK, 0);
        a.push(0x1a);
        let entries = parse_arc(&a).unwrap();
        assert_eq!(entries[0].method, ArcMethod::Lz);
        assert!(matches!(
            entries[0].decode(),
            Err(Error::UnsupportedArcMethod(3))
        ));
    }

    #[test]
    fn zero_block_count_is_rejected() {
        let a = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, b'P', 1, b'X', 0, 0, 0, 0];
        assert!(parse_arc(&a).is_err());
    }

    #[test]
    fn parses_an_sda_after_its_basic_stub() {
        // A BASIC loader at $0801 whose first line number is 7, so the embedded
        // ARC starts at (7 - 6) * 254 = 254.
        let mut sda = vec![0x01, 0x08]; // load address
        sda.extend_from_slice(&[0x0d, 0x08]); // next-line link
        sda.extend_from_slice(&[7, 0]); // line number 7
        sda.push(0x9e); // SYS token
        sda.extend_from_slice(b"2061"); // SYS argument (not starting with 7)
        sda.resize(254, 0); // pad the stub to the archive start
        // One stored ARC entry at offset 254.
        let mut entry = vec![1, 0, 0x02, 0x01, 4, 0, 0, 1, 0, b'P', 2, b'A', b'B'];
        entry.extend_from_slice(&[1, 2, 3, 4]);
        entry.resize(BLOCK, 0);
        sda.extend_from_slice(&entry);
        sda.push(0x1a); // terminator

        let entries = parse_sda(&sda).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "AB");
        assert_eq!(entries[0].decode().unwrap(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn rejects_sda_that_is_not_basic() {
        assert_eq!(parse_sda(b"not a basic program").err(), Some(Error::InvalidContainer));
    }
}
