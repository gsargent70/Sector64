//! Format conversion in one step: a disk or an archive's files, written out
//! as a new D64, D71 or D81 disk or a T64, LNX or LBR archive.
//!
//! Disk to disk keeps each entry exactly (its name bytes, type, lock flag,
//! and separators) through [`Disk::copy_file_from`]. Everything else goes
//! file by file. Files that can't go across (REL files, GEOS files, a full
//! disk, an unreadable member) are listed with the reason rather than
//! stopping the conversion.

use crate::container::{write_lbr, write_lnx, write_t64, ImportFile};
use crate::directory::FileType;
use crate::disk::Disk;
use crate::error::Result;
use crate::listing::ListedFile;

/// What to convert to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    D64,
    D71,
    D81,
    T64,
    Lnx,
    Lbr,
}

impl Target {
    /// Every target, disks first.
    pub const ALL: [Target; 6] = [
        Target::D64,
        Target::D71,
        Target::D81,
        Target::T64,
        Target::Lnx,
        Target::Lbr,
    ];

    /// The target a file extension names (any case, no dot).
    pub fn from_ext(ext: &str) -> Option<Target> {
        Target::ALL
            .into_iter()
            .find(|t| t.ext().eq_ignore_ascii_case(ext))
    }

    /// The file extension, lower case and without the dot.
    pub fn ext(self) -> &'static str {
        match self {
            Target::D64 => "d64",
            Target::D71 => "d71",
            Target::D81 => "d81",
            Target::T64 => "t64",
            Target::Lnx => "lnx",
            Target::Lbr => "lbr",
        }
    }

    /// A disk image rather than an archive.
    pub fn is_disk(self) -> bool {
        matches!(self, Target::D64 | Target::D71 | Target::D81)
    }

    fn blank(self, name: &str, id: &str) -> Disk {
        match self {
            Target::D71 => Disk::format_d71(name, id),
            Target::D81 => Disk::format_d81(name, id),
            _ => Disk::format(name, id),
        }
    }

    fn pack(self, files: &[ImportFile]) -> Vec<u8> {
        match self {
            Target::Lnx => write_lnx(files),
            Target::Lbr => write_lbr(files),
            _ => write_t64(files),
        }
    }
}

/// A finished conversion.
#[derive(Clone)]
pub struct Converted {
    /// The new image or archive.
    pub bytes: Vec<u8>,
    /// The new disk, for a disk target.
    pub disk: Option<Disk>,
    /// The names of the files that went across.
    pub copied: Vec<String>,
    /// The files left behind, with why.
    pub skipped: Vec<(String, String)>,
    /// Things worth knowing that aren't failures, by entry: separators an
    /// archive leaves out, and types a T64 can't keep (it records every
    /// file as PRG, as VICE reads it).
    pub notes: Vec<(String, String)>,
}

impl Converted {
    fn from_disk(disk: Disk, copied: Vec<String>, skipped: Vec<(String, String)>) -> Converted {
        Converted {
            bytes: disk.as_bytes().to_vec(),
            disk: Some(disk),
            copied,
            skipped,
            notes: Vec::new(),
        }
    }

    fn from_archive(
        to: Target,
        files: Vec<ImportFile>,
        skipped: Vec<(String, String)>,
        mut notes: Vec<(String, String)>,
    ) -> Converted {
        if to == Target::T64 {
            notes.extend(
                files
                    .iter()
                    .filter(|f| f.file_type != FileType::Prg)
                    .map(|f| {
                        let was = f.file_type.mnemonic().to_uppercase();
                        let how = format!("stored as PRG, not {was}: a T64 holds only PRG files");
                        (f.name.clone(), how)
                    }),
            );
        }
        Converted {
            bytes: to.pack(&files),
            disk: None,
            copied: files.into_iter().map(|f| f.name).collect(),
            skipped,
            notes,
        }
    }
}

/// Convert a disk. A disk target gets the source's name and ID.
pub fn convert_disk(src: &Disk, to: Target) -> Result<Converted> {
    let entries = src.directory()?;
    let (mut copied, mut skipped) = (Vec::new(), Vec::new());
    if to.is_disk() {
        let header = src.header()?;
        let mut disk = to.blank(&header.name, &header.id);
        for e in &entries {
            match disk.copy_file_from(src, e) {
                Ok(()) => copied.push(e.name.clone()),
                Err(err) => skipped.push((e.name.clone(), err.to_string())),
            }
        }
        return Ok(Converted::from_disk(disk, copied, skipped));
    }
    let (mut files, mut notes) = (Vec::new(), Vec::new());
    for e in &entries {
        if src.is_placeholder(e) {
            notes.push((e.name.clone(), "a separator line, left out".to_string()));
            continue;
        }
        let why = if e.file_type == FileType::Rel {
            Some("REL files can't be converted yet".to_string())
        } else if src.is_geos() && (e.side_track != 0 || e.rel_record_len != 0) {
            Some("GEOS files can't be converted yet".to_string())
        } else {
            None
        };
        if let Some(why) = why {
            skipped.push((e.name.clone(), why));
            continue;
        }
        match src.read_entry(e) {
            Ok(data) => files.push(ImportFile {
                name: e.name.clone(),
                file_type: e.file_type,
                data,
            }),
            Err(err) => skipped.push((e.name.clone(), err.to_string())),
        }
    }
    Ok(Converted::from_archive(to, files, skipped, notes))
}

