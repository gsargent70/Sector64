//! Reader for **CVT** (ConVerT) containers — the sequential form of a GEOS
//! file used to move it across non-GEOS media.
//!
//! A GEOS file is special: it has an INFO block, and its payload is either a
//! plain sequential chain or a **VLIR** (Variable Length Index Record) file of
//! up to 127 independent record chains. A CVT linearises all of that into one
//! file: a 254-byte **signature block** (the file's directory entry plus a
//! `"… formatted GEOS file"` signature), the **INFO block**, and — for VLIR —
//! a **record index** block followed by the record data concatenated.
//!
//! This module parses a CVT into the GEOS name, CBM and GEOS file types, the
//! structure (sequential vs VLIR), the signature, the INFO block, the VLIR
//! record index, and the data **split into records** ([`CvtInfo::record_data`]).
//! VLIR records are stored as whole 254-byte sectors strung together; each
//! record's content is `(sectors-1)*254 + (last_size-1)` bytes per the index,
//! the stream advances a full `sectors*254` per record, and the final record is
//! clamped to the data present. This split tiles both reference samples exactly
//! (the file ends mid-last-sector). There is no local GEOS tool to validate the
//! bytes against, so the split follows the documented record geometry rather
//! than an oracle. Every field access is bounds-checked.
//!
//! Reference: the CVT format as documented by Joe Forster/STA.

use crate::directory::FileType;
use crate::error::{Error, Result};

/// A CVT block is a 1541 block with its two link bytes dropped — 254 bytes.
const BLOCK: usize = 254;
/// Offset of the GEOS file-structure byte within the embedded directory entry
/// (directory slot position 23, less the two unused leading bytes).
const STRUCTURE_OFF: usize = 0x15;
/// Offset of the GEOS file-type byte.
const GEOS_TYPE_OFF: usize = 0x16;

/// How a GEOS file's payload is organised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeosStructure {
    /// A single sequential chain.
    Sequential,
    /// Up to 127 independent record chains, via an index block.
    Vlir,
}

/// What a parsed CVT holds.
#[derive(Debug, Clone)]
pub struct CvtInfo {
    /// The GEOS file name (PETSCII rendered to text).
    pub name: String,
    /// The CBM directory file type (GEOS files are USR).
    pub cbm_type: FileType,
    /// Sequential or VLIR payload.
    pub structure: GeosStructure,
    /// The GEOS file-type byte (see [`geos_type_name`]).
    pub geos_file_type: u8,
    /// The `"PRG/SEQ formatted GEOS file V1.0"` signature string.
    pub signature: String,
    /// The GEOS INFO block (its stored bytes).
    pub info_block: Vec<u8>,
    /// For VLIR: the record index as `(sector_count, last_sector_size)` pairs,
    /// in order (a `(0, 255)` pair marks a non-existent record). Empty for a
    /// sequential file.
    pub records: Vec<(u8, u8)>,
    /// The raw data region (all record chains concatenated, for VLIR; the
    /// sequential data otherwise).
    pub data: Vec<u8>,
    /// The data split into records (one entry per index slot; a non-existent
    /// VLIR record is empty). A sequential file has a single entry. Each
    /// record's length is `(sectors-1)*254 + (last_sector_size-1)` per the
    /// record index; the final record is clamped to the available data.
    pub record_data: Vec<Vec<u8>>,
}

/// A human name for a GEOS file-type byte.
pub fn geos_type_name(t: u8) -> &'static str {
    match t {
        0 => "non-GEOS",
        1 => "BASIC",
        2 => "assembly",
        3 => "data",
        4 => "system",
        5 => "desk accessory",
        6 => "application",
        7 => "application data",
        8 => "font",
        9 => "printer driver",
        10 => "input driver",
        11 => "disk device",
        12 => "system boot",
        13 => "temporary",
        14 => "auto-execute",
        _ => "unknown",
    }
}

