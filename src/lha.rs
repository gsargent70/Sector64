//! Reader for the **LHA** archive family as used on the C64 (`.lzh`, and the
//! self-extracting `.SFX`, which prepends a loader stub).
//!
//! An LHA archive is a sequence of entries, each a level-0 header followed by
//! the file's compressed data; a trailing zero byte (a header of size 0) ends
//! it. This module walks that structure — locating the first header past any
//! self-extractor stub by its checksum — and exposes each entry's name, CBM
//! type, method, sizes and CRC.
//!
//! Decoding is implemented only for the **stored** method (`-lh0-`); the
//! compressed methods (`-lh1-` = LZHUF and friends) return
//! [`Error::UnsupportedLhaMethod`]. The stored [`LhaEntry::crc16`] is kept as
//! the oracle a future decoder will be checked against. Every field access is
//! bounds-checked and the walk always advances, so malformed input cannot panic
//! or loop forever.

use crate::directory::FileType;
use crate::error::{Error, Result};

/// Level-0 header: `size, checksum, "-lhX-"` then fixed fields; the checksum
/// covers the header bytes after the first two.
const HEADER_MIN: usize = 22;

/// One archive member: its header fields and its raw compressed data.
#[derive(Debug, Clone)]
pub struct LhaEntry {
    /// File name (host text; a C64 name is followed by NUL and its CBM type).
    pub name: String,
    /// CBM file type parsed from the name field, defaulting to PRG.
    pub file_type: FileType,
    /// The five-byte method id, e.g. `-lh1-` or `-lh0-`.
    pub method: [u8; 5],
    /// Compressed (stored) size of the data that follows the header.
    pub compressed_size: u32,
    /// Original (uncompressed) size.
    pub original_size: u32,
    /// The archive's stored CRC-16 of the original data.
    pub crc16: u16,
    /// The raw compressed data for this entry (no header).
    pub packed: Vec<u8>,
}

impl LhaEntry {
    /// The method id as text (`-lh1-`), for display.
    pub fn method_str(&self) -> &str {
        core::str::from_utf8(&self.method).unwrap_or("?????")
    }

    /// Decode this entry to the original bytes.
    ///
    /// Only the stored method (`-lh0-`) is implemented; compressed methods
    /// return [`Error::UnsupportedLhaMethod`].
    pub fn decode(&self) -> Result<Vec<u8>> {
        if &self.method == b"-lh0-" {
            let n = self.original_size as usize;
            Ok(self.packed.get(..n).unwrap_or(&self.packed).to_vec())
        } else {
            Err(Error::UnsupportedLhaMethod(self.method))
        }
    }
}

/// Map a C64 LHA type character (`P`/`S`/`U`/`R`/`D`, either case) to a type.
fn file_type_of(c: u8) -> FileType {
    match c & 0xDF {
        b'S' => FileType::Seq,
        b'U' => FileType::Usr,
        b'R' => FileType::Rel,
        b'D' => FileType::Del,
        _ => FileType::Prg,
    }
}

/// Find the first level-0 header: a `-lhX-` method id at `pos+2` whose stored
/// checksum (at `pos+1`) matches the sum of the header's bytes. Scanning by the
/// checksum skips any self-extractor loader stub reliably.
fn first_header(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < HEADER_MIN {
        return None;
    }
    let limit = bytes.len() - HEADER_MIN;
    for pos in 0..=limit {
        if &bytes[pos + 2..pos + 5] == b"-lh" && bytes[pos + 6] == b'-' {
            let hsz = bytes[pos] as usize;
            if hsz < HEADER_MIN - 2 {
                continue;
            }
            let Some(hdr) = bytes.get(pos + 2..pos + 2 + hsz) else {
                continue;
            };
            let sum = hdr.iter().fold(0u8, |a, &b| a.wrapping_add(b));
            if sum == bytes[pos + 1] {
                return Some(pos);
            }
        }
    }
    None
}

