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

/// Collected decimal value of all ASCII digits in a segment (ignoring the
/// surrounding spaces Lynx pads its numbers with).
fn parse_uint(seg: &[u8]) -> usize {
    seg.iter()
        .filter(|b| b.is_ascii_digit())
        .fold(0usize, |a, &b| a.saturating_mul(10).saturating_add((b - b'0') as usize))
}

/// Return the bytes up to (not including) the next CR, and the cursor past it.
fn read_line(bytes: &[u8], cur: usize) -> Option<(&[u8], usize)> {
    let rel = bytes.get(cur..)?.iter().position(|&b| b == 0x0D)?;
    Some((&bytes[cur..cur + rel], cur + rel + 1))
}

/// Parse a **Lynx** (`.lnx`) archive.
///
/// After an optional BASIC "un-lynx" loader, the header is PETSCII text: a line
/// carrying the header size in 254-byte blocks and the `*LYNX` signature, then a
/// file count, then per file a 16-byte name, block count, type char and
/// last-block byte count (CR-separated). File data begins at `blocks × 254`,
/// each file stored in whole blocks; a file's real length is
/// `(blocks - 1) × 254 + (last_bytes - 1)`.
pub fn parse_lnx(bytes: &[u8]) -> Result<Vec<ImportFile>> {
    // The signature sits in the first header block; find it and the number that
    // precedes it on the same line (the header size in blocks). The marker is
    // "*LYNX" — the asterisk distinguishes it from the word "LYNX" that appears
    // in the BASIC "USE LYNX TO DISSOLVE" loader string.
    let scan_end = bytes.len().min(512);
    let sig = bytes[..scan_end]
        .windows(5)
        .position(|w| w == b"*LYNX")
        .ok_or(Error::InvalidContainer)?
        + 1; // point at the "L"
    let line_start = bytes[..sig].iter().rposition(|&b| b == 0x0D).map_or(0, |i| i + 1);
    let dir_blocks = parse_uint(&bytes[line_start..sig]);
    if dir_blocks == 0 {
        return Err(Error::InvalidContainer);
    }

    // Skip to just past the signature line, then read the file count.
    let sig_cr = bytes[sig..]
        .iter()
        .position(|&b| b == 0x0D)
        .ok_or(Error::InvalidContainer)?
        + sig;
    let mut cur = sig_cr + 1;
    let (seg, nc) = read_line(bytes, cur).ok_or(Error::InvalidContainer)?;
    cur = nc;
    let num_files = parse_uint(seg);

    let mut files = Vec::new();
    let mut data_off = dir_blocks * 254;
    for _ in 0..num_files {
        let name = bytes.get(cur..cur + 16).ok_or(Error::InvalidContainer)?;
        cur += 16;
        if bytes.get(cur) == Some(&0x0D) {
            cur += 1;
        }
        let (seg, nc) = read_line(bytes, cur).ok_or(Error::InvalidContainer)?;
        cur = nc;
        let blocks = parse_uint(seg);
        let (seg, nc) = read_line(bytes, cur).ok_or(Error::InvalidContainer)?;
        cur = nc;
        let type_char = seg.iter().copied().find(|b| !b.is_ascii_whitespace()).unwrap_or(b'P');
        let (seg, nc) = read_line(bytes, cur).ok_or(Error::InvalidContainer)?;
        cur = nc;
        let last_bytes = parse_uint(seg);

        let file_type = match type_char {
            b'S' | b's' => FileType::Seq,
            b'U' | b'u' => FileType::Usr,
            b'R' | b'r' => FileType::Rel,
            _ => FileType::Prg,
        };
        if file_type == FileType::Rel {
            // REL files carry an extra record-length line.
            let (_, nc) = read_line(bytes, cur).ok_or(Error::InvalidContainer)?;
            cur = nc;
        }

        let length = if blocks == 0 {
            0
        } else {
            (blocks - 1) * 254 + last_bytes.saturating_sub(1)
        };
        let avail = bytes.len().saturating_sub(data_off);
        let data = bytes
            .get(data_off..data_off + length.min(avail))
            .ok_or(Error::InvalidContainer)?
            .to_vec();
        files.push(ImportFile {
            name: name_trimmed(name),
            file_type,
            data,
        });
        data_off += blocks * 254;
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

    /// Build a minimal Lynx archive (self-made BASIC stub + text directory).
    pub(crate) fn build_lnx(files: &[(&str, char, Vec<u8>)]) -> Vec<u8> {
        let blocks = |l: usize| l.div_ceil(254).max(1);
        let last = |l: usize| l - (blocks(l) - 1) * 254 + 1;
        // Minimal BASIC stub: load address + one "0 REM" line, program-terminated.
        let stub: &[u8] = &[0x01, 0x08, 0x09, 0x08, 0x00, 0x00, 0x8F, 0x00, 0x00, 0x00];
        let header = |db: usize| -> Vec<u8> {
            let mut s = stub.to_vec();
            s.extend_from_slice(format!("\r {db}  *LYNX TEST\r{}\r", files.len()).as_bytes());
            for (name, ty, data) in files {
                let mut nm = name.as_bytes().to_vec();
                nm.resize(16, b' ');
                s.extend_from_slice(&nm);
                s.extend_from_slice(
                    format!("\r{}\r{}\r{}\r", blocks(data.len()), ty, last(data.len())).as_bytes(),
                );
            }
            s
        };
        let mut db = 1;
        loop {
            let need = blocks(header(db).len());
            if need == db {
                break;
            }
            db = need;
        }
        let mut out = header(db);
        out.resize(db * 254, 0);
        for (_, _, data) in files {
            let start = out.len();
            out.extend_from_slice(data);
            out.resize(start + blocks(data.len()) * 254, 0);
        }
        out
    }

    #[test]
    fn parses_a_lynx_archive() {
        let f1 = vec![0x11u8; 100];
        let f2: Vec<u8> = (0..300u32).map(|x| x as u8).collect();
        let lnx = build_lnx(&[("FIRST", 'P', f1.clone()), ("SECOND", 'S', f2.clone())]);
        let files = parse_lnx(&lnx).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "FIRST");
        assert_eq!(files[0].file_type, FileType::Prg);
        assert_eq!(files[0].data, f1);
        assert_eq!(files[1].name, "SECOND");
        assert_eq!(files[1].file_type, FileType::Seq);
        assert_eq!(files[1].data, f2);
    }

    #[test]
    fn rejects_non_lynx() {
        assert_eq!(parse_lnx(b"no signature here at all").err(), Some(Error::InvalidContainer));
    }
}
