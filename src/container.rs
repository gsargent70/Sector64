//! Readers for file-container formats, for importing into a disk image.
//!
//! These parse an archive's bytes into a list of [`ImportFile`]s, each carrying
//! the CBM name and the file's raw bytes (for a PRG, including its two-byte load
//! address — the same representation [`crate::Disk::write_file`] stores and
//! `c1541` reads back). Parsing is bounds-checked and never panics on malformed
//! input.

use crate::directory::FileType;
use crate::error::{Error, Result};
use crate::petscii;

/// One file extracted from a container.
#[derive(Debug, Clone)]
pub struct ImportFile {
    pub name: String,
    pub file_type: FileType,
    pub data: Vec<u8>,
}

/// Decode a fixed field as display text, trimming trailing spaces and NULs
/// (T64 pads names with spaces, not the directory's shifted-space).
fn name_trimmed(field: &[u8]) -> String {
    let end = field
        .iter()
        .rposition(|&b| b != 0x20 && b != 0x00 && b != petscii::PAD)
        .map_or(0, |i| i + 1);
    field[..end].iter().map(|&b| petscii::char_of(b)).collect()
}

/// Parse a **T64** tape archive.
///
/// The 64-byte header holds the entry count; each 32-byte directory entry gives
/// the load/end addresses, the offset of the file's data, and its name. A PRG is
/// reconstructed as the little-endian load address followed by the data.
pub fn parse_t64(bytes: &[u8]) -> Result<Vec<ImportFile>> {
    if bytes.len() < 0x40 || !bytes[..0x20].starts_with(b"C64") {
        return Err(Error::InvalidContainer);
    }
    let max_entries = u16::from_le_bytes([bytes[0x22], bytes[0x23]]) as usize;

    let mut files = Vec::new();
    for i in 0..max_entries {
        let eo = 0x40 + i * 0x20;
        let Some(e) = bytes.get(eo..eo + 0x20) else {
            break;
        };
        if e[0] == 0 {
            continue; // free / unused slot
        }
        if e[0] != 1 {
            continue; // not a normal tape file (e.g. a memory snapshot)
        }
        let start = u16::from_le_bytes([e[2], e[3]]);
        let end = u16::from_le_bytes([e[4], e[5]]);
        let data_off = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;

        // Length from the addresses, bounded by the bytes actually present (some
        // T64s store a bogus end address).
        let claimed = end.saturating_sub(start) as usize;
        let avail = bytes.len().saturating_sub(data_off);
        let len = claimed.min(avail);
        let data = bytes
            .get(data_off..data_off + len)
            .ok_or(Error::InvalidContainer)?;

        let mut prg = Vec::with_capacity(2 + len);
        prg.extend_from_slice(&start.to_le_bytes());
        prg.extend_from_slice(data);
        files.push(ImportFile {
            name: name_trimmed(&e[0x10..0x20]),
            file_type: FileType::Prg,
            data: prg,
        });
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_t64(entries: &[(&str, u16, &[u8])]) -> Vec<u8> {
        let n = entries.len();
        let mut hdr = b"C64S tape image file\0".to_vec();
        hdr.resize(0x20, 0);
        hdr.extend_from_slice(&0x0101u16.to_le_bytes());
        hdr.extend_from_slice(&(n as u16).to_le_bytes()); // max entries
        hdr.extend_from_slice(&(n as u16).to_le_bytes()); // used
        hdr.extend_from_slice(&0u16.to_le_bytes());
        let mut name = b"TEST TAPE".to_vec();
        name.resize(24, b' ');
        hdr.extend_from_slice(&name);

        let data_off = 0x40 + 0x20 * n;
        let mut dir = Vec::new();
        let mut blob = Vec::new();
        let mut off = data_off;
        for (nm, load, data) in entries {
            dir.push(1u8); // entry used
            dir.push(0x82); // PRG
            dir.extend_from_slice(&load.to_le_bytes());
            dir.extend_from_slice(&(load + data.len() as u16).to_le_bytes());
            dir.extend_from_slice(&0u16.to_le_bytes());
            dir.extend_from_slice(&(off as u32).to_le_bytes());
            dir.extend_from_slice(&0u32.to_le_bytes());
            let mut f = nm.as_bytes().to_vec();
            f.resize(16, b' ');
            dir.extend_from_slice(&f);
            blob.extend_from_slice(data);
            off += data.len();
        }
        [hdr, dir, blob].concat()
    }

    #[test]
    fn parses_two_file_t64() {
        let f1: Vec<u8> = (0..200u32).map(|x| x as u8).collect();
        let f2 = vec![0xAAu8; 50];
        let img = build_t64(&[("HELLO", 0x0801, &f1), ("WORLD", 0x1000, &f2)]);
        let files = parse_t64(&img).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "HELLO");
        assert_eq!(files[0].data, [&0x0801u16.to_le_bytes()[..], &f1].concat());
        assert_eq!(files[1].name, "WORLD");
        assert_eq!(files[1].data, [&0x1000u16.to_le_bytes()[..], &f2].concat());
    }

    #[test]
    fn rejects_non_t64() {
        assert_eq!(parse_t64(b"not a tape at all..............").err(), Some(Error::InvalidContainer));
        assert_eq!(parse_t64(&[]).err(), Some(Error::InvalidContainer));
    }
}