/// Parse an LHA archive (or an `.SFX` self-extractor) into its entries.
pub fn parse_lha(bytes: &[u8]) -> Result<Vec<LhaEntry>> {
    let start = first_header(bytes).ok_or(Error::InvalidContainer)?;
    let mut pos = start;
    let mut entries = Vec::new();

    // The archive ends at a zero header-size byte, trailing padding, or EOF.
    // Any position that is not a well-formed `-lh` header stops the walk with
    // the entries gathered so far (the first header was already validated).
    loop {
        let hsz = match bytes.get(pos) {
            None | Some(0) => break,
            Some(&h) => h as usize,
        };
        let Some(header_end) = pos.checked_add(2 + hsz) else {
            break;
        };
        let Some(header) = bytes.get(pos..header_end) else {
            break;
        };
        if header.len() < HEADER_MIN {
            break;
        }

        let mut method = [0u8; 5];
        method.copy_from_slice(&header[2..7]);
        if &method[..3] != b"-lh" {
            break;
        }
        let compressed_size = u32::from_le_bytes([header[7], header[8], header[9], header[10]]);
        let original_size = u32::from_le_bytes([header[11], header[12], header[13], header[14]]);
        let name_len = header[21] as usize;
        let Some(name_field) = header.get(22..22 + name_len) else {
            break;
        };
        // CRC-16 follows the name.
        let crc16 = header
            .get(22 + name_len..24 + name_len)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .unwrap_or(0);

        // The C64 convention stores "<name>\0<type char>" in the name field.
        let (name_bytes, type_char) = match name_field.iter().position(|&b| b == 0) {
            Some(i) => (&name_field[..i], name_field.get(i + 1).copied()),
            None => (name_field, None),
        };
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        let file_type = type_char.map_or(FileType::Prg, file_type_of);

        // The compressed data follows the header.
        let Some(data_end) = header_end.checked_add(compressed_size as usize) else {
            break;
        };
        let Some(packed) = bytes.get(header_end..data_end.min(bytes.len())) else {
            break;
        };
        let packed = packed.to_vec();

        entries.push(LhaEntry {
            name,
            file_type,
            method,
            compressed_size,
            original_size,
            crc16,
            packed,
        });

        if data_end <= pos {
            break; // never advance backwards on degenerate input
        }
        pos = data_end;
    }

    if entries.is_empty() {
        return Err(Error::InvalidContainer);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal level-0 LHA archive from (name, type, method, data).
    fn build(entries: &[(&str, u8, &[u8; 5], &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, typ, method, data) in entries {
            let mut field = name.as_bytes().to_vec();
            field.push(0);
            field.push(*typ);
            let name_len = field.len();
            let hsz = 20 + name_len + 2; // fixed fields (through name_len byte) + name + crc
            let mut hdr = vec![hsz as u8, 0u8];
            hdr.extend_from_slice(*method); // 2..7
            hdr.extend_from_slice(&(data.len() as u32).to_le_bytes()); // 7..11
            hdr.extend_from_slice(&(data.len() as u32).to_le_bytes()); // 11..15
            hdr.extend_from_slice(&[0, 0, 0, 0]); // 15..19 timestamp
            hdr.push(0x20); // 19 attr
            hdr.push(0); // 20 level 0
            hdr.push(name_len as u8); // 21
            hdr.extend_from_slice(&field); // name
            hdr.extend_from_slice(&[0, 0]); // crc
            let sum = hdr[2..].iter().fold(0u8, |a, &b| a.wrapping_add(b));
            hdr[1] = sum;
            out.extend_from_slice(&hdr);
            out.extend_from_slice(data);
        }
        out.push(0); // terminator
        out
    }

    #[test]
    fn parses_stored_and_compressed_entries() {
        let stored = vec![0x01u8, 0x08, 0xDE, 0xAD];
        let comp = vec![0u8; 50];
        let arc = build(&[
            ("hello", b'P', b"-lh0-", &stored),
            ("notes", b'S', b"-lh1-", &comp),
        ]);
        let e = parse_lha(&arc).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].name, "hello");
        assert_eq!(e[0].file_type, FileType::Prg);
        assert_eq!(e[0].method_str(), "-lh0-");
        assert_eq!(e[0].decode().unwrap(), stored);
        assert_eq!(e[1].name, "notes");
        assert_eq!(e[1].file_type, FileType::Seq);
        assert!(matches!(e[1].decode(), Err(Error::UnsupportedLhaMethod(_))));
    }

    #[test]
    fn finds_header_past_a_stub() {
        let mut data = vec![0xAAu8; 300]; // fake loader stub
        data.extend_from_slice(&build(&[("x", b'P', b"-lh0-", &[1, 2, 3])]));
        let e = parse_lha(&data).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].name, "x");
    }

    #[test]
    fn rejects_non_lha() {
        assert!(parse_lha(b"no header here, just text").is_err());
        assert!(parse_lha(&[]).is_err());
    }
}
