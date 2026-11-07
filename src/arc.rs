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
//! - **Decodes crunched entries** (methods 3 and 5): the C64 ARC LZW variant
//!   (big-endian codes read bit-reversed within each byte, 9..=12-bit with an
//!   early-change width bump and `first_free = 258`) over a run-length stream
//!   (escape `0xFE`, `FE count value`). Verified byte-for-byte against the
//!   original files of a real archive.
//! - **Decodes packed, squeezed, and squashed entries** (methods 1, 2, 4): the
//!   0xFE run-length stream (pack); a static Huffman code stored as a 256-entry
//!   table of 5-bit lengths plus bit-reversed codes, read least-significant bit
//!   first (squeeze); and that Huffman over the run-length stream (squash). The
//!   Huffman grammar was recovered by reverse-engineering the ARC 2.50 decoder
//!   and validated byte-for-byte against a real squashed entry.
//!
//! Every method 0–5 now decodes; only an unknown method byte returns
//! [`Error::UnsupportedArcMethod`]. The per-entry 16-bit [`ArcEntry::checksum`]
//! is captured (its algorithm is not yet identified).
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
            // Crunch (2-pass) and 1-pass crunch are the same codec: LZW over a
            // run-length-encoded stream.
            ArcMethod::Lz | ArcMethod::LzOnePass => {
                Ok(crunch_decode(&self.packed, self.orig_size as usize))
            }
            // Pack is the bare 0xFE run-length stream.
            ArcMethod::Rle => Ok(un_rle_fe(&self.packed, self.orig_size as usize)),
            // Squeeze is static Huffman; squash is the same Huffman over the
            // 0xFE run-length stream that crunch uses.
            ArcMethod::Huffman => Ok(squeeze_decode(&self.packed, self.orig_size as usize, false)),
            ArcMethod::HuffmanRle => Ok(squeeze_decode(&self.packed, self.orig_size as usize, true)),
            m => Err(Error::UnsupportedArcMethod(m.to_byte())),
        }
    }

}

