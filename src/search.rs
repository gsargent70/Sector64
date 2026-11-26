//! Searching file contents for text or hex bytes.
//!
//! Text is matched the way a C64 shows it: a letter matches its unshifted
//! PETSCII code (`$41`-`$5A`), its ASCII lowercase code (`$61`-`$7A`) and its
//! shifted code (`$C1`-`$DA`), so `hello` finds `HELLO` whichever case set
//! wrote it, and a space also matches a shifted space (`$A0`). Hex patterns
//! are byte values with `??` for "any byte".

/// What one position of a pattern matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    /// Exactly this byte.
    Byte(u8),
    /// Any of these bytes (a letter in any case, or a space).
    OneOf([u8; 3]),
    /// Any byte.
    Any,
}

impl Unit {
    fn matches(self, b: u8) -> bool {
        match self {
            Unit::Byte(x) => b == x,
            Unit::OneOf(set) => set.contains(&b),
            Unit::Any => true,
        }
    }
}

/// A search pattern, built by [`Pattern::text`] or [`Pattern::hex`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    units: Vec<Unit>,
}

impl Pattern {
    /// A text pattern. Only printable ASCII is accepted (the characters a
    /// C64 and a PC keyboard share); letters match in either case.
    pub fn text(query: &str) -> Result<Pattern, String> {
        if query.is_empty() {
            return Err("type some text to look for".into());
        }
        let units = query
            .chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' => {
                    let k = c.to_ascii_uppercase() as u8 - b'A';
                    Ok(Unit::OneOf([0x41 + k, 0x61 + k, 0xC1 + k]))
                }
                ' ' => Ok(Unit::OneOf([0x20, 0xA0, 0x20])),
                '!'..='~' => Ok(Unit::Byte(c as u8)),
                _ => Err(format!(
                    "\"{c}\" can't be searched for as text; search for its byte in hex instead"
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Pattern { units })
    }

    /// A hex pattern: byte values such as `A9 00 8D` or `a9008d`, with `??`
    /// (or `?`) standing for any byte. A leading `$` or `0x` on a byte is
    /// allowed, and commas count as spaces.
    pub fn hex(query: &str) -> Result<Pattern, String> {
        let mut units = Vec::new();
        for word in query.split(|c: char| c.is_whitespace() || c == ',') {
            let word = word
                .strip_prefix('$')
                .or_else(|| word.strip_prefix("0x"))
                .or_else(|| word.strip_prefix("0X"))
                .unwrap_or(word);
            if word == "?" || word == "??" {
                units.push(Unit::Any);
                continue;
            }
            if word.len() % 2 == 1 {
                return Err(format!(
                    "\"{word}\" has an odd number of hex digits; write each byte as two"
                ));
            }
            for pair in word.as_bytes().chunks(2) {
                let pair = std::str::from_utf8(pair).unwrap_or("");
                if pair == "??" {
                    units.push(Unit::Any);
                    continue;
                }
                let b = u8::from_str_radix(pair, 16)
                    .map_err(|_| format!("\"{pair}\" isn't a hex byte (00 to FF, or ??)"))?;
                units.push(Unit::Byte(b));
            }
        }
        if units.is_empty() {
            return Err("type the bytes to look for, e.g. A9 00 8D".into());
        }
        if units.iter().all(|u| *u == Unit::Any) {
            return Err("give at least one byte that isn't ??".into());
        }
        Ok(Pattern { units })
    }

    /// How many bytes a match covers.
    pub fn len(&self) -> usize {
        self.units.len()
    }

    /// Whether the pattern is empty (never true for a built pattern).
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Whether the pattern matches `data` at `at`.
    pub fn matches_at(&self, data: &[u8], at: usize) -> bool {
        data.get(at..at + self.len())
            .is_some_and(|w| self.units.iter().zip(w).all(|(u, &b)| u.matches(b)))
    }

    /// Where the pattern matches in `data` (overlapping matches included),
    /// in order. Stops after `limit` matches; [`Self::count`] gives the total.
    pub fn find(&self, data: &[u8], limit: usize) -> Vec<usize> {
        (0..data.len().saturating_sub(self.len() - 1))
            .filter(|&i| self.matches_at(data, i))
            .take(limit)
            .collect()
    }

    /// How many times the pattern matches in `data`.
    pub fn count(&self, data: &[u8]) -> usize {
        (0..data.len().saturating_sub(self.len() - 1))
            .filter(|&i| self.matches_at(data, i))
            .count()
    }

