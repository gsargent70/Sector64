//! `s64ls` — print the directory of a D64 image in the classic CBM layout.
//!
//! Usage: `s64ls <image.d64>`

use std::process::ExitCode;

use sector64::Disk;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: s64ls <image.d64>");
        return ExitCode::from(2);
    };

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("s64ls: cannot read {}: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };

    let disk = match Disk::load(bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("s64ls: {e}");
            return ExitCode::FAILURE;
        }
    };

    match print_listing(&disk) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("s64ls: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_listing(disk: &Disk) -> sector64::Result<()> {
    let header = disk.header()?;
    // Reverse-video header line, as `LOAD"$",8` would show it.
    println!("0 \"{:<16}\" {} {}", header.name, header.id, header.dos_type);

    for e in disk.directory()? {
        let (splat, lock) = e.flags();
        // blocks, then "name" in quotes, then the type with its flags.
        let quoted = format!("\"{}\"", e.name);
        println!("{:<5}{:<18}{}{}{}", e.blocks, quoted, splat, e.file_type.mnemonic(), lock);
    }

    println!("{} blocks free.", disk.blocks_free()?);
    Ok(())
}
