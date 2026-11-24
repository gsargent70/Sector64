//! Turn the text of a BASIC program into a runnable PRG: the reverse of
//! [`crate::basic`].
//!
//! The text is one program line per line (`10 PRINT "HI"`), in either of the
//! two usual ways of writing PETSCII letters, told apart by how the program's
//! own keywords are written:
//!
//! - as this crate lists programs: unshifted letters are capitals and shifted
//!   ones are lower case;
//! - as VICE's `petcat` lists them: the other way round.
//!
//! Characters that can't be typed are written in braces: control codes by
//! name (`{CLR}`, `{5 DOWN}`, `{rvon}`, `{down*3}`), keys with a modifier
//! (`{SHIFT-A}`, `{CBM-+}`, `{CTRL-C}`), or any byte by number (`{$93}`,
//! `{147}`).
//!
//! Keywords are found the way the machine's own editor does it: anywhere
//! outside quotes, `REM` and `DATA`, with or without spaces around them, and
//! in their abbreviated forms (`?`, or a shifted letter: `pO` is `POKE`).

use crate::basic::{
    Dialect, KEYWORDS, MAX_LINE_BYTES, MAX_LINE_NUMBER, SIMONS, V7_CE, V7_FE, V7_KEYWORDS,
};

/// A tokenized program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokenized {
    /// The PRG file: its load address, then the program.
    pub prg: Vec<u8>,
    pub load_address: u16,
    /// Number of program lines.
    pub lines: usize,
    /// Things that are allowed but probably weren't meant.
    pub warnings: Vec<TextProblem>,
}

/// Something wrong with a line of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextProblem {
    /// The line of the text, from 1 (not its BASIC line number).
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for TextProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "text line {}: {}", self.line, self.message)
    }
}

impl Dialect {
    /// Where the dialect's machine puts a BASIC program: $1C01 on a C128,
    /// $0801 on a C64.
    pub fn load_address(self) -> u16 {
        match self {
            Dialect::V7 => 0x1C01,
            Dialect::V2 | Dialect::Simons => 0x0801,
        }
    }
}

/// Tokenize `text` as `dialect`. The program loads at `load`, or else where a
/// petcat heading (`;name ==0801==`) says, or else at the dialect's usual
/// address. Blank lines and lines starting with `;` are skipped. Lines keep
/// the order they're in.
///
/// Fails with every problem found, in order.
pub fn tokenize(
    text: &str,
    dialect: Dialect,
    load: Option<u16>,
) -> Result<Tokenized, Vec<TextProblem>> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let shifted = shifted_case(text);
    let words = keywords(dialect);
    let mut heading = None;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut lines: Vec<(u16, Vec<u8>)> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(h) = trimmed.strip_prefix(';') {
            heading = heading.or_else(|| heading_address(h));
            continue;
        }
        match program_line(trimmed, shifted, &words) {
            Ok((number, body)) => {
                if let Some(&(last, _)) = lines.last().filter(|(last, _)| *last >= number) {
                    warnings.push(TextProblem {
                        line: i + 1,
                        message: format!("{number} comes after {last}"),
                    });
                }
                lines.push((number, body));
            }
            Err(message) => errors.push(TextProblem {
                line: i + 1,
                message,
            }),
        }
    }
    let whole = |message: &str| TextProblem {
        line: 1,
        message: message.into(),
    };
    if lines.is_empty() && errors.is_empty() {
        errors.push(whole("there's no program"));
    }
    let load = load.or(heading).unwrap_or(dialect.load_address());
    let size: usize = lines.iter().map(|(_, b)| b.len() + 5).sum::<usize>() + 2;
    if usize::from(load) + size > 0x1_0000 {
        errors.push(whole("the program doesn't fit in memory"));
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut prg = load.to_le_bytes().to_vec();
    let mut at = load;
    for (number, body) in &lines {
        at += body.len() as u16 + 5;
        prg.extend_from_slice(&at.to_le_bytes());
        prg.extend_from_slice(&number.to_le_bytes());
        prg.extend_from_slice(body);
        prg.push(0);
    }
    prg.extend_from_slice(&[0, 0]);
    Ok(Tokenized {
        prg,
        load_address: load,
        lines: lines.len(),
        warnings,
    })
}

/// Which letters of the text are shifted ones.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shifted {
    Lower,
    Upper,
}

