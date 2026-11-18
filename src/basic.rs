//! Turn a tokenized Commodore BASIC program (a PRG file) back into a readable
//! listing, the way the C64's `LIST` command shows it.
//!
//! Written from the documented BASIC V2 program format, not from any existing
//! tool's code. A PRG starts with a two-byte load address; each program line
//! is then a two-byte link to the next line, a two-byte line number, the
//! tokenized text and a `$00` terminator. A link of `$0000` ends the program.
//! Keywords are stored as single bytes `$80`–`$CB`; inside quotes bytes are
//! plain PETSCII, so control codes there are shown as `{CLR}`-style names, as
//! printed magazine listings did.
//!
//! Like `LIST`, the text after `REM` is expanded too (only quotes switch
//! tokenizing off). Every read is bounds-checked; malformed input gives
//! `None`, never a panic.

/// BASIC V2 keywords for tokens `$80`–`$CB`, in token order.
const KEYWORDS: [&str; 76] = [
    "END", "FOR", "NEXT", "DATA", "INPUT#", "INPUT", "DIM", "READ", // $80
    "LET", "GOTO", "RUN", "IF", "RESTORE", "GOSUB", "RETURN", "REM", // $88
    "STOP", "ON", "WAIT", "LOAD", "SAVE", "VERIFY", "DEF", "POKE", // $90
    "PRINT#", "PRINT", "CONT", "LIST", "CLR", "CMD", "SYS", "OPEN", // $98
    "CLOSE", "GET", "NEW", "TAB(", "TO", "FN", "SPC(", "THEN", // $A0
    "NOT", "STEP", "+", "-", "*", "/", "^", "AND", // $A8
    "OR", ">", "=", "<", "SGN", "INT", "ABS", "USR", // $B0
    "FRE", "POS", "SQR", "RND", "LOG", "EXP", "COS", "SIN", // $B8
    "TAN", "ATN", "PEEK", "LEN", "STR$", "VAL", "ASC", "CHR$", // $C0
    "LEFT$", "RIGHT$", "MID$", "GO", // $C8
];

/// Load addresses where BASIC programs start on Commodore machines: PET and
/// VIC-20 +3K ($0401), C64 ($0801), unexpanded VIC-20 and Plus/4 ($1001),
/// VIC-20 +8K ($1201), C128 ($1C01) and C128 with a graphics screen ($4001).
const BASIC_STARTS: [u16; 6] = [0x0401, 0x0801, 0x1001, 0x1201, 0x1C01, 0x4001];

/// Highest line number BASIC accepts.
const MAX_LINE_NUMBER: u16 = 63999;

/// Longest tokenized line accepted (BASIC's own limit is 255 bytes).
const MAX_LINE_BYTES: usize = 255;

/// A detokenized BASIC program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicListing {
    /// The PRG's load address (`$0801` on a C64).
    pub load_address: u16,
    /// Number of program lines.
    pub lines: usize,
    /// The listing: one `number text` line per program line, each ending in
    /// `\n`.
    pub text: String,
    /// Bytes after the end-of-program marker (often machine code started by a
    /// `SYS` line).
    pub trailing_bytes: usize,
}

/// Detokenize `prg` (a PRG file including its load address). Returns `None`
/// when it isn't a well-formed BASIC program.
pub fn detokenize(prg: &[u8]) -> Option<BasicListing> {
    let word =
        |i: usize| -> Option<u16> { Some(u16::from_le_bytes([*prg.get(i)?, *prg.get(i + 1)?])) };
    let load = word(0)?;
    let mut pos = 2;
    let mut text = String::new();
    let mut lines = 0usize;
    let mut links_ok = true;
    loop {
        let link = match word(pos) {
            Some(l) => l,
            // Some files stop without the final $0000; accept that once at
            // least one line has been read.
            None if lines > 0 && pos >= prg.len() => break,
            None => return None,
        };
        if link == 0 {
            pos += 2;
            break;
        }
        let number = word(pos + 2)?;
        if number > MAX_LINE_NUMBER {
            return None;
        }
        let body_start = pos + 4;
        let len = prg
            .get(body_start..)?
            .iter()
            .take(MAX_LINE_BYTES + 1)
            .position(|&b| b == 0)?;
        let body = &prg[body_start..body_start + len];
        let next = body_start + len + 1;
        // The link should be the next line's address; files moved between
        // machines can be off (LOAD relinks them), so it only counts as a
        // hint below.
        if usize::from(link) != usize::from(load) + next - 2 {
            links_ok = false;
        }
        text.push_str(&number.to_string());
        text.push(' ');
        list_line(body, &mut text);
        text.push('\n');
        lines += 1;
        pos = next;
    }
    // Plenty of non-BASIC data happens to parse for a line or two; insist on
    // either a correct link chain or a usual BASIC start address.
    if lines == 0 || !(links_ok || BASIC_STARTS.contains(&load)) {
        return None;
    }
    Some(BasicListing {
        load_address: load,
        lines,
        text,
        trailing_bytes: prg.len().saturating_sub(pos),
    })
}