/// Expand the ARC run-length stream (escape `0xFE`, `FE count value` → `value`
/// repeated `count` times) into at most `orig_size` bytes. Shared by crunch and
/// squash. A truncated trailing escape ends the stream.
fn un_rle_fe(rle: &[u8], orig_size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(orig_size.min(1 << 20));
    let mut i = 0;
    while i < rle.len() && out.len() < orig_size {
        let c = rle[i];
        i += 1;
        if c == 0xFE {
            let (Some(&count), Some(&value)) = (rle.get(i), rle.get(i + 1)) else {
                break;
            };
            i += 2;
            for _ in 0..count {
                out.push(value);
                if out.len() >= orig_size {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out.truncate(orig_size);
    out
}

/// A least-significant-bit-first bit reader over a byte slice. Reads past the
/// end yield zero bits rather than panicking.
struct SqueezeBits<'a> {
    data: &'a [u8],
    pos: usize,
    cur: u8,
    nbits: u8,
}

impl<'a> SqueezeBits<'a> {
    fn new(data: &'a [u8]) -> Self {
        SqueezeBits { data, pos: 0, cur: 0, nbits: 0 }
    }
    /// One bit, least-significant first within each byte.
    fn bit(&mut self) -> u32 {
        if self.nbits == 0 {
            self.cur = self.data.get(self.pos).copied().unwrap_or(0);
            self.pos += 1;
            self.nbits = 8;
        }
        let b = (self.cur & 1) as u32;
        self.cur >>= 1;
        self.nbits -= 1;
        b
    }
}

/// The widest Huffman code the ARC decoder accepts.
const SQ_MAX_LEN: usize = 24;

/// Decode an ARC **squeeze** (method 2, Huffman only) or **squash** (method 4,
/// Huffman over the 0xFE run-length stream) entry.
///
/// The packed data opens with a stored Huffman table — 256 symbols, each a
/// 5-bit code length (LSB-first) and, when the length is non-zero, that many
/// code bits — followed by the bit-coded data. The table is read, codes are
/// matched by accumulating bits (least-significant first, up to 24 bits), and
/// for squash the decoded bytes are run-length-expanded. Recovered by reverse-
/// engineering the ARC 2.50 decoder and validated byte-for-byte against a real
/// squashed entry. Everything is bounded; no input can panic or loop forever.
fn squeeze_decode(packed: &[u8], orig_size: usize, rle: bool) -> Vec<u8> {
    let mut br = SqueezeBits::new(packed);

    // Read the 256-entry code table, grouping entries by length so a decode
    // only scans the (small) bucket for the current bit count.
    let mut by_len: Vec<Vec<(u32, u8)>> = vec![Vec::new(); SQ_MAX_LEN + 1];
    let mut any = false;
    for sym in 0u16..256 {
        // 5-bit length, least-significant bit first.
        let mut length = 0u32;
        for i in 0..5 {
            length |= br.bit() << i;
        }
        let length = length as usize;
        if length == 0 || length > SQ_MAX_LEN {
            if length > SQ_MAX_LEN {
                return Vec::new(); // malformed table
            }
            continue;
        }
        // `length` code bits, first bit most-significant; then bit-reversed so
        // the first transmitted bit is the least-significant, matching how the
        // decoder accumulates.
        let mut msb = 0u32;
        for _ in 0..length {
            msb = (msb << 1) | br.bit();
        }
        let mut code = 0u32;
        for i in 0..length {
            code |= ((msb >> (length - 1 - i)) & 1) << i;
        }
        by_len[length].push((code, sym as u8));
        any = true;
    }
    if !any {
        return Vec::new();
    }

    // Decode symbols. For squash the Huffman output is the run-length stream, so
    // allow headroom before the run-length expansion truncates to `orig_size`.
    let sym_cap = if rle { orig_size.saturating_mul(2).saturating_add(64) } else { orig_size };
    let mut symbols: Vec<u8> = Vec::with_capacity(sym_cap.min(1 << 20));
    let total_bits = packed.len().saturating_mul(8);
    while symbols.len() < sym_cap {
        let mut acc = 0u32;
        let mut matched = None;
        for (nb, bucket) in by_len.iter().enumerate().skip(1) {
            acc |= br.bit() << (nb - 1);
            if let Some(&(_, sym)) = bucket.iter().find(|&&(code, _)| code == acc) {
                matched = Some(sym);
                break;
            }
        }
        match matched {
            Some(sym) => symbols.push(sym),
            None => break, // bad code / ran out of input
        }
        // Stop once the input is exhausted to avoid spinning on zero bits.
        if br.pos.saturating_sub(1).saturating_mul(8) >= total_bits && br.nbits == 0 {
            break;
        }
    }

    if rle {
        un_rle_fe(&symbols, orig_size)
    } else {
        symbols.truncate(orig_size);
        symbols
    }
}

/// Decode an ARC **crunch** stream (methods 3 and 5): LZW, then un-RLE.
///
/// The LZW is the C64 ARC variant — big-endian codes read bit-reversed within
/// each byte ("mix" order), 9..=12-bit codes with an early-change width bump,
/// and `first_free = 258` (codes 256/257 reserved). Its output is a run-length
/// stream whose escape is `0xFE`: `FE count value` expands to `value` repeated
/// `count` times (a literal `0xFE` is `FE 01 FE`). `orig_size` bounds the
/// result; everything is bounded so no input can panic or loop forever.
fn crunch_decode(packed: &[u8], orig_size: usize) -> Vec<u8> {
    // Generous cap: the RLE stream is usually smaller than the original, but a
    // literal 0xFE triples, so allow headroom before truncating.
    let cap = orig_size.saturating_mul(2).saturating_add(64);
    let rle = lzw_decode(packed, cap);
    un_rle_fe(&rle, orig_size)
}

/// The C64 ARC crunch LZW decoder. Produces at most `cap` bytes.
fn lzw_decode(data: &[u8], cap: usize) -> Vec<u8> {
    const MAX_BITS: u32 = 12;
    const SIZE: usize = 1 << 12;
    const CLEAR: usize = 256;
    const EOF: usize = 257;
    const FIRST: usize = 258;

    let mut prefix = [0u16; SIZE];
    let mut suffix: [u8; SIZE] = [0; SIZE];
    for (i, s) in suffix.iter_mut().enumerate().take(256) {
        *s = i as u8;
    }

    // Big-endian code assembled from bits taken LSB-first within each byte.
    let total_bits = data.len() * 8;
    let mut bitpos = 0usize;
    let mut read = |n_bits: u32| -> Option<u32> {
        if bitpos + n_bits as usize > total_bits {
            return None;
        }
        let mut v = 0u32;
        for _ in 0..n_bits {
            let bit = (data[bitpos >> 3] >> (bitpos & 7)) & 1;
            v = (v << 1) | bit as u32;
            bitpos += 1;
        }
        Some(v)
    };

    let mut n_bits = 9u32;
    let mut free = FIRST;
    let mut out = Vec::new();
    let mut stack = Vec::new();

    // Bump the code width one code early, as the ARC crunch encoder does.
    let grow = |n_bits: &mut u32, free: usize| {
        if free >= (1usize << *n_bits) - 1 && *n_bits < MAX_BITS {
            *n_bits += 1;
        }
    };

    grow(&mut n_bits, free);
    let mut old = match read(n_bits) {
        Some(c) => c as usize,
        None => return out,
    };
    let mut fin = (old & 0xff) as u8;
    out.push(fin);

    while out.len() < cap {
        grow(&mut n_bits, free);
        let Some(code) = read(n_bits) else { break };
        let mut code = code as usize;
        if code == EOF {
            break;
        }
        if code == CLEAR {
            n_bits = 9;
            free = FIRST;
            let Some(c) = read(n_bits) else { break };
            old = c as usize;
            fin = (old & 0xff) as u8;
            out.push(fin);
            continue;
        }
        if code > free {
            break; // invalid forward reference
        }
        let incode = code;
        if code == free {
            stack.push(fin);
            code = old;
        }
        let mut guard = 0;
        while code >= 256 {
            guard += 1;
            if guard > SIZE || code >= SIZE {
                return out;
            }
            stack.push(suffix[code]);
            code = prefix[code] as usize;
        }
        fin = suffix[code];
        stack.push(fin);
        while let Some(b) = stack.pop() {
            out.push(b);
        }
        if free < SIZE {
            prefix[free] = old as u16;
            suffix[free] = fin;
            free += 1;
        }
        old = incode;
    }
    out
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
    fn unknown_method_is_unsupported_not_panic() {
        // An unknown method byte (7) must error, not panic.
        let mut a = vec![2, 7, 0, 0, 4, 0, 0, 1, 0, b'P', 1, b'X'];
        a.extend_from_slice(&[0xfe, 0x21, 0x00]); // v2 record-len + date
        a.resize(BLOCK, 0);
        a.push(0x1a);
        let entries = parse_arc(&a).unwrap();
        assert_eq!(entries[0].method, ArcMethod::Other(7));
        assert!(matches!(
            entries[0].decode(),
            Err(Error::UnsupportedArcMethod(7))
        ));
    }

    #[test]
    fn pack_rle_round_trips() {
        // Method 1 (pack) is the bare 0xFE run-length stream.
        let mut a = vec![1, 1, 0, 0, 6, 0, 0, 1, 0, b'S', 1, b'X'];
        a.extend_from_slice(&[0xfe, 0x04, 0xAA, b'Z', b'Z']); // AAAA ZZ
        a.resize(BLOCK, 0);
        a.push(0x1a);
        let e = parse_arc(&a).unwrap();
        assert_eq!(e[0].method, ArcMethod::Rle);
        assert_eq!(e[0].decode().unwrap(), vec![0xAA, 0xAA, 0xAA, 0xAA, b'Z', b'Z']);
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