/// Judge by the letters outside quotes, braces and remarks: whichever case
/// the keywords and variables are in is the unshifted one.
fn shifted_case(text: &str) -> Shifted {
    let (mut lower, mut upper) = (0usize, 0usize);
    for line in text.lines() {
        if line.trim_start().starts_with(';') {
            continue;
        }
        let (mut quoted, mut braced) = (false, false);
        let mut word = String::new();
        for c in line.chars() {
            match c {
                '"' => quoted = !quoted,
                '{' => braced = true,
                '}' => braced = false,
                _ if quoted || braced => {}
                c if c.is_ascii_lowercase() => lower += 1,
                c if c.is_ascii_uppercase() => upper += 1,
                _ => {}
            }
            if quoted || braced {
                continue;
            }
            word.push(c.to_ascii_uppercase());
            if word.ends_with("REM") {
                break;
            }
        }
    }
    if lower > upper {
        Shifted::Upper
    } else {
        Shifted::Lower
    }
}

/// The address in a petcat heading, `name.prg ==0801==`.
fn heading_address(h: &str) -> Option<u16> {
    let (_, rest) = h.split_once("==")?;
    let (hex, _) = rest.split_once("==")?;
    u16::from_str_radix(hex.trim(), 16).ok()
}

/// One character of a line. A byte written in braces is kept as it is; a
/// typed one may be part of a keyword.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ch {
    Typed(u8),
    Byte(u8),
}

impl Ch {
    fn byte(self) -> u8 {
        match self {
            Ch::Typed(b) | Ch::Byte(b) => b,
        }
    }
}

/// A line's number and tokenized body.
fn program_line(
    line: &str,
    shifted: Shifted,
    words: &[(&str, Vec<u8>)],
) -> Result<(u16, Vec<u8>), String> {
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return Err("there's no line number".into());
    }
    let number = line[..digits]
        .parse()
        .ok()
        .filter(|&n| n <= MAX_LINE_NUMBER)
        .ok_or_else(|| format!("line numbers go up to {MAX_LINE_NUMBER}"))?;
    // The machine's editor drops spaces before a line's first word too.
    let rest = line[digits..].trim_start_matches([' ', '\t', '\u{A0}']);
    let body = crunch(&characters(rest, shifted)?, words);
    if body.contains(&0) {
        return Err("a line can't hold a {$00}: it marks the line's end".into());
    }
    if body.len() > MAX_LINE_BYTES {
        return Err(format!(
            "the line is {} bytes long (at most {MAX_LINE_BYTES} fit)",
            body.len()
        ));
    }
    Ok((number, body))
}

/// The text as PETSCII.
fn characters(text: &str, shifted: Shifted) -> Result<Vec<Ch>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '{' {
            let end = rest.find('}').ok_or("a { has no } after it on the line")?;
            out.extend(braces(&rest[1..end])?.into_iter().map(Ch::Byte));
            rest = &rest[end + 1..];
            continue;
        }
        rest = &rest[c.len_utf8()..];
        let b = match c {
            'A'..='Z' if shifted == Shifted::Upper => c as u8 + 0x80,
            'a'..='z' if shifted == Shifted::Upper => c as u8 - 0x20,
            'a'..='z' => c as u8 + 0x60,
            ' '..='_' => c as u8,
            '\t' => b' ',
            '\u{A0}' => 0xA0,
            '£' => 0x5C,
            '↑' => 0x5E,
            '←' => 0x5F,
            'π' | '~' => 0xFF,
            // What word processors make of quotes.
            '“' | '”' => b'"',
            '‘' | '’' => b'\'',
            _ => {
                return Err(format!(
                    "\"{}\" isn't a PETSCII character (write it as {{$xx}})",
                    c.escape_debug()
                ))
            }
        };
        out.push(Ch::Typed(b));
    }
    Ok(out)
}

