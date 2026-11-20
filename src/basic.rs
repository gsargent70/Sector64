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
//!
//! Two other dialects are understood (see [`Dialect`]): the C128's BASIC 7.0,
//! which adds single-byte tokens `$CC`–`$FD` and two-byte ones behind the
//! prefixes `$CE` and `$FE`, and Simons' BASIC for the C64, whose keywords are
//! two bytes, `$64` followed by `$01`–`$7F`. Their keyword tables were checked
//! token by token against VICE's `petcat` tokenizer.

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

/// BASIC 7.0's single-byte tokens `$CC`–`$FD` (`$CE` is a prefix, below).
#[rustfmt::skip]
const V7_KEYWORDS: [&str; 50] = [
    "RGR", "RCLR", "", "JOY", "RDOT", "DEC", "HEX$", "ERR$", // $CC
    "INSTR", "ELSE", "RESUME", "TRAP", "TRON", "TROFF", "SOUND", "VOL", // $D4
    "AUTO", "PUDEF", "GRAPHIC", "PAINT", "CHAR", "BOX", "CIRCLE", "GSHAPE", // $DC
    "SSHAPE", "DRAW", "LOCATE", "COLOR", "SCNCLR", "SCALE", "HELP", "DO", // $E4
    "LOOP", "EXIT", "DIRECTORY", "DSAVE", "DLOAD", "HEADER", "SCRATCH", "COLLECT", // $EC
    "COPY", "RENAME", "BACKUP", "DELETE", "RENUMBER", "KEY", "MONITOR", "USING", // $F4
    "UNTIL", "WHILE", // $FC
];

/// BASIC 7.0 functions behind the `$CE` prefix, from `$CE $02`.
const V7_CE: [&str; 9] = [
    "POT", "BUMP", "PEN", "RSPPOS", "RSPRITE", "RSPCOLOR", "XOR", "RWINDOW", "POINTER",
];

/// BASIC 7.0 statements behind the `$FE` prefix, from `$FE $02`; `$FE $20`
/// and `$FE $22` are unused.
#[rustfmt::skip]
const V7_FE: [&str; 37] = [
    "BANK",
    "FILTER",
    "PLAY",
    "TEMPO",
    "MOVSPR",
    "SPRITE",
    "SPRCOLOR",
    "RREG", // $02
    "ENVELOPE",
    "SLEEP",
    "CATALOG",
    "DOPEN",
    "APPEND",
    "DCLOSE",
    "BSAVE",
    "BLOAD", // $0A
    "RECORD",
    "CONCAT",
    "DVERIFY",
    "DCLEAR",
    "SPRSAV",
    "COLLISION",
    "BEGIN",
    "BEND", // $12
    "WINDOW",
    "BOOT",
    "WIDTH",
    "SPRDEF",
    "QUIT",
    "STASH",
    "",
    "FETCH", // $1A
    "",
    "SWAP",
    "OFF",
    "FAST",
    "SLOW", // $22
];

/// Simons' BASIC keywords `$64 $01`–`$64 $7F`; empty slots are unassigned
/// and listed as `{$64xx}`.
#[rustfmt::skip]
const SIMONS: [&str; 127] = [
    "HIRES", "PLOT", "LINE", "BLOCK", "FCHR", "FCOL", "FILL", // $01
    "REC", "ROT", "DRAW", "CHAR", "HI COL", "INV", "FRAC", "MOVE", // $08
    "PLACE", "UPB", "UPW", "LEFTW", "LEFTB", "DOWNB", "DOWNW", "RIGHTB", // $10
    "RIGHTW", "MULTI", "COLOUR", "MMOB", "BFLASH", "MOB SET", "MUSIC", "FLASH", // $18
    "REPEAT", "PLAY", "", "CENTRE", "ENVELOPE", "CGOTO", "WAVE", "FETCH", // $20
    "AT(", "UNTIL", "", "", "USE", "", "GLOBAL", "", // $28
    "RESET", "PROC", "CALL", "EXEC", "END PROC", "EXIT", "END LOOP", "ON KEY", // $30
    "DISABLE", "RESUME", "LOOP", "DELAY", "", "", "", "", // $38
    "SECURE", "DISAPA", "CIRCLE", "ON ERROR", "NO ERROR", "LOCAL", "RCOMP", "ELSE", // $40
    "RETRACE", "TRACE", "DIR", "PAGE", "DUMP", "FIND", "OPTION", "AUTO", // $48
    "OLD", "JOY", "MOD", "DIV", "", "DUP", "INKEY", "INST", // $50
    "TEST", "LIN", "EXOR", "INSERT", "POT", "PENX", "", "PENY", // $58
    "SOUND", "GRAPHICS", "DESIGN", "RLOCMOB", "CMOB", "BCKGNDS", "PAUSE", "NRM", // $60
    "MOB OFF", "OFF", "ANGL", "ARC", "COLD", "SCRSV", "SCRLD", "TEXT", // $68
    "CSET", "VOL", "DISK", "HRDCPY", "KEY", "PAINT", "LOW COL", "COPY", // $70
    "MERGE", "RENUMBER", "MEM", "DETECT", "CHECK", "DISPLAY", "ERR", "OUT", // $78
];

