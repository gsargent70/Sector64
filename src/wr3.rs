//! Reader for **Wraptor 3** (`.wr3`) archives.
//!
//! A Wraptor-3 archive is a sequence of entries, each introduced by the
//! four-byte marker `FF "BL" FF` followed by the file's NUL-terminated name and
//! then its compressed data. The marker does not occur inside the compressed
//! streams (every one in real archives is followed by a clean name), so the
//! entries are recovered by scanning for markers and validating the name that
//! must follow each one.
//!
//! This parses the container and lists the entries by name. The payload is
//! Wraptor-compressed with a custom scheme; decoding it is not implemented, so
//! [`Wr3Entry::compressed`] is exposed as-is. Every access is bounds-checked.

use crate::error::{Error, Result};

/// The per-entry marker: `0xFF 'B' 'L' 0xFF`.
const MARKER: [u8; 4] = [0xFF, 0x42, 0x4C, 0xFF];
/// A name may be at most this many bytes before its terminating NUL.
const MAX_NAME: usize = 24;

/// One archive member: its name and its raw (still compressed) data.
#[derive(Debug, Clone)]
pub struct Wr3Entry {
    /// The member's file name.
    pub name: String,
    /// The member's compressed data (everything between the name's NUL and the
    /// next marker). Decoding is not implemented.
    pub compressed: Vec<u8>,
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
}