/// Append one line's tokenized body, expanded, to `out`.
fn list_line(body: &[u8], out: &mut String) {
    let mut quoted = false;
    let mut i = 0;
    while i < body.len() {
        let b = body[i];
        if b == b'"' {
            quoted = !quoted;
            out.push('"');
            i += 1;
            continue;
        }
        if quoted {
            // Runs of the same control code collapse to "{5 DOWN}".
            if let Some(name) = control_name(b) {
                let run = body[i..].iter().take_while(|&&c| c == b).count();
                if run > 1 {
                    out.push_str(&format!("{{{run} {name}}}"));
                } else {
                    out.push_str(&format!("{{{name}}}"));
                }
                i += run;
                continue;
            }
            push_char(b, out);
        } else {
            match b {
                0x80..=0xCB => out.push_str(KEYWORDS[usize::from(b - 0x80)]),
                0xFF => out.push('π'),
                // Tokens BASIC V2 doesn't have (BASIC 7.0 and extensions
                // such as Simons' BASIC use these).
                0xCC..=0xFE => out.push_str(&format!("{{${b:02X}}}")),
                _ => push_char(b, out),
            }
        }
        i += 1;
    }
}

/// A printable PETSCII byte as text, or `{$xx}` for anything else.
fn push_char(b: u8, out: &mut String) {
    match b {
        0x20..=0x5F => out.push(crate::petscii::char_of(b)),
        0xC1..=0xDA => out.push(crate::petscii::char_of(b)),
        0xA0 => out.push(' '),
        _ => out.push_str(&format!("{{${b:02X}}}")),
    }
}