/// Which BASIC a program is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Commodore BASIC V2 (C64, VIC-20; also what PET BASIC 2 and 4 share).
    V2,
    /// The C128's BASIC 7.0 (a superset of V2).
    V7,
    /// Simons' BASIC, the C64 cartridge (V2 plus `$64`-prefixed keywords).
    Simons,
}

impl Dialect {
    /// The dialect's usual name.
    pub fn name(self) -> &'static str {
        match self {
            Dialect::V2 => "BASIC V2",
            Dialect::V7 => "BASIC 7.0",
            Dialect::Simons => "Simons' BASIC",
        }
    }
}

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
    /// The dialect the listing was expanded as.
    pub dialect: Dialect,
}

/// Detokenize `prg` (a PRG file including its load address), guessing its
/// dialect: BASIC 7.0 when it loads where a C128 puts BASIC ($1C01, or $4001
/// with a graphics screen), Simons' BASIC when it uses Simons' `$64` tokens
/// (which typed V2 text can't contain), otherwise V2. Returns `None` when it
/// isn't a well-formed BASIC program.
pub fn detokenize(prg: &[u8]) -> Option<BasicListing> {
    let program = parse(prg)?;
    let dialect = if matches!(program.load, 0x1C01 | 0x4001) {
        Dialect::V7
    } else if program
        .lines
        .iter()
        .any(|(_, body)| has_simons_tokens(body))
    {
        Dialect::Simons
    } else {
        Dialect::V2
    };
    Some(program.list(dialect))
}

/// Detokenize `prg` as a given dialect.
pub fn detokenize_as(prg: &[u8], dialect: Dialect) -> Option<BasicListing> {
    Some(parse(prg)?.list(dialect))
}

/// A program split into lines, not yet expanded.
struct Program<'a> {
    load: u16,
    lines: Vec<(u16, &'a [u8])>,
    trailing_bytes: usize,
}

impl Program<'_> {
    fn list(&self, dialect: Dialect) -> BasicListing {
        let mut text = String::new();
        for (number, body) in &self.lines {
            text.push_str(&number.to_string());
            text.push(' ');
            list_line(body, dialect, &mut text);
            text.push('\n');
        }
        BasicListing {
            load_address: self.load,
            lines: self.lines.len(),
            text,
            trailing_bytes: self.trailing_bytes,
            dialect,
        }
    }
}

/// Split `prg` into its lines, or `None` when it isn't BASIC.
fn parse(prg: &[u8]) -> Option<Program<'_>> {
    let word =
        |i: usize| -> Option<u16> { Some(u16::from_le_bytes([*prg.get(i)?, *prg.get(i + 1)?])) };
    let load = word(0)?;
    let mut pos = 2;
    let mut lines = Vec::new();
    let mut links_ok = true;
    loop {
        let link = match word(pos) {
            Some(l) => l,
            // Some files stop without the final $0000; accept that once at
            // least one line has been read.
            None if !lines.is_empty() && pos >= prg.len() => break,
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
        let next = body_start + len + 1;
        // The link should be the next line's address; files moved between
        // machines can be off (LOAD relinks them), so it only counts as a
        // hint below.
        if usize::from(link) != usize::from(load) + next - 2 {
            links_ok = false;
        }
        lines.push((number, &prg[body_start..body_start + len]));
        pos = next;
    }
    // Plenty of non-BASIC data happens to parse for a line or two; insist on
    // either a correct link chain or a usual BASIC start address.
    if lines.is_empty() || !(links_ok || BASIC_STARTS.contains(&load)) {
        return None;
    }
    Some(Program {
        load,
        lines,
        trailing_bytes: prg.len().saturating_sub(pos),
    })
}

/// Whether a line uses a Simons' BASIC keyword outside quotes.
fn has_simons_tokens(body: &[u8]) -> bool {
    let mut quoted = false;
    body.windows(2).any(|w| {
        if w[0] == b'"' {
            quoted = !quoted;
        }
        !quoted && w[0] == 0x64 && (0x01..=0x7F).contains(&w[1])
    })
}