/// What the inside of a pair of braces stands for.
fn braces(inside: &str) -> Result<Vec<u8>, String> {
    let bad = || format!("{{{inside}}} isn't a known code");
    let s = inside.trim().to_ascii_uppercase();
    if let Some(hex) = s.strip_prefix('$') {
        // One byte, or a two-byte token as listings write it.
        if !matches!(hex.len(), 2 | 4) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad());
        }
        return (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad()))
            .collect();
    }
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(vec![s.parse().map_err(|_| bad())?]);
    }
    // "5 DOWN" or "DOWN*5".
    let digits = s.len() - s.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let (count, name) = if digits > 0 {
        (Some(&s[..digits]), s[digits..].trim())
    } else {
        match s.rsplit_once('*') {
            Some((name, n)) if !n.trim().is_empty() => (Some(n.trim()), name.trim()),
            _ => (None, s.as_str()),
        }
    };
    let count = match count {
        Some(n) => n
            .parse()
            .ok()
            .filter(|n| (1..=MAX_LINE_BYTES).contains(n))
            .ok_or_else(bad)?,
        None => 1,
    };
    let byte = key(name).or_else(|| named(name)).ok_or_else(bad)?;
    Ok(vec![byte; count])
}

/// A key pressed with a modifier: `SHIFT-A`, `CBM-+`, `CTRL-C`.
fn key(name: &str) -> Option<u8> {
    let (modifier, k) = ["SHIFT", "SHFT", "CBM", "COMMODORE", "C=", "CTRL"]
        .iter()
        .find_map(|m| Some((*m, name.strip_prefix(m)?.strip_prefix(['-', ' '])?)))?;
    let k = match k {
        "POUND" => b'\\',
        "SPACE" => b' ',
        _ if k.len() == 1 => k.as_bytes()[0],
        _ => return None,
    };
    Some(match (modifier, k) {
        ("SHIFT" | "SHFT", b'A'..=b'Z') => k + 0x80,
        ("SHIFT" | "SHFT", _) => match k {
            b'*' => 0xC0,
            b'+' => 0xDB,
            b'-' => 0xDD,
            b'@' => 0xBA,
            b'^' => 0xDE,
            b'\\' => 0xA9,
            b' ' => 0xA0,
            _ => return None,
        },
        ("CTRL", b'A'..=b'Z') => k - 0x40,
        ("CTRL", _) => return None,
        // The Commodore key's graphics, A to Z.
        (_, b'A'..=b'Z') => {
            const CBM: [u8; 26] = [
                0xB0, 0xBF, 0xBC, 0xAC, 0xB1, 0xBB, 0xA5, 0xB4, 0xA2, 0xB5, 0xA1, 0xB6, 0xA7, 0xAA,
                0xB9, 0xAF, 0xAB, 0xB2, 0xAE, 0xA3, 0xB8, 0xBE, 0xB3, 0xBD, 0xB7, 0xAD,
            ];
            CBM[usize::from(k - b'A')]
        }
        (_, b'+') => 0xA6,
        (_, b'-') => 0xDC,
        (_, b'*') => 0xDF,
        (_, b'@') => 0xA4,
        (_, b'^') => 0xDE,
        (_, b'\\') => 0xA8,
        (_, b' ') => 0xA0,
        _ => return None,
    })
}

/// A control code's byte, by the names listings use (this crate's, petcat's
/// and the magazines').
fn named(name: &str) -> Option<u8> {
    let plain: String = name
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_' | '.'))
        .collect();
    Some(match plain.as_str() {
        "STOP" => 0x03,
        "WHT" | "WHITE" => 0x05,
        "DISH" => 0x08,
        "ENSH" => 0x09,
        "RETURN" | "RET" => 0x0D,
        "SWLC" | "LOWER" => 0x0E,
        "DOWN" => 0x11,
        "RVSON" | "RVON" | "RVS" | "REVERSEON" => 0x12,
        "HOME" | "HOM" => 0x13,
        "DEL" => 0x14,
        "ESC" => 0x1B,
        "RED" => 0x1C,
        "RIGHT" | "RGHT" | "RIGT" => 0x1D,
        "GRN" | "GREEN" => 0x1E,
        "BLU" | "BLUE" => 0x1F,
        "SPACE" | "SPC" => 0x20,
        "POUND" => 0x5C,
        "UPARROW" => 0x5E,
        "LEFTARROW" | "ARROWLEFT" => 0x5F,
        "ORNG" | "ORANGE" => 0x81,
        "F1" => 0x85,
        "F3" => 0x86,
        "F5" => 0x87,
        "F7" => 0x88,
        "F2" => 0x89,
        "F4" => 0x8A,
        "F6" => 0x8B,
        "F8" => 0x8C,
        "SHIFTRETURN" | "SRET" => 0x8D,
        "SWUC" | "UPPER" => 0x8E,
        "BLK" | "BLACK" => 0x90,
        "UP" => 0x91,
        "RVSOFF" | "RVOF" | "OFF" | "REVERSEOFF" => 0x92,
        "CLR" | "CLEAR" => 0x93,
        "INST" | "INS" => 0x94,
        "BRN" | "BROWN" => 0x95,
        "LRED" | "PINK" | "LTRED" | "LIGHTRED" => 0x96,
        "GRY1" | "GRAY1" | "GREY1" | "DARKGRAY" | "DARKGREY" => 0x97,
        "GRY2" | "GRAY2" | "GREY2" | "GRAY" | "GREY" => 0x98,
        "LGRN" | "LTGRN" | "LIGHTGREEN" => 0x99,
        "LBLU" | "LTBLU" | "LIGHTBLUE" => 0x9A,
        "GRY3" | "GRAY3" | "GREY3" | "LIGHTGRAY" | "LIGHTGREY" => 0x9B,
        "PUR" | "PURPLE" => 0x9C,
        "LEFT" => 0x9D,
        "YEL" | "YELLOW" => 0x9E,
        "CYN" | "CYAN" => 0x9F,
        "SHIFTSPACE" => 0xA0,
        "PI" => 0xFF,
        _ => return None,
    })
}

