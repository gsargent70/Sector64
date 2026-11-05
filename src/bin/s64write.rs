//! `s64write` — create and edit D64 images from the shell.
//!
//! ```text
//! s64write format <image.d64> <disk name> <id>
//! s64write add    <image.d64> <cbm name> <host file> [prg|seq|usr]
//! s64write delete <image.d64> <cbm name>
//! s64write rename <image.d64> <old name> <new name>
//! ```
//!
//! `add` stores the host file's raw bytes (for a PRG, that includes its two-byte
//! load address). Edits are written back to the same image file.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use sector64::{Disk, FileType};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let strs: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();

    let result = match strs.first().map(String::as_str) {
        Some("format") if strs.len() == 4 || strs.len() == 5 => {
            cmd_format(&strs[1], &strs[2], &strs[3], strs.get(4).map(String::as_str))
        }
        Some("add") if strs.len() == 4 || strs.len() == 5 => {
            cmd_add(&strs[1], &strs[2], &strs[3], strs.get(4).map(String::as_str))
        }
        Some("delete") if strs.len() == 3 => cmd_delete(&strs[1], &strs[2]),
        Some("rename") if strs.len() == 4 => cmd_rename(&strs[1], &strs[2], &strs[3]),
        Some("import") if strs.len() == 3 => cmd_import(&strs[1], &strs[2]),
        Some("unzip") if strs.len() == 3 => cmd_unzip(&strs[1], &strs[2]),
        _ => {
            eprintln!(
                "usage:\n  \
                 s64write format <image> <disk name> <id> [d64|d71|d81]\n  \
                 s64write add    <image.d64> <cbm name> <host file> [prg|seq|usr]\n  \
                 s64write delete <image.d64> <cbm name>\n  \
                 s64write rename <image.d64> <old name> <new name>\n  \
                 s64write import <image.d64> <archive.t64|.lnx|.lbr>\n  \
                 s64write unzip  <out.d64> <zipcode base>   (reads 1!base..4!base)"
            );
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("s64write: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn parse_type(s: Option<&str>) -> Result<FileType, String> {
    match s.map(str::to_ascii_lowercase).as_deref() {
        None | Some("prg") => Ok(FileType::Prg),
        Some("seq") => Ok(FileType::Seq),
        Some("usr") => Ok(FileType::Usr),
        Some(other) => Err(format!("unknown file type '{other}' (use prg, seq, or usr)")),
    }
}

fn load(image: &str) -> Result<Disk, String> {
    let bytes =
        std::fs::read(image).map_err(|e| format!("cannot read {image}: {e}"))?;
    Disk::load(bytes).map_err(|e| format!("{image}: {e}"))
}

fn save(image: &str, disk: &Disk) -> Result<(), String> {
    std::fs::write(image, disk.as_bytes()).map_err(|e| format!("cannot write {image}: {e}"))
}

fn cmd_format(image: &str, name: &str, id: &str, kind: Option<&str>) -> Result<String, String> {
    if Path::new(image).exists() {
        return Err(format!("{image} already exists; refusing to overwrite"));
    }
    let disk = match kind.map(str::to_ascii_lowercase).as_deref() {
        None | Some("d64") => Disk::format(name, id),
        Some("d71") => Disk::format_d71(name, id),
        Some("d81") => Disk::format_d81(name, id),
        Some(other) => return Err(format!("unknown disk kind '{other}' (use d64, d71, or d81)")),
    };
    save(image, &disk)?;
    Ok(format!(
        "formatted {image}: \"{name}\" {id}, {} blocks free",
        disk.blocks_free().unwrap_or(0)
    ))
}

fn cmd_add(image: &str, name: &str, host: &str, ty: Option<&str>) -> Result<String, String> {
    let file_type = parse_type(ty)?;
    let data = std::fs::read(host).map_err(|e| format!("cannot read {host}: {e}"))?;
    let mut disk = load(image)?;
    disk.write_file(name, file_type, &data)
        .map_err(|e| format!("writing \"{name}\": {e}"))?;
    save(image, &disk)?;
    Ok(format!(
        "added \"{name}\" ({} bytes, {}), {} blocks free",
        data.len(),
        file_type.mnemonic(),
        disk.blocks_free().unwrap_or(0)
    ))
}

fn cmd_delete(image: &str, name: &str) -> Result<String, String> {
    let mut disk = load(image)?;
    let freed = disk
        .scratch(name)
        .map_err(|e| format!("deleting \"{name}\": {e}"))?;
    save(image, &disk)?;
    Ok(format!(
        "deleted \"{name}\", freed {freed} blocks, {} blocks free",
        disk.blocks_free().unwrap_or(0)
    ))
}

fn cmd_import(image: &str, archive: &str) -> Result<String, String> {
    let bytes = std::fs::read(archive).map_err(|e| format!("cannot read {archive}: {e}"))?;
    let lower = archive.to_ascii_lowercase();
    let files = if lower.ends_with(".t64") {
        sector64::parse_t64(&bytes).map_err(|e| format!("{archive}: {e}"))?
    } else if lower.ends_with(".lnx") {
        sector64::parse_lnx(&bytes).map_err(|e| format!("{archive}: {e}"))?
    } else if lower.ends_with(".lbr") {
        sector64::parse_lbr(&bytes).map_err(|e| format!("{archive}: {e}"))?
    } else {
        return Err(format!("unsupported archive '{archive}' (expected .t64, .lnx or .lbr)"));
    };
    let mut disk = load(image)?;
    let mut n = 0;
    for f in &files {
        disk.write_file(&f.name, f.file_type, &f.data)
            .map_err(|e| format!("importing \"{}\": {e}", f.name))?;
        n += 1;
    }
    save(image, &disk)?;
    Ok(format!(
        "imported {n} file(s) from {archive}, {} blocks free",
        disk.blocks_free().unwrap_or(0)
    ))
}

fn cmd_unzip(out_image: &str, base: &str) -> Result<String, String> {
    if Path::new(out_image).exists() {
        return Err(format!("{out_image} already exists; refusing to overwrite"));
    }
    let p = Path::new(base);
    let name = p
        .file_name()
        .ok_or_else(|| format!("invalid zipcode base '{base}'"))?
        .to_string_lossy();
    let parent = p.parent().unwrap_or_else(|| Path::new("."));
    let mut data = Vec::new();
    for i in 1..=4u8 {
        let path = parent.join(format!("{i}!{name}"));
        data.push(std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?);
    }
    let d64 = sector64::parse_zipcode([&data[0], &data[1], &data[2], &data[3]])
        .map_err(|e| format!("decoding zipcode '{base}': {e}"))?;
    std::fs::write(out_image, &d64).map_err(|e| format!("cannot write {out_image}: {e}"))?;
    Ok(format!("unzipped {base} to {out_image} ({} bytes)", d64.len()))
}

fn cmd_rename(image: &str, old: &str, new: &str) -> Result<String, String> {
    let mut disk = load(image)?;
    disk.rename(old, new)
        .map_err(|e| format!("renaming \"{old}\": {e}"))?;
    save(image, &disk)?;
    Ok(format!("renamed \"{old}\" to \"{new}\""))
}