/// Append one line's tokenized body, expanded, to `out`.
fn list_line(body: &[u8], dialect: Dialect, out: &mut String) {
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
            let next = body.get(i + 1).copied();
            match (dialect, b, next) {
                (_, 0x80..=0xCB, _) => out.push_str(KEYWORDS[usize::from(b - 0x80)]),
                (_, 0xFF, _) => out.push('π'),
                (Dialect::V7, 0xCE | 0xFE, Some(n)) => {
                    let table: &[&str] = if b == 0xCE { &V7_CE } else { &V7_FE };
                    match table.get(usize::from(n).wrapping_sub(2)) {
                        Some(k) if !k.is_empty() => out.push_str(k),
                        _ => out.push_str(&format!("{{${b:02X}{n:02X}}}")),
                    }
                    i += 1;
                }
                (Dialect::V7, 0xCC..=0xFD, _) => match V7_KEYWORDS[usize::from(b - 0xCC)] {
                    "" => out.push_str(&format!("{{${b:02X}}}")),
                    k => out.push_str(k),
                },
                (Dialect::Simons, 0x64, Some(n @ 0x01..=0x7F)) => {
                    match SIMONS[usize::from(n - 1)] {
                        "" => out.push_str(&format!("{{$64{n:02X}}}")),
                        k => out.push_str(k),
                    }
                    i += 1;
                }
                // Tokens the dialect doesn't have.
                (_, 0xCC..=0xFE, _) => out.push_str(&format!("{{${b:02X}}}")),
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
    fn dialect_tables_have_their_token_ranges() {
        assert_eq!(V7_KEYWORDS.len(), 0xFD - 0xCC + 1);
        assert_eq!(V7_KEYWORDS[0xD5 - 0xCC], "ELSE");
        assert_eq!(V7_KEYWORDS[0xFD - 0xCC], "WHILE");
        assert_eq!(V7_CE[0x0A - 2], "POINTER");
        assert_eq!(V7_FE[0x26 - 2], "SLOW");
        assert_eq!(SIMONS.len(), 0x7F);
        assert_eq!(SIMONS[0x01 - 1], "HIRES");
        assert_eq!(SIMONS[0x7F - 1], "OUT");
    }

    #[test]
    fn c128_programs_list_as_basic_7() {
        // 10 GRAPHIC 1,1:DO:LOOP UNTIL JOY(1)
        // 20 BANK 0:FAST:X=POT(1)
        let p = prg(
            0x1C01,
            &[
                (10, b"\xde 1,1:\xeb:\xec \xfc \xcf(1)"),
                (20, b"\xfe\x02 0:\xfe\x25:X=\xce\x02(1)"),
            ],
        );
        let l = detokenize(&p).unwrap();
        assert_eq!(l.dialect, Dialect::V7);
        assert_eq!(
            l.text,
            "10 GRAPHIC 1,1:DO:LOOP UNTIL JOY(1)\n20 BANK 0:FAST:X=POT(1)\n"
        );
        // Unused two-byte tokens show their bytes.
        let p = prg(0x1C01, &[(1, b"\xfe\x20:\xce\x7f")]);
        assert_eq!(detokenize(&p).unwrap().text, "1 {$FE20}:{$CE7F}\n");
        // The same bytes as V2: tokens it doesn't have.
        let p = prg(0x0801, &[(10, b"\xde 1")]);
        assert_eq!(detokenize(&p).unwrap().text, "10 {$DE} 1\n");
        let as7 = detokenize_as(&p, Dialect::V7).unwrap();
        assert_eq!(
            (as7.text.as_str(), as7.dialect),
            ("10 GRAPHIC 1\n", Dialect::V7)
        );
    }

    #[test]
    fn simons_tokens_are_detected_outside_quotes_only() {
        // 10 HIRES 0,1:PRINT AT(1,2)"X":CIRCLE 160,100,50,50,1
        let p = prg(
            0x0801,
            &[(
                10,
                b"\x64\x01 0,1:\x99\x64\x28 1,2)\"X\":\x64\x42 160,100,50,50,1",
            )],
        );
        let l = detokenize(&p).unwrap();
        assert_eq!(l.dialect, Dialect::Simons);
        assert_eq!(
            l.text,
            "10 HIRES 0,1:PRINTAT( 1,2)\"X\":CIRCLE 160,100,50,50,1\n"
        );
        // $64 followed by a token byte inside quotes is just text.
        let p = prg(0x0801, &[(10, b"\x99\"\x64\x01\"")]);
        assert_eq!(detokenize(&p).unwrap().dialect, Dialect::V2);
        // Unassigned Simons slots show their bytes.
        let p = prg(0x0801, &[(10, b"\x64\x22")]);
        assert_eq!(detokenize(&p).unwrap().text, "10 {$6422}\n");
    }

    #[test]
    fn missing_end_marker_is_tolerated() {
        let mut p = prg(0x0801, &[(10, b"\x80")]);
        p.truncate(p.len() - 2);
        assert_eq!(detokenize(&p).unwrap().text, "10 END\n");
    }
}