/// The usual listing name for a PETSCII control code.
fn control_name(b: u8) -> Option<&'static str> {
    Some(match b {
        0x05 => "WHT",
        0x08 => "DISH",
        0x09 => "ENSH",
        0x0D => "RETURN",
        0x0E => "SWLC",
        0x11 => "DOWN",
        0x12 => "RVS ON",
        0x13 => "HOME",
        0x14 => "DEL",
        0x1C => "RED",
        0x1D => "RIGHT",
        0x1E => "GRN",
        0x1F => "BLU",
        0x81 => "ORNG",
        0x85 => "F1",
        0x86 => "F3",
        0x87 => "F5",
        0x88 => "F7",
        0x89 => "F2",
        0x8A => "F4",
        0x8B => "F6",
        0x8C => "F8",
        0x8D => "SHIFT RETURN",
        0x8E => "SWUC",
        0x90 => "BLK",
        0x91 => "UP",
        0x92 => "RVS OFF",
        0x93 => "CLR",
        0x94 => "INST",
        0x95 => "BRN",
        0x96 => "LRED",
        0x97 => "GRY1",
        0x98 => "GRY2",
        0x99 => "LGRN",
        0x9A => "LBLU",
        0x9B => "GRY3",
        0x9C => "PUR",
        0x9D => "LEFT",
        0x9E => "YEL",
        0x9F => "CYN",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a PRG at `load` from `(line number, tokenized body)` pairs, with
    /// correct links and the `$0000` end marker.
    pub(crate) fn prg(load: u16, lines: &[(u16, &[u8])]) -> Vec<u8> {
        let mut out = load.to_le_bytes().to_vec();
        for (n, body) in lines {
            let next = load as usize + (out.len() - 2) + 4 + body.len() + 1;
            out.extend_from_slice(&(next as u16).to_le_bytes());
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(body);
            out.push(0);
        }
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn keyword_table_covers_80_to_cb() {
        assert_eq!(KEYWORDS.len(), 0xCB - 0x80 + 1);
        assert_eq!(KEYWORDS[0x99 - 0x80], "PRINT");
        assert_eq!(KEYWORDS[0x9E - 0x80], "SYS");
        assert_eq!(KEYWORDS[0xC7 - 0x80], "CHR$");
    }

    #[test]
    fn lists_a_simple_program() {
        // 10 PRINT "HELLO"
        // 20 GOTO 10
        let p = prg(0x0801, &[(10, b"\x99 \"HELLO\""), (20, b"\x89 10")]);
        let l = detokenize(&p).unwrap();
        assert_eq!(l.text, "10 PRINT \"HELLO\"\n20 GOTO 10\n");
        assert_eq!((l.load_address, l.lines, l.trailing_bytes), (0x0801, 2, 0));
    }

    #[test]
    fn quotes_switch_off_tokens_and_name_control_codes() {
        // 10 PRINT "{CLR}{5 DOWN}"; CHR$(147)  — $99 inside quotes is a
        // control code (LGRN), not PRINT.
        let p = prg(
            0x0801,
            &[(10, b"\x99\"\x93\x11\x11\x11\x11\x11\x99\";\xC7(147)")],
        );
        assert_eq!(
            detokenize(&p).unwrap().text,
            "10 PRINT\"{CLR}{5 DOWN}{LGRN}\";CHR$(147)\n"
        );
    }

    #[test]
    fn rem_text_is_expanded_like_list() {
        // C64 LIST expands tokens after REM too (only quotes stop it).
        let p = prg(0x0801, &[(1, b"\x8f \x99")]);
        assert_eq!(detokenize(&p).unwrap().text, "1 REM PRINT\n");
    }

    #[test]
    fn counts_machine_code_after_a_sys_stub() {
        // 10 SYS2061, then 3 bytes of 6502 code.
        let mut p = prg(0x0801, &[(10, b"\x9e2061")]);
        p.extend_from_slice(&[0xA9, 0x00, 0x60]);
        let l = detokenize(&p).unwrap();
        assert_eq!(l.text, "10 SYS2061\n");
        assert_eq!(l.trailing_bytes, 3);
    }

    #[test]
    fn pi_and_unknown_bytes() {
        let p = prg(0x0801, &[(5, b"A=\xff:B=\xcc")]);
        assert_eq!(detokenize(&p).unwrap().text, "5 A=π:B={$CC}\n");
    }

    #[test]
    fn rejects_non_basic() {
        assert_eq!(detokenize(&[]), None);
        assert_eq!(detokenize(&[0x01, 0x08]), None);
        // Machine code at $C000.
        assert_eq!(
            detokenize(&[0x00, 0xC0, 0xA9, 0x00, 0x8D, 0x20, 0xD0, 0x60]),
            None
        );
        // Line number above 63999.
        let mut p = prg(0x0801, &[(10, b"\x80")]);
        p[4..6].copy_from_slice(&64000u16.to_le_bytes());
        assert_eq!(detokenize(&p), None);
        // Unterminated line.
        assert_eq!(
            detokenize(&[0x01, 0x08, 0x10, 0x08, 0x0A, 0x00, 0x99]),
            None
        );
    }

    #[test]
    fn odd_load_address_needs_a_correct_link_chain() {
        let good = prg(0x3000, &[(10, b"\x80")]);
        assert!(detokenize(&good).is_some());
        let mut bad = good.clone();
        bad[2] ^= 0x40; // break the link
        assert_eq!(detokenize(&bad), None);
        // At $0801 a wrong link is tolerated (LOAD would relink it).
        let mut moved = prg(0x0801, &[(10, b"\x80")]);
        moved[2] ^= 0x40;
        assert!(detokenize(&moved).is_some());
    }

    #[test]
    fn missing_end_marker_is_tolerated() {
        let mut p = prg(0x0801, &[(10, b"\x80")]);
        p.truncate(p.len() - 2);
        assert_eq!(detokenize(&p).unwrap().text, "10 END\n");
    }
}
