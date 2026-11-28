//! Reading any supported disk image or archive as a flat list of files,
//! each with its name, type and decoded bytes.

use std::path::Path;

use crate::container::{parse_lbr, parse_lnx, parse_p00, parse_t64, ImportFile};
use crate::disk::Disk;
use crate::error::Result;

/// One file in a listing.
#[derive(Debug, Clone)]
pub struct ListedFile {
    pub name: String,
    /// The type's mnemonic (`prg`, `seq`, …), or empty when the format
    /// doesn't record one (LHA and Wraptor).
    pub kind: &'static str,
    /// The decoded bytes, or why they couldn't be read.
    pub data: Result<Vec<u8>>,
    /// For a compressed member, its packed size and the method's name.
    pub packed: Option<(usize, String)>,
}

/// An archive's kind and contents.
#[derive(Debug, Clone)]
pub struct Archive {
    /// What it is, for display (e.g. `T64 tape archive`).
    pub label: &'static str,
    pub files: Vec<ListedFile>,
}

/// The files on a disk, in directory order.
pub fn disk_files(disk: &Disk) -> Result<Vec<ListedFile>> {
    Ok(disk
        .directory()?
        .into_iter()
        .map(|e| ListedFile {
            data: disk.read_entry(&e),
            kind: e.file_type.mnemonic(),
            name: e.name,
            packed: None,
        })
        .collect())
}

/// Read an archive, choosing the format by the file extension (any case, no
/// dot). Returns `None` when the extension isn't one of an archive's.
pub fn read_archive(ext: &str, bytes: &[u8]) -> Option<Result<Archive>> {
    let ext = ext.to_ascii_lowercase();
    let (label, files) = match ext.as_str() {
        "arc" | "sda" => ("ARC/SDA archive", arc_files(bytes)),
        "lha" | "lzh" | "sfx" => ("LHA/SFX archive", lha_files(bytes)),
        "lbr" => ("LBR library", parse_lbr(bytes).map(stored)),
        "t64" => ("T64 tape archive", parse_t64(bytes).map(stored)),
        "lnx" => ("Lynx archive", parse_lnx(bytes).map(stored)),
        "wr3" => ("Wraptor-3 archive", wr3_files(bytes)),
        e if is_p00(e) => ("PC64 container", parse_p00(bytes).map(|f| stored(vec![f]))),
        "sid" => ("SID music file", sid_file(bytes)),
        _ => return None,
    };
    Some(files.map(|files| Archive { label, files }))
}

/// The file names of a Zip-Code set's four parts (`1!NAME` to `4!NAME`), if
/// `path` names one of them.
pub fn zipcode_parts(path: &Path) -> Option<[String; 4]> {
    let file = path.file_name()?.to_str()?;
    let rest = match file.as_bytes() {
        [b'1'..=b'4', b'!', _, ..] => &file[2..],
        _ => return None,
    };
    Some([1, 2, 3, 4].map(|n| format!("{n}!{rest}")))
}

/// `p00`-style extensions: the type's letter and two digits.
fn is_p00(ext: &str) -> bool {
    matches!(
        ext.as_bytes(),
        [b'p' | b's' | b'u' | b'r' | b'd', b'0'..=b'9', b'0'..=b'9']
    )
}

/// A SID file as a listing of one: the tune, named by its title, with the
/// whole file (header included) as its bytes.
fn sid_file(bytes: &[u8]) -> Result<Vec<ListedFile>> {
    let info = crate::sid::parse_sid(bytes)?;
    let name = if info.title.is_empty() {
        "SID TUNE".to_string()
    } else {
        info.title
    };
    Ok(vec![ListedFile {
        name,
        kind: "sid",
        data: Ok(bytes.to_vec()),
        packed: None,
    }])
}

fn stored(files: Vec<ImportFile>) -> Vec<ListedFile> {
    files
        .into_iter()
        .map(|f| ListedFile {
            name: f.name,
            kind: f.file_type.mnemonic(),
            data: Ok(f.data),
            packed: None,
        })
        .collect()
}

fn arc_files(bytes: &[u8]) -> Result<Vec<ListedFile>> {
    // A self-dissolving archive is a BASIC program, loading at $0801.
    let entries = if bytes.starts_with(&[0x01, 0x08]) {
        crate::parse_sda(bytes)?
    } else {
        crate::parse_arc(bytes)?
    };
    Ok(entries
        .into_iter()
        .map(|e| ListedFile {
            kind: e.file_type.mnemonic(),
            data: e.decode(),
            packed: Some((e.packed.len(), e.method.label().to_string())),
            name: e.name,
        })
        .collect())
}

fn lha_files(bytes: &[u8]) -> Result<Vec<ListedFile>> {
    Ok(crate::parse_lha(bytes)?
        .into_iter()
        .map(|e| ListedFile {
            kind: "",
            data: e.decode(),
            packed: Some((e.compressed_size as usize, e.method_str().to_string())),
            name: e.name,
        })
        .collect())
}

fn wr3_files(bytes: &[u8]) -> Result<Vec<ListedFile>> {
    // Members share one sliding window, so the whole archive is decoded.
    Ok(crate::decompress_wr3(bytes)?
        .into_iter()
        .map(|f| ListedFile {
            name: f.name,
            kind: "",
            data: Ok(f.content),
            packed: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{write_p00, write_t64, FileType};

    #[test]
    fn reads_disks_and_archives_alike() {
        let data = vec![0x01, 0x08, 0xAA];
        let mut disk = Disk::format("D", "D1");
        disk.write_file("ONE", FileType::Prg, &data).unwrap();
        let file = ImportFile {
            name: "ONE".into(),
            file_type: FileType::Prg,
            data: data.clone(),
        };
        let t64 = read_archive("T64", &write_t64(std::slice::from_ref(&file)));
        let p00 = read_archive("p00", &write_p00(&file));
        let lists = [
            disk_files(&disk).unwrap(),
            t64.unwrap().unwrap().files,
            p00.unwrap().unwrap().files,
        ];
        for files in lists {
            assert_eq!(files.len(), 1);
            assert_eq!((files[0].name.as_str(), files[0].kind), ("ONE", "prg"));
            assert_eq!(files[0].data.as_ref().unwrap(), &data);
        }
    }

    #[test]
    fn unknown_extensions_are_not_archives() {
        for ext in ["", "txt", "p0", "p0x", "x00", "p000"] {
            assert!(read_archive(ext, b"").is_none(), "{ext}");
        }
        assert!(read_archive("t64", b"junk").unwrap().is_err());
    }

    #[test]
    fn zipcode_part_names() {
        let parts = zipcode_parts(Path::new("some/dir/3!GAME")).unwrap();
        assert_eq!(parts, ["1!GAME", "2!GAME", "3!GAME", "4!GAME"]);
        for name in ["5!GAME", "1!", "GAME", "1GAME"] {
            assert!(zipcode_parts(Path::new(name)).is_none(), "{name}");
        }
    }
}
