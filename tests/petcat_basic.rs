//! BASIC detokenizer differential against VICE's `petcat`.
//!
//! `petcat` tokenizes BASIC source text into a PRG independently of this
//! crate (`-w2` for BASIC V2, `-w70` for the C128's BASIC 7.0, `-wsimon` for
//! Simons' BASIC); `detokenize_basic` must list it back as the same program,
//! recognising the dialect by itself. Skipped when `petcat` is absent.

use std::fs;
use std::process::Command;

use sector64::{detokenize_basic, BasicDialect};

fn petcat_available() -> bool {
    Command::new("petcat")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

/// Tokenize `source` (petcat syntax) into a C64 BASIC V2 PRG with petcat.
fn tokenize(name: &str, source: &str) -> Vec<u8> {
    tokenize_as("2", name, source)
}

/// Tokenize `source` with petcat's `-w<version>`.
fn tokenize_as(version: &str, name: &str, source: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("s64-petcat-{}-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let txt = dir.join("prog.txt");
    let prg = dir.join("prog.prg");
    fs::write(&txt, source).unwrap();
    let out = Command::new("petcat")
        .arg(format!("-w{version}"))
        .arg("-o")
        .arg(&prg)
        .arg("--")
        .arg(&txt)
        .output()
        .unwrap();
    assert!(out.status.success(), "petcat failed: {out:?}");
    let bytes = fs::read(&prg).unwrap();
    let _ = fs::remove_dir_all(&dir);
    bytes
}

#[test]
fn lists_a_petcat_tokenized_program() {
    if !petcat_available() {
        eprintln!("petcat not found; skipping");
        return;
    }
    let source = r#"10 rem *** starfall ***
20 print "{clr}{down}{down}{down}{down}{down}{rvon} starfall {rvof}":poke 53280,0
30 for i=1 to 10:x=int(rnd(1)*40):print tab(x);"*";:next i
40 input "your name";n$:if n$="" then 40
50 a$=left$(n$,3)+mid$(n$,2,1)+right$(n$,1):print a$;len(a$)
60 on x gosub 100,200:get k$:if k$<>"q" goto 30
70 data 1,2,3,"hi"
80 print#1,chr$(147);spc(5);sqr(2)*~
90 sys 64738:end
100 return
200 wait 198,1:return
"#;
    let expected = r#"10 REM *** STARFALL ***
20 PRINT "{CLR}{5 DOWN}{RVS ON} STARFALL {RVS OFF}":POKE 53280,0
30 FOR I=1 TO 10:X=INT(RND(1)*40):PRINT TAB(X);"*";:NEXT I
40 INPUT "YOUR NAME";N$:IF N$="" THEN 40
50 A$=LEFT$(N$,3)+MID$(N$,2,1)+RIGHT$(N$,1):PRINT A$;LEN(A$)
60 ON X GOSUB 100,200:GET K$:IF K$<>"Q" GOTO 30
70 DATA 1,2,3,"HI"
80 PRINT#1,CHR$(147);SPC(5);SQR(2)*π
90 SYS 64738:END
100 RETURN
200 WAIT 198,1:RETURN
"#;
    let listing = detokenize_basic(&tokenize("program", source)).expect("a BASIC program");
    assert_eq!(listing.load_address, 0x0801);
    assert_eq!(listing.lines, 11);
    assert_eq!(listing.trailing_bytes, 0);
    assert_eq!(listing.text, expected);
}

#[test]
fn every_keyword_round_trips_through_petcat() {
    if !petcat_available() {
        eprintln!("petcat not found; skipping");
        return;
    }
    let keywords = [
        "end", "for", "next", "data", "input#", "input", "dim", "read", "let", "goto", "run", "if",
        "restore", "gosub", "return", "rem", "stop", "on", "wait", "load", "save", "verify", "def",
        "poke", "print#", "print", "cont", "list", "clr", "cmd", "sys", "open", "close", "get",
        "new", "tab(", "to", "fn", "spc(", "then", "not", "step", "+", "-", "*", "/", "^", "and",
        "or", ">", "=", "<", "sgn", "int", "abs", "usr", "fre", "pos", "sqr", "rnd", "log", "exp",
        "cos", "sin", "tan", "atn", "peek", "len", "str$", "val", "asc", "chr$", "left$", "right$",
        "mid$", "go",
    ];
    assert_eq!(keywords.len(), 76);
    let source: String = keywords
        .iter()
        .enumerate()
        .map(|(i, k)| format!("{} {k}\n", i + 1))
        .collect();
    let prg = tokenize("keywords", &source);
    // Each line must hold exactly one byte: petcat tokenized the keyword.
    let listing = detokenize_basic(&prg).expect("a BASIC program");
    let expected: String = keywords
        .iter()
        .enumerate()
        .map(|(i, k)| format!("{} {}\n", i + 1, k.to_uppercase()))
        .collect();
    assert_eq!(listing.text, expected);
    assert_eq!(
        prg.len(),
        2 + 76 * 6 + 2,
        "every keyword should be one token"
    );
}

/// List `keywords` one per line through petcat and back, and check each
/// became a token of `token_len` bytes.
fn round_trip(version: &str, dialect: BasicDialect, keywords: &[(&str, usize)]) {
    let source: String = keywords
        .iter()
        .enumerate()
        .map(|(i, (k, _))| format!("{} {}\n", i + 1, k.to_lowercase()))
        .collect();
    let prg = tokenize_as(version, version, &source);
    let listing = detokenize_basic(&prg).expect("a BASIC program");
    assert_eq!(listing.dialect, dialect);
    let expected: String = keywords
        .iter()
        .enumerate()
        .map(|(i, (k, _))| format!("{} {k}\n", i + 1))
        .collect();
    assert_eq!(listing.text, expected);
    // Link, line number and terminator: 5 bytes a line, plus the token.
    let tokens: usize = keywords.iter().map(|(_, n)| 5 + n).sum();
    assert_eq!(
        prg.len(),
        2 + tokens + 2,
        "every keyword should be one token"
    );
}

#[test]
fn basic_7_keywords_round_trip_through_petcat() {
    if !petcat_available() {
        eprintln!("petcat not found; skipping");
        return;
    }
    let one = [
        "RGR",
        "RCLR",
        "JOY",
        "RDOT",
        "DEC",
        "HEX$",
        "ERR$",
        "INSTR",
        "ELSE",
        "RESUME",
        "TRAP",
        "TRON",
        "TROFF",
        "SOUND",
        "VOL",
        "AUTO",
        "PUDEF",
        "GRAPHIC",
        "PAINT",
        "CHAR",
        "BOX",
        "CIRCLE",
        "GSHAPE",
        "SSHAPE",
        "DRAW",
        "LOCATE",
        "COLOR",
        "SCNCLR",
        "SCALE",
        "HELP",
        "DO",
        "LOOP",
        "EXIT",
        "DIRECTORY",
        "DSAVE",
        "DLOAD",
        "HEADER",
        "SCRATCH",
        "COLLECT",
        "COPY",
        "RENAME",
        "BACKUP",
        "DELETE",
        "RENUMBER",
        "KEY",
        "MONITOR",
        "USING",
        "UNTIL",
        "WHILE",
        // V2 keywords are still single bytes.
        "PRINT",
        "GOTO",
    ];
    let two = [
        "POT",
        "BUMP",
        "PEN",
        "RSPPOS",
        "RSPRITE",
        "RSPCOLOR",
        "XOR",
        "RWINDOW",
        "POINTER",
        "BANK",
        "FILTER",
        "PLAY",
        "TEMPO",
        "MOVSPR",
        "SPRITE",
        "SPRCOLOR",
        "RREG",
        "ENVELOPE",
        "SLEEP",
        "CATALOG",
        "DOPEN",
        "APPEND",
        "DCLOSE",
        "BSAVE",
        "BLOAD",
        "RECORD",
        "CONCAT",
        "DVERIFY",
        "DCLEAR",
        "SPRSAV",
        "COLLISION",
        "BEGIN",
        "BEND",
        "WINDOW",
        "BOOT",
        "WIDTH",
        "SPRDEF",
        "QUIT",
        "STASH",
        "FETCH",
        "SWAP",
        "OFF",
        "FAST",
        "SLOW",
    ];
    let keywords: Vec<(&str, usize)> = one
        .iter()
        .map(|&k| (k, 1))
        .chain(two.iter().map(|&k| (k, 2)))
        .collect();
    round_trip("70", BasicDialect::V7, &keywords);
}

#[test]
fn simons_keywords_round_trip_through_petcat() {
    if !petcat_available() {
        eprintln!("petcat not found; skipping");
        return;
    }
    let simons = [
        "HIRES", "PLOT", "LINE", "BLOCK", "FCHR", "FCOL", "FILL", "REC", "ROT", "DRAW", "CHAR",
        "HI COL", "INV", "FRAC", "MOVE", "PLACE", "UPB", "UPW", "LEFTW", "LEFTB", "DOWNB", "DOWNW",
        "RIGHTB", "RIGHTW", "MULTI", "COLOUR", "MMOB", "BFLASH", "MOB SET", "MUSIC", "FLASH",
        "REPEAT", "PLAY", "CENTRE", "ENVELOPE", "CGOTO", "WAVE", "FETCH", "AT(", "UNTIL", "USE",
        "GLOBAL", "RESET", "PROC", "CALL", "EXEC", "END PROC", "EXIT", "END LOOP", "ON KEY",
        "DISABLE", "RESUME", "LOOP", "DELAY", "SECURE", "DISAPA", "CIRCLE", "ON ERROR", "NO ERROR",
        "LOCAL", "RCOMP", "ELSE", "RETRACE", "TRACE", "DIR", "PAGE", "DUMP", "FIND", "OPTION",
        "AUTO", "OLD", "JOY", "MOD", "DIV", "DUP", "INKEY", "INST", "TEST", "LIN", "EXOR",
        "INSERT", "POT", "PENX", "PENY", "SOUND", "GRAPHICS", "DESIGN", "RLOCMOB", "CMOB",
        "BCKGNDS", "PAUSE", "NRM", "MOB OFF", "OFF", "ANGL", "ARC", "COLD", "SCRSV", "SCRLD",
        "TEXT", "CSET", "VOL", "DISK", "HRDCPY", "KEY", "PAINT", "LOW COL", "COPY", "MERGE",
        "RENUMBER", "MEM", "DETECT", "CHECK", "DISPLAY", "ERR", "OUT",
    ];
    let mut keywords: Vec<(&str, usize)> = simons.iter().map(|&k| (k, 2)).collect();
    keywords.push(("PRINT", 1));
    round_trip("simon", BasicDialect::Simons, &keywords);
}

#[test]
fn dialect_programs_list_like_their_source() {
    if !petcat_available() {
        eprintln!("petcat not found; skipping");
        return;
    }
    let c128 = "10 graphic 1,1:color 1,2\n20 do:x=pot(1):loop until joy(2)\n30 if x then begin:print hex$(x):bend:else fast\n";
    let listing = detokenize_basic(&tokenize_as("70", "c128", c128)).unwrap();
    assert_eq!(listing.load_address, 0x1C01);
    assert_eq!(listing.dialect, BasicDialect::V7);
    assert_eq!(listing.text, c128.to_uppercase());

    let simons = "10 hires 0,1:circle 160,100,50,50,1\n20 repeat:get a$:until a$<>\"\"\n30 proc done:print \"bye\":end proc\n";
    let listing = detokenize_basic(&tokenize_as("simon", "simons", simons)).unwrap();
    assert_eq!(listing.load_address, 0x0801);
    assert_eq!(listing.dialect, BasicDialect::Simons);
    assert_eq!(listing.text, simons.to_uppercase());
}

/// This crate's own tokenizer must turn the same text into the same bytes.
fn same_as_petcat(version: &str, dialect: BasicDialect, name: &str, source: &str) {
    let theirs = tokenize_as(version, name, source);
    let ours = sector64::tokenize::tokenize(source, dialect, None).unwrap();
    assert_eq!(ours.prg, theirs, "{source}");
    // And from this crate's own listing of it.
    let listing = sector64::detokenize_basic_as(&theirs, dialect).unwrap();
    let again = sector64::tokenize::tokenize(&listing.text, dialect, None).unwrap();
    assert_eq!(again.prg, theirs, "{}", listing.text);
}

#[test]
fn tokenizes_like_petcat() {
    if !petcat_available() {
        eprintln!("petcat not found; skipping");
        return;
    }
    use BasicDialect::{Simons, V2, V7};
    let v2 = r#"10    print "{clr}{down}{rvon}Hi{rvof}{5 down}{CBM-A}{SHIFT-B}{$a1}":rem   print data
20 data for,"to":for i=1 to 9
30 goto10:go to 5:fori=atob:a$=ti$:st
40 print#1,x:input#2,y:get#3,z:open1,8,15
50 restore:step:str$(:a^2:a@[]_\
60 rem "a":print
70 print "open
80 b=~:x>=y<=z<>w
"#;
    same_as_petcat("2", V2, "tok2", v2);
    let v7 = "10 graphic 1,1:color 1,2:dopen#1:do:loop until joy(2)\n\
              20 if x then begin:print hex$(x):bend:else fast\n\
              30 printusing\"#\";a:getkey a$:x=pointer(a)+xor(1,2)\n\
              40 spc(1):sprcolor:end:bend:key1,\"x\":go64\n";
    same_as_petcat("70", V7, "tok7", v7);
    let simons = "10 hires 0,1:circle 160,100,50,50,1:lin:line:inst:insert\n\
                  20 repeat:get a$:until a$<>\"\":hi col 1:mob set 1:mob off\n\
                  30 proc done:print at(1,2)\"bye\":end proc:end:on key:on a goto 1\n";
    same_as_petcat("simon", Simons, "toks", simons);
}
