//! Host files and listings: opening, naming, importing, copying, recent files.

use crate::*;

/// An Open dialog that lists `exts` first, with an "All files" choice. macOS
/// dialogs have no such choice: they merge every filter into one and take no
/// wildcard, which would leave files like Zip-Code's `1!NAME` unselectable,
/// so there the dialog just shows everything.
pub(crate) fn open_dialog(name: &str, exts: &[&str]) -> rfd::FileDialog {
    let dlg = rfd::FileDialog::new();
    if cfg!(target_os = "macos") {
        dlg
    } else {
        dlg.add_filter(name, exts).add_filter("All files", &["*"])
    }
}

/// A CBM file name derived from a host path: the stem, uppercased, ≤16 chars.
pub(crate) fn cbm_name_from(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    stem.to_uppercase().chars().take(16).collect()
}

/// What a host file becomes on a disk: its Commodore name, type and data.
///
/// - `.prg`, `.seq`, `.usr`, `.del` (any case) set the type, and the name is
///   the rest of the file name.
/// - PC64 files (`.P00`, `.S00`, `.U00`, `.D00`) keep the name in their
///   header, and the type is the extension's letter.
/// - Anything else gets `fallback`, and keeps its whole file name, so a file
///   extracted as `GAME V1.2` comes back with its dot.
///
/// REL files are refused: adding one would need its record length and side
/// sectors, which a plain host file doesn't carry.
pub(crate) fn host_import(
    path: &Path,
    bytes: Vec<u8>,
    fallback: FileType,
) -> std::result::Result<ImportFile, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let named = |file_type| ImportFile {
        name: cbm_name_from(path),
        file_type,
        data: bytes.clone(),
    };
    let pc64 = match ext.as_bytes() {
        [t @ (b'p' | b's' | b'u' | b'r' | b'd'), b'0'..=b'9', b'0'..=b'9'] => Some(*t),
        _ => None,
    };
    match (ext.as_str(), pc64) {
        ("prg", _) => Ok(named(FileType::Prg)),
        ("seq", _) => Ok(named(FileType::Seq)),
        ("usr", _) => Ok(named(FileType::Usr)),
        ("del", _) => Ok(named(FileType::Del)),
        ("rel", _) | (_, Some(b'r')) => Err("REL files can't be added".into()),
        (_, Some(t)) => {
            let mut f = sector64::parse_p00(&bytes)
                .map_err(|_| "not a PC64 file: its C64File header is missing".to_string())?;
            f.file_type = match t {
                b's' => FileType::Seq,
                b'u' => FileType::Usr,
                b'd' => FileType::Del,
                _ => FileType::Prg,
            };
            if f.name.is_empty() {
                f.name = cbm_name_from(path);
            }
            Ok(f)
        }
        _ => {
            let whole = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
            Ok(ImportFile {
                name: whole.to_uppercase().chars().take(16).collect(),
                file_type: fallback,
                data: bytes,
            })
        }
    }
}

/// Replace characters unsafe in a host file name.
pub(crate) fn sanitize(name: &str) -> String {
    let n: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || " .-_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let n = n.trim();
    if n.is_empty() {
        "file".into()
    } else {
        n.to_string()
    }
}

pub(crate) enum Opened {
    Disk(Disk),
    /// A disk shown as a listing only (title, rows with block counts, and the
    /// disk itself for the map).
    ReadOnlyDisk(String, Vec<Node>, Disk),
    /// An archive or container listing (title, rows).
    List(String, Vec<Node>),
}

/// Build the listing (title, items) from a disk.
pub(crate) fn disk_nodes(disk: &Disk) -> (String, Vec<Node>) {
    let header = disk.header();
    let entries = disk.directory().unwrap_or_default();
    // Width of the widest block count, so the numbers right-align in the column.
    let bw = entries
        .iter()
        .map(|e| e.blocks)
        .max()
        .unwrap_or(0)
        .to_string()
        .len();
    let mut nodes = Vec::new();
    for e in &entries {
        let (splat, lock) = e.flags();
        let flags = format!("{splat}{lock}");
        let detail = if flags.trim().is_empty() {
            format!("{:>bw$} BLK", e.blocks)
        } else {
            format!("{:>bw$} BLK  {}", e.blocks, flags.trim())
        };
        nodes.push(Node {
            name: e.name.clone(),
            badge: e.file_type.mnemonic().to_string(),
            detail,
            blocks: e.blocks as u32,
            data: disk.read_entry(e).map_err(|x| x.to_string()),
        });
    }
    (disk_title(disk, header), nodes)
}

/// A disk's header line, as the listing's title shows it.
pub(crate) fn disk_title(disk: &Disk, header: sector64::Result<sector64::Header>) -> String {
    let free = disk
        .blocks_free()
        .map(|f| format!("{f} free"))
        .unwrap_or_default();
    match header {
        Ok(h) => format!("Disk  0 \"{}\" {} {}   {}", h.name, h.id, h.dos_type, free),
        Err(e) => format!("Disk (header error: {e})"),
    }
}

