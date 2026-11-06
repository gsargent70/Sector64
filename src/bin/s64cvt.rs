//! `s64cvt` — describe or extract a GEOS **CVT** (ConVerT) container.
//!
//! Usage:
//!   `s64cvt <file.cvt>`            describe the GEOS file
//!   `s64cvt <file.cvt> <outdir>`   also write each record to <name>.rNN
//!
//! Prints the GEOS file's name, types, structure and (for VLIR) its record
//! index; with an output directory, writes the INFO block and each record's
//! data as separate files.

use std::path::PathBuf;
use std::process::ExitCode;

use sector64::{geos_type_name, parse_cvt, GeosStructure};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: s64cvt <file.cvt> [outdir]");
        return ExitCode::from(2);
    };
    let out_dir = args.next();
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

    if let Some(dir) = out_dir {
        let base: String = info
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || " .-_".contains(c) { c } else { '_' })
            .collect();
        let base = base.trim();
        let base = if base.is_empty() { "geos" } else { base };
        let dir = PathBuf::from(&dir);
        let info_path = dir.join(format!("{base}.info"));
        if let Err(e) = std::fs::write(&info_path, &info.info_block) {
            eprintln!("s64cvt: write {}: {e}", info_path.display());
            return ExitCode::FAILURE;
        }
        let mut written = 1;
        for (i, rec) in info.record_data.iter().enumerate() {
            if rec.is_empty() {
                continue; // non-existent record slot
            }
            let rp = dir.join(format!("{base}.r{i:02}"));
            if let Err(e) = std::fs::write(&rp, rec) {
                eprintln!("s64cvt: write {}: {e}", rp.display());
                return ExitCode::FAILURE;
            }
            written += 1;
        }
        println!("wrote {written} file(s) to {}", dir.display());
    }
    ExitCode::SUCCESS
}