/// Every keyword and its token, in the order the machine tries them. The
/// added keywords come before V2's: BASIC 7.0 looks through its two-byte
/// tables first, and a cartridge gets the line before the ROM does.
fn keywords(dialect: Dialect) -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&str, Vec<u8>)> = Vec::new();
    match dialect {
        Dialect::V2 => {}
        Dialect::V7 => {
            let two = |p: u8, t: &'static [&'static str]| {
                t.iter()
                    .enumerate()
                    .map(move |(i, &k)| (k, vec![p, i as u8 + 2]))
            };
            out.extend(two(0xFE, &V7_FE));
            out.extend(two(0xCE, &V7_CE));
        }
        Dialect::Simons => out.extend(
            SIMONS
                .iter()
                .enumerate()
                .map(|(i, &k)| (k, vec![0x64, i as u8 + 1])),
        ),
    }
    let one = |first: u8, t: &'static [&'static str]| {
        t.iter()
            .enumerate()
            .map(move |(i, &k)| (k, vec![first + i as u8]))
    };
    out.extend(one(0x80, &KEYWORDS));
    if dialect == Dialect::V7 {
        out.extend(one(0xCC, &V7_KEYWORDS));
    }
    out.retain(|(k, _)| !k.is_empty());
    out
}

/// Replace keywords by their tokens, the way the machine's editor does.
fn crunch(line: &[Ch], words: &[(&str, Vec<u8>)]) -> Vec<u8> {
    const DATA: u8 = 0x83;
    const REM: u8 = 0x8F;
    let mut out = Vec::new();
    let (mut quoted, mut data, mut rem) = (false, false, false);
    let mut i = 0;
    while let Some(&ch) = line.get(i) {
        i += 1;
        let Ch::Typed(b) = ch else {
            out.push(ch.byte());
            continue;
        };
        if rem {
            out.push(b);
            continue;
        }
        if b == b'"' {
            quoted = !quoted;
        }
        if quoted || b == b'"' {
            out.push(b);
            continue;
        }
        if b == b':' {
            data = false;
        }
        if data || matches!(b, b' ' | b'0'..=b';') {
            out.push(b);
            continue;
        }
        if b == b'?' {
            out.push(0x99); // PRINT
            continue;
        }
        if let Some((token, len)) = keyword_at(&line[i - 1..], words) {
            out.extend_from_slice(token);
            data = token == [DATA];
            rem = token == [REM];
            i += len - 1;
            continue;
        }
        // A shifted letter that isn't part of an abbreviation means nothing
        // in a program (the machine drops it); it's a capital in a listing
        // with mixed case.
        out.push(unshift(b));
    }
    out
}

/// A shifted letter's or space's unshifted one.
fn unshift(b: u8) -> u8 {
    match b {
        0xA0 | 0xC1..=0xDA => b - 0x80,
        _ => b,
    }
}