    /// Whether a file name (as listed) contains the text, ignoring case.
    pub fn in_name(&self, name: &str) -> bool {
        // Letters already match either case; the listing's names are text.
        self.count(name.as_bytes()) > 0
    }
}

/// Where byte `offset` of a file lies on its disk: the block in `chain` that
/// holds it and the byte's position in that block (2 to 255; each block
/// carries 254 bytes after its two-byte link). `None` if the chain doesn't
/// fit a plain file of `len` bytes (REL and GEOS files are laid out
/// differently) or the offset is past the end.
pub fn block_of_offset(chain: &[(u8, u8)], len: usize, offset: usize) -> Option<(u8, u8, usize)> {
    if offset >= len || chain.len() != len.div_ceil(254) {
        return None;
    }
    let (t, s) = chain[offset / 254];
    Some((t, s, 2 + offset % 254))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_matches_any_case_set() {
        let p = Pattern::text("Hi there").unwrap();
        assert_eq!(p.len(), 8);
        assert!(p.matches_at(b"HI THERE", 0));
        assert!(p.matches_at(b"hi there", 0));
        // Shifted PETSCII capitals and a shifted space.
        assert!(p.matches_at(&[0xC8, 0x49, 0xA0, 0x54, 0x48, 0x45, 0x52, 0x45], 0));
        assert!(!p.matches_at(b"HI THER", 0));
        assert!(!p.matches_at(b"HO THERE", 0));
        assert_eq!(
            Pattern::text("1+1").unwrap().find(b"x1+1=2 1+1", 9),
            vec![1, 7]
        );
        assert!(Pattern::text("").is_err());
        assert!(Pattern::text("£").unwrap_err().contains("hex"));
    }

    #[test]
    fn hex_patterns_and_wildcards() {
        let p = Pattern::hex("A9 00 8D ?? D0").unwrap();
        assert_eq!(p.len(), 5);
        let data = [
            0xEA, 0xA9, 0x00, 0x8D, 0x20, 0xD0, 0xA9, 0x00, 0x8D, 0x21, 0xD0,
        ];
        assert_eq!(p.find(&data, 10), vec![1, 6]);
        assert_eq!(p.count(&data), 2);
        assert_eq!(p.find(&data, 1), vec![1]);
        assert_eq!(Pattern::hex("a9008d??d0").unwrap(), p);
        assert_eq!(Pattern::hex("$A9,$00, 0x8D ? $D0").unwrap(), p);
        assert!(Pattern::hex("A9 0").unwrap_err().contains("odd"));
        assert!(Pattern::hex("ZZ").unwrap_err().contains("isn't a hex byte"));
        assert!(Pattern::hex("  ").is_err());
        assert!(Pattern::hex("?? ??").is_err());
    }

    #[test]
    fn overlapping_and_edge_matches() {
        let p = Pattern::hex("AA AA").unwrap();
        assert_eq!(p.find(&[0xAA; 4], 10), vec![0, 1, 2]);
        assert!(p.find(&[0xAA], 10).is_empty());
        assert!(p.find(&[], 10).is_empty());
        assert_eq!(p.find(&[0, 0xAA, 0xAA], 10), vec![1]);
    }

    #[test]
    fn names_match_ignoring_case() {
        let p = Pattern::text("tools").unwrap();
        assert!(p.in_name("HD-TOOLS.64"));
        assert!(p.in_name("hd-tools.64"));
        assert!(!p.in_name("FCOPY"));
        assert!(!Pattern::text("longer than it").unwrap().in_name("short"));
    }

    #[test]
    fn offsets_map_to_disk_blocks() {
        let chain = [(17, 0), (17, 10), (17, 20)];
        // 600 bytes = 254 + 254 + 92.
        assert_eq!(block_of_offset(&chain, 600, 0), Some((17, 0, 2)));
        assert_eq!(block_of_offset(&chain, 600, 253), Some((17, 0, 255)));
        assert_eq!(block_of_offset(&chain, 600, 254), Some((17, 10, 2)));
        assert_eq!(block_of_offset(&chain, 600, 599), Some((17, 20, 93)));
        assert_eq!(block_of_offset(&chain, 600, 600), None);
        // A chain that isn't laid out like a plain file.
        assert_eq!(block_of_offset(&chain, 100, 0), None);
    }
}