/// Copy the files at `rows` of `src` into `dest`'s disk or new archive, as
/// one step of its Undo. Returns the status line, which calls it `dest_name`.
pub(crate) fn copy_rows(src: &Pane, dest: &mut Pane, rows: &[usize], dest_name: &str) -> String {
    let before = dest.snapshot();
    // From a disk, copy directory entries (exact names, lock flags); from an
    // archive, the extracted bytes under the listed name.
    let src_disk = src.map_disk();
    let entries = src_disk.map(|d| d.directory().unwrap_or_default());
    let mut done = Vec::new();
    let mut failed = Vec::new();
    for &i in rows {
        let Some(node) = src.nodes.get(i) else {
            continue;
        };
        let r = if let Some(ed) = &mut dest.disk {
            match (src_disk, entries.as_ref().and_then(|e| e.get(i))) {
                (Some(d), Some(entry)) => {
                    ed.disk.copy_file_from(d, entry).map_err(|e| e.to_string())
                }
                _ => copy_bytes(&mut ed.disk, node),
            }
        } else if let Some(stage) = &mut dest.archive {
            node_file(node).map(|f| stage.push(f))
        } else {
            Err("nothing to copy into".to_string())
        };
        match r {
            Ok(()) => done.push(node.name.clone()),
            Err(e) => failed.push(format!("\"{}\": {e}", node.name)),
        }
    }
    if !done.is_empty() {
        if let Some(ed) = &mut dest.disk {
            ed.dirty = true;
        }
        dest.record(
            before,
            &format!("Copied {} file(s) from the other pane", done.len()),
        );
        dest.refresh();
    }
    copy_report(&done, &failed, dest_name)
}

/// Write an archive member (or any listed file without a directory entry)
/// onto `disk` under its listed name, refusing names already there and REL
/// files.
pub(crate) fn copy_bytes(disk: &mut Disk, node: &Node) -> Result<(), String> {
    let f = node_file(node)?;
    // A disk keeps 16 characters of the name, so that's what can clash.
    let name: String = f.name.chars().take(16).collect();
    if disk.find_entry(&name).ok().flatten().is_some() {
        return Err(sector64::Error::FileExists.to_string());
    }
    disk.write_file(&f.name, f.file_type, &f.data)
        .map_err(|e| e.to_string())
}

/// A listed file as a file to write elsewhere: its name, its type (from the
/// badge) and its bytes. REL files and files that couldn't be read fail.
pub(crate) fn node_file(node: &Node) -> Result<ImportFile, String> {
    let data = node.data.as_ref().map_err(|e| e.clone())?;
    let file_type = match node.badge.as_str() {
        "rel" => return Err(sector64::Error::CannotCopy("REL files").to_string()),
        "seq" => FileType::Seq,
        "usr" => FileType::Usr,
        "del" => FileType::Del,
        _ => FileType::Prg,
    };
    Ok(ImportFile {
        name: node.name.clone(),
        file_type,
        data: data.clone(),
    })
}

/// The status line after a copy: how many went to `dest`, and why any
/// didn't.
pub(crate) fn copy_report(done: &[String], failed: &[String], dest: &str) -> String {
    let mut s = match done {
        [] => "Nothing copied".to_string(),
        [one] => format!("Copied \"{one}\" to {dest}"),
        many => format!("Copied {} files to {dest}", many.len()),
    };
    if !failed.is_empty() {
        s.push_str(&format!("; skipped {}", failed.join(", ")));
    }
    s
}

/// A file name for saving `disk`, with the extension for its format.
pub(crate) fn default_disk_name(disk: &Disk) -> &'static str {
    use sector64::geometry::Geometry;
    match disk.geometry() {
        Geometry::Cbm1571 => "disk.d71",
        Geometry::Cbm1581 => "disk.d81",
        Geometry::CmdNative { .. } => "disk.d2m",
        Geometry::Cbm1541 { .. } => "disk.d64",
    }
}

/// If `path` names one part of a Zip-Code set (`1!NAME` to `4!NAME`), read
/// all four parts from the same folder and decode them into a D64 image.
/// Returns `None` when the name isn't a Zip-Code part.
pub(crate) fn zipcode_set(path: &Path) -> Option<Result<Vec<u8>, String>> {
    let names = sector64::listing::zipcode_parts(path)?;
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut parts = Vec::with_capacity(4);
    for name in names {
        let p = dir.join(name);
        match std::fs::read(&p) {
            Ok(b) => parts.push(b),
            Err(e) => {
                return Some(Err(format!(
                    "Zip-Code set incomplete: cannot read {}: {e}",
                    p.display()
                )))
            }
        }
    }
    Some(
        sector64::parse_zipcode([&parts[0], &parts[1], &parts[2], &parts[3]])
            .map_err(|e| format!("Zip-Code: {e}")),
    )
}