/// The keyword at the start of `text`, if there is one: its token and how
/// many characters it takes up.
fn keyword_at<'a>(text: &[Ch], words: &'a [(&str, Vec<u8>)]) -> Option<(&'a [u8], usize)> {
    let typed = |n: usize| match text.get(n) {
        Some(&Ch::Typed(b)) => Some(b),
        _ => None,
    };
    let spelled = |k: &str| {
        k.bytes()
            .enumerate()
            .all(|(n, want)| typed(n).map(unshift) == Some(want))
    };
    // An abbreviation's length: some of a keyword's letters, the last of them
    // shifted.
    let abbreviated = |k: &str| {
        let k = k.as_bytes();
        let same = (0..k.len()).take_while(|&n| typed(n) == Some(k[n])).count();
        let shifted = k.get(same).filter(|c| c.is_ascii_uppercase())?;
        (same > 0 && typed(same) == Some(shifted + 0x80)).then_some(same + 1)
    };
    let full = words
        .iter()
        .find(|(k, _)| spelled(k))
        .map(|(k, t)| (&t[..], k.len()));
    let short = words
        .iter()
        .find_map(|(k, t)| Some((&t[..], abbreviated(k)?)));
    match (full, short) {
        // "gO" is short for GOTO, not GO; but "pRint" is just PRINT.
        (Some(f), Some(s)) if s.1 >= f.1 => Some(s),
        (Some(f), _) => Some(f),
        (None, s) => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basic::{detokenize, detokenize_as};

    /// The tokenized body of a one-line program.
    fn body(line: &str, dialect: Dialect) -> Vec<u8> {
        let t = tokenize(&format!("10 {line}\n"), dialect, None).unwrap();
        t.prg[6..t.prg.len() - 3].to_vec()
    }

    fn v2(line: &str) -> Vec<u8> {
        body(line, Dialect::V2)
    }

    fn fails(text: &str) -> String {
        tokenize(text, Dialect::V2, None).unwrap_err()[0].to_string()
    }

    #[test]
    fn makes_a_prg() {
        let t = tokenize(
            "10 PRINT \"HELLO\"\r\n\r\n20 GOTO 10\r\n",
            Dialect::V2,
            None,
        )
        .unwrap();
        assert_eq!(
            t.prg,
            b"\x01\x08\x0f\x08\x0a\x00\x99 \"HELLO\"\x00\x18\x08\x14\x00\x89 10\x00\x00\x00"
        );
        assert_eq!((t.lines, t.load_address), (2, 0x0801));
        assert!(t.warnings.is_empty());
    }

    #[test]
    fn keywords_need_no_spaces_but_not_in_quotes_rem_or_data() {
        assert_eq!(v2("FORI=ATOB"), b"\x81I\xb2A\xa4B");
        assert_eq!(v2("PRINT\"PRINT\":PRINT"), b"\x99\"PRINT\":\x99");
        assert_eq!(v2("REM PRINT \"A\":END"), b"\x8f PRINT \"A\":END");
        assert_eq!(v2("DATA END,\"A:END\":END"), b"\x83 END,\"A:END\":\x80");
        assert_eq!(v2("INPUT#1,A:PRINT#1:GOTO5"), b"\x84\x31,A:\x98\x31:\x895");
        // An open quote runs to the end of the line.
        assert_eq!(v2("PRINT\"END"), b"\x99\"END");
    }

    #[test]
    fn abbreviations_and_mixed_case() {
        assert_eq!(v2("?A"), b"\x99A");
        // As listed here, a shifted letter is lower case; petcat, upper.
        for line in ["Po1,2:Go10:GOs20:Ne:Pe(1)", "pO1,2:gO10:goS20:nE:pE(1)"] {
            assert_eq!(v2(line), b"\x971,2:\x8910:\x8d20:\x82:\xc2(1)", "{line}");
        }
        assert_eq!(v2("print \"hi\":Goto 10"), b"\x99 \"HI\":\x89 10");
        assert_eq!(v2("print \"Hi\""), b"\x99 \"\xc8I\"");
        assert_eq!(v2("PRINT \"Hi\""), b"\x99 \"H\xc9\"");
    }

    #[test]
    fn braces_name_bytes() {
        assert_eq!(
            v2("PRINT\"{CLR}{5 DOWN}{down*3}{RVS ON}{rvof}{$A0}{147}{SHIFT-A}{CBM-+}{shift--}\""),
            b"\x99\"\x93\x11\x11\x11\x11\x11\x11\x11\x11\x12\x92\xa0\x93\xc1\xa6\xdd\""
        );
        assert_eq!(
            v2("{$DE}:{$FE20}:A={PI}+π"),
            b"\xde:\xfe\x20:A\xb2\xff\xaa\xff"
        );
        // A byte given by number is never part of a keyword.
        assert_eq!(v2("{$45}ND"), b"END");
        for bad in ["{NOPE}", "{$1}", "{300}", "{0 UP}", "{}", "{DOWN", "{$00}"] {
            assert!(
                fails(&format!("10 {bad}")).starts_with("text line 1:"),
                "{bad}"
            );
        }
    }

    #[test]
    fn problems_name_the_text_line() {
        assert_eq!(
            fails("10 PRINT\n\nPRINT\n"),
            "text line 3: there's no line number"
        );
        assert!(fails("64000 END").contains("63999"));
        assert!(fails("10 PRINT \"€\"").contains("PETSCII"));
        assert!(fails(&format!("10 {}", "A".repeat(256))).contains("256 bytes"));
        assert!(fails(" \n;heading\n").contains("no program"));
        let all = tokenize("A\n10 {X}\n", Dialect::V2, None).unwrap_err();
        assert_eq!(all.iter().map(|p| p.line).collect::<Vec<_>>(), [1, 2]);
        let t = tokenize("20 END\n10 END\n", Dialect::V2, None).unwrap();
        assert_eq!(t.warnings[0].to_string(), "text line 2: 10 comes after 20");
        let big: String = (0..300)
            .map(|i| format!("{i} {}\n", "A".repeat(250)))
            .collect();
        assert!(fails(&big).contains("memory"));
    }

    #[test]
    fn load_address_is_given_headed_or_usual() {
        let load = |text, d, l| tokenize(text, d, l).unwrap().load_address;
        assert_eq!(load("10 END", Dialect::V7, None), 0x1C01);
        assert_eq!(load("10 END", Dialect::V2, Some(0x1001)), 0x1001);
        let headed = "\n;vic.prg ==1201==\n10 end\n";
        assert_eq!(load(headed, Dialect::V2, None), 0x1201);
        assert_eq!(load(headed, Dialect::V2, Some(0x0801)), 0x0801);
    }

    #[test]
    fn dialects() {
        let v7 = |l| body(l, Dialect::V7);
        assert_eq!(v7("DOPEN#1:DO:LOOP"), b"\xfe\x0d#1:\xeb:\xec");
        assert_eq!(
            v7("X=POINTER(A):END:BEND"),
            b"X\xb2\xce\x0a(A):\x80:\xfe\x19"
        );
        assert_eq!(v7("GETKEY A$:ELSEIF"), b"\xa1\xf9 A$:\xd5\x8b");
        assert_eq!(v2("DRAW"), b"DRAW");
        let simons = |l| body(l, Dialect::Simons);
        assert_eq!(simons("HI COL1:END PROC:END"), b"\x64\x0c1:\x64\x34:\x80");
        assert_eq!(simons("ON KEY:ON A GOTO1"), b"\x64\x37:\x91 A \x891");
        assert_eq!(simons("LINE:LIN"), b"\x64\x03:\x64\x59");
    }

    #[test]
    fn every_keyword_lists_and_tokenizes_back() {
        for d in [Dialect::V2, Dialect::V7, Dialect::Simons] {
            for (k, token) in keywords(d) {
                assert_eq!(body(k, d), token, "{k}");
                assert_eq!(body(&k.to_lowercase(), d), token, "{k}");
            }
        }
    }

    #[test]
    fn every_byte_in_quotes_survives_a_round_trip() {
        for b in 1..=255u8 {
            if b == b'"' {
                continue; // it ends the quotes
            }
            let prg = crate::basic::tests::prg(0x0801, &[(10, &[0x99, b'"', b, b, b'!', b'"'])]);
            let l = detokenize(&prg).unwrap();
            let t = tokenize(&l.text, l.dialect, Some(l.load_address)).unwrap();
            assert_eq!(t.prg, prg, "{b:02X}: {}", l.text);
        }
        let prg = crate::basic::tests::prg(0x1C01, &[(1, b"\xfe\x20:\xce\x7f:\xff")]);
        let l = detokenize_as(&prg, Dialect::V7).unwrap();
        assert_eq!(tokenize(&l.text, Dialect::V7, None).unwrap().prg, prg);
    }
}
