//! BASIC detokenizer differential against VICE's `petcat`.
//!
//! `petcat -w2` tokenizes BASIC source text into a C64 PRG independently of
//! this crate; `detokenize_basic` must list it back as the same program.
//! Skipped when `petcat` is absent.

use std::fs;
use std::process::Command;

use sector64::detokenize_basic;

fn petcat_available() -> bool {
    Command::new("petcat")
        .arg("-help")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
        .unwrap_or(false)
}

/// Tokenize `source` (petcat syntax) into a PRG with petcat.
fn tokenize(name: &str, source: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("s64-petcat-{}-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let txt = dir.join("prog.txt");
    let prg = dir.join("prog.prg");
    fs::write(&txt, source).unwrap();
    let out = Command::new("petcat")
        .args(["-w2", "-o"])
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