/// Open a path as an editable disk or a read-only listing.
pub(crate) fn open_path(path: &Path) -> Result<Opened, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    // A Zip-Code set (1!NAME … 4!NAME) opens from any of its four parts as a
    // read-only disk: saving must never write a D64 over a Zip-Code file.
    if let Some(d64) = zipcode_set(path) {
        let disk = Disk::load(d64?).map_err(|e| e.to_string())?;
        let (title, nodes) = disk_nodes(&disk);
        return Ok(Opened::ReadOnlyDisk(
            format!("{title}   (Zip-Code, read-only)"),
            nodes,
            disk,
        ));
    }

    if let Ok(disk) = Disk::load(bytes.clone()) {
        // The sector formats are editable, as is a CMD-native (DNP) D2M (its
        // header byte at 1/1 is 'H'); G64 and 1581-partition D2Ms are read-only.
        let dnp_d2m = ext == "d2m" && bytes.get(258) == Some(&b'H');
        if matches!(ext.as_str(), "d64" | "d71" | "d81") || dnp_d2m {
            return Ok(Opened::Disk(disk));
        }
        let (title, nodes) = disk_nodes(&disk);
        return Ok(Opened::ReadOnlyDisk(
            format!("{title}   (read-only)"),
            nodes,
            disk,
        ));
    }

    let archive = sector64::listing::read_archive(&ext, &bytes)
        .ok_or_else(|| format!("unrecognized file type (.{ext})"))?
        .map_err(|e| e.to_string())?;
    let mut details =
        packed_details(archive.files.iter().filter_map(|f| f.packed.clone())).into_iter();
    let nodes = archive
        .files
        .into_iter()
        .map(|f| Node {
            badge: f.kind.to_string(),
            // Stored files have no detail: their size is in the SIZE column.
            detail: match f.packed {
                Some(_) => details.next().unwrap_or_default(),
                None => String::new(),
            },
            name: f.name,
            blocks: 0,
            data: f.data.map_err(|e| e.to_string()),
        })
        .collect();
    Ok(Opened::List(archive.label.to_string(), nodes))
}

/// How many files File ▸ Open Recent remembers.
pub(crate) const RECENT_MAX: usize = 10;

/// Put `path` at the front of the recent-files list, dropping any older
/// entry for the same file and keeping at most `RECENT_MAX`.
pub(crate) fn remember_recent(list: &mut Vec<String>, path: &Path) {
    let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let full = full.display().to_string();
    list.retain(|p| *p != full);
    list.insert(0, full);
    list.truncate(RECENT_MAX);
}

/// Menu labels for the recent files: the file name, with its folder added
/// when two entries share a name. Returns `(label, full path)` pairs.
pub(crate) fn recent_labels(list: &[String]) -> Vec<(String, String)> {
    let name = |p: &str| {
        Path::new(p)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.to_string())
    };
    list.iter()
        .map(|p| {
            let n = name(p);
            let dup = list.iter().filter(|q| name(q) == n).count() > 1;
            let label = match Path::new(p).parent().and_then(|d| d.file_name()) {
                Some(dir) if dup => format!("{n}  ({})", dir.to_string_lossy()),
                _ => n,
            };
            (label, p.clone())
        })
        .collect()
}

/// A listing's readable rows as the files `convert_files` takes, and the
/// unreadable ones with why. Rows without a type the converter knows (LHA
/// and Wraptor members) become PRG files.
pub(crate) fn listed_files(
    nodes: &[Node],
) -> (Vec<sector64::listing::ListedFile>, Vec<(String, String)>) {
    let unreadable = nodes
        .iter()
        .filter_map(|n| n.data.as_ref().err().map(|e| (n.name.clone(), e.clone())))
        .collect();
    let files = nodes
        .iter()
        .filter(|n| n.data.is_ok())
        .map(|n| sector64::listing::ListedFile {
            name: n.name.clone(),
            kind: match n.badge.to_ascii_lowercase().as_str() {
                "seq" => "seq",
                "usr" => "usr",
                "del" => "del",
                "rel" => "rel",
                _ => "prg",
            },
            data: Ok(n.data.clone().unwrap_or_default()),
            packed: None,
        })
        .collect();
    (files, unreadable)
}

/// The status line after File > Convert To wrote `file`.
pub(crate) fn convert_report(file: &str, c: &sector64::convert::Converted) -> String {
    let n = c.copied.len();
    let mut s = format!(
        "Converted to {file}: {n} file{}",
        if n == 1 { "" } else { "s" }
    );
    let separators = c.notes.iter().filter(|n| n.1.contains("separator")).count();
    if separators > 0 {
        s += &format!(", {separators} separator line(s) left out");
    }
    if c.notes.iter().any(|n| n.1.contains("stored as PRG")) {
        s += "; SEQ and USR files are PRG in a T64 (LNX and LBR keep the type)";
    }
    if !c.skipped.is_empty() {
        let left: Vec<String> = c
            .skipped
            .iter()
            .map(|(name, why)| format!("\"{name}\" ({why})"))
            .collect();
        s += &format!("; cannot convert {}", left.join(", "));
    }
    s
}