/// Convert an archive's (or any listing's) files. A disk target is named
/// `title` (cut to 16 characters) with ID `01`. Files whose format records
/// no type (LHA, Wraptor) become PRG files.
pub fn convert_files(title: &str, files: &[ListedFile], to: Target) -> Converted {
    let mut skipped = Vec::new();
    let mut members = Vec::new();
    for f in files {
        let file_type = match f.kind {
            "seq" => FileType::Seq,
            "usr" => FileType::Usr,
            "del" => FileType::Del,
            "rel" => {
                skipped.push((f.name.clone(), "REL files can't be converted yet".into()));
                continue;
            }
            _ => FileType::Prg,
        };
        match &f.data {
            Ok(data) => members.push(ImportFile {
                name: f.name.clone(),
                file_type,
                data: data.clone(),
            }),
            Err(e) => skipped.push((f.name.clone(), e.to_string())),
        }
    }
    if !to.is_disk() {
        return Converted::from_archive(to, members, skipped, Vec::new());
    }
    let name: String = title.chars().take(16).collect();
    let mut disk = to.blank(&name, "01");
    let mut copied = Vec::new();
    for f in members {
        match disk.write_file(&f.name, f.file_type, &f.data) {
            Ok(()) => copied.push(f.name),
            Err(e) => skipped.push((f.name, e.to_string())),
        }
    }
    Converted::from_disk(disk, copied, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::parse_t64;
    use crate::listing::disk_files;
    use crate::SEPARATOR;

    fn source() -> Disk {
        let mut d = Disk::format("SOURCE", "S1");
        d.write_file("GAME", FileType::Prg, &[1, 8, 1, 2, 3])
            .unwrap();
        d.write_file("NOTES", FileType::Seq, b"hello").unwrap();
        d.add_separator(SEPARATOR, Some(0)).unwrap();
        d.set_locked(0, true).unwrap();
        d
    }

    #[test]
    fn d64_to_d81_keeps_names_types_locks_and_separators() {
        let c = convert_disk(&source(), Target::D81).unwrap();
        let d = c.disk.unwrap();
        assert_eq!(d.track_count(), 80);
        let h = d.header().unwrap();
        assert_eq!((h.name.as_str(), h.id.as_str()), ("SOURCE", "S1"));
        let dir = d.directory().unwrap();
        let shown: Vec<_> = dir.iter().map(|e| (e.name.as_str(), e.file_type)).collect();
        assert_eq!(
            shown,
            [
                ("GAME", FileType::Prg),
                ("----------------", FileType::Del),
                ("NOTES", FileType::Seq)
            ]
        );
        assert!(dir[0].locked);
        assert_eq!(d.read_entry(&dir[2]).unwrap(), b"hello");
        assert!(c.skipped.is_empty());
        assert!(d.validate().unwrap().is_clean());
        assert_eq!(Disk::load(c.bytes).unwrap().directory().unwrap().len(), 3);
    }

    #[test]
    fn a_disk_to_t64_notes_the_separator_and_the_seq_file() {
        let c = convert_disk(&source(), Target::T64).unwrap();
        assert_eq!(c.copied, ["GAME", "NOTES"]);
        assert!(c.skipped.is_empty());
        let back = parse_t64(&c.bytes).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].data, b"hello");
        let notes: Vec<_> = c.notes.iter().map(|n| n.1.as_str()).collect();
        assert_eq!(
            notes,
            [
                "a separator line, left out",
                "stored as PRG, not SEQ: a T64 holds only PRG files"
            ]
        );
        let lnx = convert_disk(&source(), Target::Lnx).unwrap();
        assert_eq!(lnx.notes.len(), 1, "LNX keeps types: {:?}", lnx.notes);
    }

    #[test]
    fn an_archive_to_a_disk_types_each_file_and_names_the_disk() {
        let files = disk_files(&source()).unwrap();
        let c = convert_files("MY ARCHIVE", &files, Target::D64);
        let d = c.disk.unwrap();
        assert_eq!(d.header().unwrap().name, "MY ARCHIVE");
        let dir = d.directory().unwrap();
        let types: Vec<_> = dir.iter().map(|e| e.file_type).collect();
        // The separator came through as an empty DEL listing: a plain file.
        assert_eq!(types, [FileType::Prg, FileType::Del, FileType::Seq]);
        assert_eq!(d.read_entry(&dir[0]).unwrap(), [1, 8, 1, 2, 3]);
    }

    #[test]
    fn what_does_not_fit_is_listed_not_fatal() {
        let big = ListedFile {
            name: "BIG".into(),
            kind: "prg",
            data: Ok(vec![0; 200_000]),
            packed: None,
        };
        let small = ListedFile {
            name: "SMALL".into(),
            kind: "",
            data: Ok(vec![1]),
            packed: None,
        };
        let rel = ListedFile {
            name: "DATA".into(),
            kind: "rel",
            data: Ok(vec![1]),
            packed: None,
        };
        let c = convert_files("X", &[big, small, rel], Target::D64);
        assert_eq!(c.copied, ["SMALL"]);
        let why: Vec<_> = c.skipped.iter().map(|s| s.0.as_str()).collect();
        assert_eq!(why, ["DATA", "BIG"]);
    }

    #[test]
    fn targets_come_from_extensions() {
        assert_eq!(Target::from_ext("D81"), Some(Target::D81));
        assert_eq!(Target::from_ext("lnx"), Some(Target::Lnx));
        assert_eq!(Target::from_ext("g64"), None);
    }
}
