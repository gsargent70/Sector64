//! `s64cvt` — describe a GEOS **CVT** (ConVerT) container.
//!
//! Usage: `s64cvt <file.cvt>`
//!
//! Prints the GEOS file's name, types, structure and (for VLIR) its record
//! index. CVT holds a single GEOS file; record data is not split out (see the
//! `cvt` module note).

use std::process::ExitCode;

use sector64::{geos_type_name, parse_cvt, GeosStructure};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: s64cvt <file.cvt>");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("s64cvt: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let info = match parse_cvt(&bytes) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("s64cvt: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("name:      \"{}\"", info.name);
    println!("CBM type:  {}", info.cbm_type.mnemonic());
    println!(
        "GEOS type: {} ({})",
        info.geos_file_type,
        geos_type_name(info.geos_file_type)
    );
    println!(
        "structure: {}",
        match info.structure {
            GeosStructure::Sequential => "sequential",
            GeosStructure::Vlir => "VLIR",
        }
    );
    println!("signature: \"{}\"", info.signature);
    if info.structure == GeosStructure::Vlir {
        let present = info.records.iter().filter(|&&(c, s)| !(c == 0 && s == 0xFF)).count();
        println!("records:   {} slots, {present} present", info.records.len());
        for (i, &(c, s)) in info.records.iter().enumerate() {
            if !(c == 0 && s == 0xFF) {
                println!("  [{i:3}] {c} sector(s), last {s} bytes");
            }
        }
    }
    println!("data:      {} bytes", info.data.len());
    ExitCode::SUCCESS
}