/// Parse a CVT container.
pub fn parse_cvt(bytes: &[u8]) -> Result<CvtInfo> {
    // Need at least the signature block and INFO block.
    if bytes.len() < 2 * BLOCK {
        return Err(Error::InvalidContainer);
    }
    // The signature must be present and name a GEOS file.
    let signature_field = &bytes[0x1E..0x3A];
    let signature: String = signature_field
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect();
    if !signature.contains("formatted GEOS file") {
        return Err(Error::InvalidContainer);
    }

    let cbm_type = FileType::from_nibble(bytes[0] & 0x0F);
    let structure = if bytes[STRUCTURE_OFF] == 1 {
        GeosStructure::Vlir
    } else {
        GeosStructure::Sequential
    };
    let geos_file_type = bytes[GEOS_TYPE_OFF];
    // GEOS stores file names in ASCII (mixed case), not the C64 uppercase
    // PETSCII set, so render printable ASCII directly.
    let name: String = bytes[3..19]
        .iter()
        .take_while(|&&b| b != 0xA0 && b != 0x00)
        .map(|&b| if (0x20..=0x7E).contains(&b) { b as char } else { '.' })
        .collect();

    let info_block = bytes[BLOCK..2 * BLOCK].to_vec();

    let (records, data) = match structure {
        GeosStructure::Sequential => (Vec::new(), bytes[2 * BLOCK..].to_vec()),
        GeosStructure::Vlir => {
            // The record index is the third block.
            let rec_start = 2 * BLOCK;
            let rec_end = rec_start + BLOCK;
            let Some(rec) = bytes.get(rec_start..rec_end) else {
                // Index block missing: treat as header-only.
                return Ok(CvtInfo {
                    name,
                    cbm_type,
                    structure,
                    geos_file_type,
                    signature,
                    info_block,
                    records: Vec::new(),
                    data: Vec::new(),
                    record_data: Vec::new(),
                });
            };
            let mut records = Vec::new();
            let mut i = 0;
            while i + 1 < rec.len() {
                let (c, s) = (rec[i], rec[i + 1]);
                i += 2;
                if c == 0 && s == 0 {
                    break; // end of index
                }
                records.push((c, s));
            }
            let data = bytes.get(rec_end..).map(<[u8]>::to_vec).unwrap_or_default();
            (records, data)
        }
    };

    let record_data = split_records(structure, &records, &data);
    Ok(CvtInfo {
        name,
        cbm_type,
        structure,
        geos_file_type,
        signature,
        info_block,
        records,
        data,
        record_data,
    })
}

/// Split the concatenated data into records. VLIR records are stored as whole
/// 254-byte sectors strung together; each record's real content is
/// `(sectors-1)*254 + (last_size-1)` bytes, and the stream advances a full
/// `sectors*254` per record (the final record is clamped to the data). A
/// sequential file is one record holding all of `data`.
fn split_records(structure: GeosStructure, records: &[(u8, u8)], data: &[u8]) -> Vec<Vec<u8>> {
    if structure == GeosStructure::Sequential {
        return vec![data.to_vec()];
    }
    let mut out = Vec::with_capacity(records.len());
    let mut off = 0usize;
    for &(c, s) in records {
        if c == 0 {
            out.push(Vec::new()); // non-existent record, keep the index slot
            continue;
        }
        let content = (c as usize - 1) * BLOCK + (s as usize).saturating_sub(1);
        let end = off.saturating_add(content).min(data.len());
        out.push(data.get(off.min(data.len())..end).unwrap_or_default().to_vec());
        off = off.saturating_add(c as usize * BLOCK);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_cvt(name: &str, structure: u8, gtype: u8, sig: &str, records: &[(u8, u8)]) -> Vec<u8> {
        let mut b = vec![0u8; BLOCK];
        b[0] = 0xC3; // USR, closed+locked
        let nm = name.as_bytes();
        for (i, slot) in b[3..19].iter_mut().enumerate() {
            *slot = if i < nm.len() { nm[i] } else { 0xA0 };
        }
        b[STRUCTURE_OFF] = structure;
        b[GEOS_TYPE_OFF] = gtype;
        b[0x1E..0x1E + sig.len()].copy_from_slice(sig.as_bytes());
        b.extend_from_slice(&[0u8; BLOCK]); // info block
        if structure == 1 {
            let mut rec = Vec::new();
            for &(c, s) in records {
                rec.push(c);
                rec.push(s);
            }
            rec.push(0);
            rec.push(0);
            rec.resize(BLOCK, 0);
            b.extend_from_slice(&rec);
            b.extend_from_slice(&[0xEE; 100]); // some data
        }
        b
    }

    #[test]
    fn parses_a_vlir_cvt() {
        let cvt = build_cvt(
            "DESK TOP",
            1,
            4,
            "PRG formatted GEOS file V1.0",
            &[(64, 148), (8, 51)],
        );
        let info = parse_cvt(&cvt).unwrap();
        assert_eq!(info.name, "DESK TOP");
        assert_eq!(info.structure, GeosStructure::Vlir);
        assert_eq!(info.geos_file_type, 4);
        assert_eq!(geos_type_name(info.geos_file_type), "system");
        assert_eq!(info.records, vec![(64, 148), (8, 51)]);
        assert_eq!(info.info_block.len(), BLOCK);
        assert!(info.signature.starts_with("PRG formatted"));
    }

    #[test]
    fn parses_a_sequential_cvt() {
        let cvt = build_cvt("NOTES", 0, 3, "SEQ formatted GEOS file V1.0", &[]);
        let info = parse_cvt(&cvt).unwrap();
        assert_eq!(info.structure, GeosStructure::Sequential);
        assert!(info.records.is_empty());
    }

    #[test]
    fn rejects_non_cvt() {
        assert!(parse_cvt(&[0u8; 600]).is_err());
        assert!(parse_cvt(b"too short").is_err());
    }
}
