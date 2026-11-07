//! Sector64 GUI — a native browser and editor for Commodore disk images and
//! archives.
//!
//! Opens a disk image (`.d64/.d71/.d81/.g64/.d2m`) or an archive (`.arc/.sda/
//! .lha/.lzh/.sfx/.lbr/.t64/.lnx/.p00/.wr3`), lists the contents, and extracts
//! files. For the writable sector formats (`.d64/.d71/.d81`) it is also an
//! editor: create a blank image, add host files (button or drag-and-drop),
//! rename, delete, and save. G64/D2M/archives are read-only. The `sector64`
//! engine stays dependency-free; this front-end is a separate workspace member.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use eframe::egui;
use sector64::{
    parse_arc, parse_lbr, parse_lha, parse_lnx, parse_p00, parse_sda, parse_t64, parse_wr3,
    write_lbr, write_lnx, write_p00, write_t64, Disk, FileType, ImportFile,
};

/// One listed item: name, a short detail line, and its decoded bytes (or a
/// reason it can't be extracted).
struct Node {
    name: String,
    detail: String,
    data: Result<Vec<u8>, String>,
}

/// An open, editable disk and where it came from.
struct EditDisk {
    disk: Disk,
    path: Option<PathBuf>,
    dirty: bool,
}

#[derive(Default)]
struct App {
    // top bar
    path_input: String,
    // new-image controls
    new_fmt: usize, // 0=d64, 1=d71, 2=d81
    new_name: String,
    new_id: String,
    // editing
    rename_to: String,
    add_type: usize, // 0=prg,1=seq,2=usr
    // bottom bar
    out_dir: String,
    status: String,
    // state
    title: String,
    nodes: Vec<Node>,
    selected: Option<usize>,
    disk: Option<EditDisk>,
    // staging list when building a new archive
    archive: Option<Vec<ImportFile>>,
    // preview/validation pane
    hex_cache: Option<(usize, String)>,
    validation: String,
}

/// Actions collected from the UI and applied after the frame (keeps egui
/// closures free of `&mut self` conflicts).
enum Act {
    OpenDialog,
    Load,
    Select(usize),
    PickOutDir,
    ExtractSelected,
    ExtractAll,
    Create,
    AddFile,
    AddBytes(String, Vec<u8>),
    Rename,
    Delete,
    Save,
    SaveAs,
    Validate,
    NewArchive,
    SaveArchiveAs,
}

impl App {
    fn new(initial: Option<String>) -> Self {
        let mut app = App {
            new_name: "blank".into(),
            new_id: "01".into(),
            ..Default::default()
        };
        if let Some(p) = initial {
            app.open(Path::new(&p));
        }
        app
    }

    /// Whether add/rename/delete apply (an editable disk or a new archive).
    fn editable(&self) -> bool {
        self.disk.is_some() || self.archive.is_some()
    }

    fn open(&mut self, path: &Path) {
        self.nodes.clear();
        self.selected = None;
        self.disk = None;
        self.archive = None;
        self.title.clear();
        self.hex_cache = None;
        self.validation.clear();
        match open_path(path) {
            Ok(Opened::Disk(disk)) => {
                self.disk = Some(EditDisk { disk, path: Some(path.to_path_buf()), dirty: false });
                self.refresh();
                self.after_open(path);
            }
            Ok(Opened::List(title, nodes)) => {
                self.title = title;
                self.nodes = nodes;
                self.after_open(path);
            }
            Err(e) => self.status = format!("Error: {e}"),
        }
    }

    fn after_open(&mut self, path: &Path) {
        self.path_input = path.display().to_string();
        if self.out_dir.is_empty() {
            if let Some(parent) = path.parent() {
                self.out_dir = parent.display().to_string();
            }
        }
        self.status = format!("Opened {} ({} item(s))", path.display(), self.nodes.len());
    }

    /// Rebuild the listing from the editable disk or the archive staging list.
    fn refresh(&mut self) {
        if let Some(ed) = &self.disk {
            let (title, nodes) = disk_nodes(&ed.disk);
            let dirty = if ed.dirty { "  •modified" } else { "" };
            self.title = format!("{title}{dirty}");
            self.nodes = nodes;
        } else if let Some(stage) = &self.archive {
            self.title = format!("New archive — {} file(s) (unsaved)", stage.len());
            self.nodes = stage
                .iter()
                .map(|f| Node {
                    name: f.name.clone(),
                    detail: format!("{} bytes  {}", f.data.len(), f.file_type.mnemonic()),
                    data: Ok(f.data.clone()),
                })
                .collect();
        }
        if let Some(i) = self.selected {
            if i >= self.nodes.len() {
                self.selected = None;
            }
        }
    }

    fn apply(&mut self, act: Act) {
        match act {
            Act::OpenDialog => {
                let dlg = rfd::FileDialog::new()
                    .add_filter(
                        "Commodore disks & archives",
                        &[
                            "d64", "d71", "d81", "g64", "d2m", "arc", "sda", "lha", "lzh", "sfx",
                            "lbr", "t64", "lnx", "wr3", "p00",
                        ],
                    )
                    .add_filter("All files", &["*"]);
                if let Some(p) = dlg.pick_file() {
                    self.open(&p);
                }
            }
            Act::Load => {
                let p = PathBuf::from(self.path_input.trim());
                if !self.path_input.trim().is_empty() {
                    self.open(&p);
                }
            }
            Act::Select(i) => self.selected = Some(i),
            Act::PickOutDir => {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.out_dir = d.display().to_string();
                }
            }
            Act::ExtractSelected => self.extract_selected(),
            Act::ExtractAll => self.extract_all(),
            Act::Create => {
                let disk = match self.new_fmt {
                    1 => Disk::format_d71(&self.new_name, &self.new_id),
                    2 => Disk::format_d81(&self.new_name, &self.new_id),
                    _ => Disk::format(&self.new_name, &self.new_id),
                };
                self.disk = Some(EditDisk { disk, path: None, dirty: true });
                self.selected = None;
                self.refresh();
                self.status = "Created a blank image (unsaved)".into();
            }
            Act::AddFile => {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    match std::fs::read(&p) {
                        Ok(bytes) => {
                            let name = cbm_name_from(&p);
                            self.apply(Act::AddBytes(name, bytes));
                        }
                        Err(e) => self.status = format!("read failed: {e}"),
                    }
                }
            }
            Act::AddBytes(name, bytes) => self.add(name, &bytes),
            Act::Rename => self.rename(),
            Act::Delete => self.delete(),
            Act::Save => self.save(false),
            Act::SaveAs => self.save(true),
            Act::Validate => self.validate(),
            Act::NewArchive => {
                self.disk = None;
                self.archive = Some(Vec::new());
                self.selected = None;
                self.hex_cache = None;
                self.refresh();
                self.status = "New archive — add files, then Save archive as…".into();
            }
            Act::SaveArchiveAs => self.save_archive(),
        }
    }

    fn save_archive(&mut self) {
        let Some(stage) = &self.archive else { return };
        if stage.is_empty() {
            self.status = "Add at least one file first".into();
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("archive", &["t64", "lnx", "lbr", "p00"])
            .set_file_name("archive.t64")
            .save_file()
        else {
            return;
        };
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        let bytes = match ext.as_str() {
            "t64" => write_t64(stage),
            "lnx" => write_lnx(stage),
            "lbr" => write_lbr(stage),
            "p00" => {
                if stage.len() != 1 {
                    self.status = "A .p00 holds exactly one file".into();
                    return;
                }
                write_p00(&stage[0])
            }
            other => {
                self.status = format!("unknown archive type .{other} (use t64/lnx/lbr/p00)");
                return;
            }
        };
        match std::fs::write(&path, &bytes) {
            Ok(()) => self.status = format!("Saved {} ({} bytes)", path.display(), bytes.len()),
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn validate(&mut self) {
        let Some(ed) = &self.disk else {
            self.validation = "Open a D64/D71/D81 to validate".into();
            return;
        };
        self.validation = match ed.disk.validate() {
            Ok(r) => format!(
                "blocks: {} used, {} free\ncross-linked: {}\nused-but-free: {}\n{}",
                r.blocks_used,
                r.blocks_free,
                r.cross_linked.len(),
                r.used_but_free.len(),
                if r.cross_linked.is_empty() && r.used_but_free.is_empty() {
                    "✓ consistent with the BAM"
                } else {
                    "⚠ inconsistencies found"
                },
            ),
            Err(e) => format!("validate: {e}"),
        };
    }

    fn add(&mut self, name: String, bytes: &[u8]) {
        let ft = match self.add_type {
            1 => FileType::Seq,
            2 => FileType::Usr,
            _ => FileType::Prg,
        };
        if let Some(ed) = &mut self.disk {
            match ed.disk.write_file(&name, ft, bytes) {
                Ok(()) => {
                    ed.dirty = true;
                    self.refresh();
                    self.status = format!("Added \"{name}\" ({} bytes)", bytes.len());
                }
                Err(e) => self.status = format!("Add failed: {e}"),
            }
        } else if let Some(stage) = &mut self.archive {
            stage.push(ImportFile { name: name.clone(), file_type: ft, data: bytes.to_vec() });
            self.refresh();
            self.status = format!("Added \"{name}\" ({} bytes)", bytes.len());
        }
    }

    fn rename(&mut self) {
        let Some(i) = self.selected else {
            self.status = "Select a file to rename".into();
            return;
        };
        let old = self.nodes[i].name.clone();
        let new = self.rename_to.trim().to_string();
        if new.is_empty() {
            self.status = "Enter a new name".into();
            return;
        }
        if let Some(ed) = &mut self.disk {
            match ed.disk.rename(&old, &new) {
                Ok(()) => {
                    ed.dirty = true;
                    self.refresh();
                    self.status = format!("Renamed \"{old}\" → \"{new}\"");
                }
                Err(e) => self.status = format!("Rename failed: {e}"),
            }
        } else if let Some(stage) = &mut self.archive {
            stage[i].name = new.clone();
            self.refresh();
            self.status = format!("Renamed \"{old}\" → \"{new}\"");
        }
    }

    fn delete(&mut self) {
        let Some(i) = self.selected else {
            self.status = "Select a file to delete".into();
            return;
        };
        let name = self.nodes[i].name.clone();
        if let Some(ed) = &mut self.disk {
            match ed.disk.scratch(&name) {
                Ok(n) => {
                    ed.dirty = true;
                    self.selected = None;
                    self.refresh();
                    self.status = format!("Deleted \"{name}\" ({n} blocks freed)");
                }
                Err(e) => self.status = format!("Delete failed: {e}"),
            }
        } else if let Some(stage) = &mut self.archive {
            stage.remove(i);
            self.selected = None;
            self.refresh();
            self.status = format!("Removed \"{name}\"");
        }
    }

    fn save(&mut self, save_as: bool) {
        let Some(ed) = &mut self.disk else {
            self.status = "Nothing to save".into();
            return;
        };
        let path = if save_as || ed.path.is_none() {
            let dlg = rfd::FileDialog::new().add_filter("disk image", &["d64", "d71", "d81", "d2m"]);
            let dlg = if let Some(p) = &ed.path {
                dlg.set_directory(p.parent().unwrap_or(Path::new("."))).set_file_name(
                    p.file_name().and_then(|n| n.to_str()).unwrap_or("disk.d64"),
                )
            } else {
                dlg.set_file_name("disk.d64")
            };
            match dlg.save_file() {
                Some(p) => p,
                None => return,
            }
        } else {
            ed.path.clone().unwrap()
        };
        match std::fs::write(&path, ed.disk.as_bytes()) {
            Ok(()) => {
                ed.path = Some(path.clone());
                ed.dirty = false;
                self.status = format!("Saved {}", path.display());
                self.refresh();
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn extract_selected(&mut self) {
        let Some(i) = self.selected else {
            self.status = "Select a file first".into();
            return;
        };
        let (name, data) = (self.nodes[i].name.clone(), self.nodes[i].data.clone());
        let data = match data {
            Ok(d) => d,
            Err(e) => {
                self.status = format!("Cannot extract \"{name}\": {e}");
                return;
            }
        };
        let dir = if self.out_dir.is_empty() { ".".into() } else { self.out_dir.clone() };
        let dest = PathBuf::from(&dir).join(sanitize(&name));
        match std::fs::write(&dest, &data) {
            Ok(()) => self.status = format!("Wrote {} ({} bytes)", dest.display(), data.len()),
            Err(e) => self.status = format!("Write failed: {e}"),
        }
    }

    fn extract_all(&mut self) {
        let dir = if self.out_dir.is_empty() { ".".into() } else { self.out_dir.clone() };
        let (mut ok, mut skipped) = (0, 0);
        for node in &self.nodes {
            match &node.data {
                Ok(d) => {
                    let dest = PathBuf::from(&dir).join(sanitize(&node.name));
                    if std::fs::write(&dest, d).is_ok() {
                        ok += 1;
                    } else {
                        skipped += 1;
                    }
                }
                Err(_) => skipped += 1,
            }
        }
        self.status = format!("Extracted {ok} file(s) to {dir}, {skipped} skipped");
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut acts: Vec<Act> = Vec::new();
        let editable = self.editable();
        let is_disk = self.disk.is_some();
        let is_archive = self.archive.is_some();

        // Refresh the hex preview cache when the selection changes (done before
        // the panels so the closures only read `self` immutably).
        match self.selected {
            Some(i) if i < self.nodes.len() => {
                let stale = self.hex_cache.as_ref().map(|(ci, _)| *ci != i).unwrap_or(true);
                if stale {
                    let text = match &self.nodes[i].data {
                        Ok(d) => hexdump(d, 16 * 1024),
                        Err(e) => format!("(not extractable: {e})"),
                    };
                    self.hex_cache = Some((i, text));
                }
            }
            _ => self.hex_cache = None,
        }
        let preview = self.hex_cache.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
        let preview_name = self.selected.and_then(|i| self.nodes.get(i)).map(|n| n.name.clone());
        let validation = self.validation.clone();

        // Local copies of edited strings (write back after the frame).
        let mut path_input = std::mem::take(&mut self.path_input);
        let mut out_dir = std::mem::take(&mut self.out_dir);
        let mut new_name = std::mem::take(&mut self.new_name);
        let mut new_id = std::mem::take(&mut self.new_id);
        let mut rename_to = std::mem::take(&mut self.rename_to);
        let mut new_fmt = self.new_fmt;
        let mut add_type = self.add_type;

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📂 Open…").clicked() {
                    acts.push(Act::OpenDialog);
                }
                ui.add(
                    egui::TextEdit::singleline(&mut path_input)
                        .hint_text("path to a disk image or archive")
                        .desired_width(360.0),
                );
                if ui.button("Load").clicked() {
                    acts.push(Act::Load);
                }
            });
            ui.horizontal(|ui| {
                ui.label("New image:");
                egui::ComboBox::from_id_salt("fmt")
                    .selected_text(["D64", "D71", "D81"][new_fmt.min(2)])
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut new_fmt, 0, "D64");
                        ui.selectable_value(&mut new_fmt, 1, "D71");
                        ui.selectable_value(&mut new_fmt, 2, "D81");
                    });
                ui.add(egui::TextEdit::singleline(&mut new_name).hint_text("name").desired_width(130.0));
                ui.add(egui::TextEdit::singleline(&mut new_id).hint_text("id").desired_width(40.0));
                if ui.button("Create").clicked() {
                    acts.push(Act::Create);
                }
                ui.separator();
                if ui.button("New archive").clicked() {
                    acts.push(Act::NewArchive);
                }
            });
            if !self.title.is_empty() {
                ui.label(egui::RichText::new(&self.title).strong());
            }
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Output:");
                ui.add(egui::TextEdit::singleline(&mut out_dir).desired_width(230.0));
                if ui.button("…").clicked() {
                    acts.push(Act::PickOutDir);
                }
                if ui.button("Extract selected").clicked() {
                    acts.push(Act::ExtractSelected);
                }
                if ui.button("Extract all").clicked() {
                    acts.push(Act::ExtractAll);
                }
            });
            if editable {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("addtype")
                        .selected_text(["prg", "seq", "usr"][add_type.min(2)])
                        .width(50.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut add_type, 0, "prg");
                            ui.selectable_value(&mut add_type, 1, "seq");
                            ui.selectable_value(&mut add_type, 2, "usr");
                        });
                    if ui.button("Add file…").clicked() {
                        acts.push(Act::AddFile);
                    }
                    ui.separator();
                    ui.add(
                        egui::TextEdit::singleline(&mut rename_to)
                            .hint_text("new name")
                            .desired_width(130.0),
                    );
                    if ui.button("Rename").clicked() {
                        acts.push(Act::Rename);
                    }
                    if ui.button(if is_archive { "Remove" } else { "Delete" }).clicked() {
                        acts.push(Act::Delete);
                    }
                    ui.separator();
                    if is_disk {
                        if ui.button("Save").clicked() {
                            acts.push(Act::Save);
                        }
                        if ui.button("Save As…").clicked() {
                            acts.push(Act::SaveAs);
                        }
                    }
                    if is_archive && ui.button("Save archive as…").clicked() {
                        acts.push(Act::SaveArchiveAs);
                    }
                });
            }
            ui.label(&self.status);
        });

        egui::SidePanel::right("preview")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                if is_disk {
                    ui.horizontal(|ui| {
                        if ui.button("Validate disk").clicked() {
                            acts.push(Act::Validate);
                        }
                    });
                    if !validation.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(&validation).monospace()).wrap());
                    }
                    ui.separator();
                }
                match &preview_name {
                    Some(n) => ui.label(egui::RichText::new(format!("Preview: {n}")).strong()),
                    None => ui.label(egui::RichText::new("Preview").strong()),
                };
                egui::ScrollArea::vertical().id_salt("preview_scroll").show(ui, |ui| {
                    if preview.is_empty() {
                        ui.weak("Select a file to preview its bytes.");
                    } else {
                        ui.add(egui::Label::new(egui::RichText::new(&preview).monospace()).wrap());
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.nodes.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a disk/archive, or create a new image and add files.");
                });
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("files").striped(true).num_columns(3).show(ui, |ui| {
                    ui.label(egui::RichText::new("name").strong());
                    ui.label(egui::RichText::new("details").strong());
                    ui.label(egui::RichText::new("status").strong());
                    ui.end_row();
                    for (i, node) in self.nodes.iter().enumerate() {
                        let sel = self.selected == Some(i);
                        let resp = ui
                            .selectable_label(sel, node.name.as_str())
                            .on_hover_text("double-click to extract");
                        if resp.clicked() {
                            acts.push(Act::Select(i));
                        }
                        if resp.double_clicked() {
                            acts.push(Act::Select(i));
                            acts.push(Act::ExtractSelected);
                        }
                        ui.label(&node.detail);
                        match &node.data {
                            Ok(d) => ui.label(format!("{} bytes", d.len())),
                            Err(e) => ui.label(egui::RichText::new(e).weak()),
                        };
                        ui.end_row();
                    }
                });
            });
        });

        // Drag-and-drop a host file onto the window to add it (editable disks).
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect()
        });
        if editable {
            for p in dropped {
                if let Ok(bytes) = std::fs::read(&p) {
                    acts.push(Act::AddBytes(cbm_name_from(&p), bytes));
                }
            }
        }

        // Write edited strings back, then apply actions.
        self.path_input = path_input;
        self.out_dir = out_dir;
        self.new_name = new_name;
        self.new_id = new_id;
        self.rename_to = rename_to;
        self.new_fmt = new_fmt;
        self.add_type = add_type;
        for act in acts {
            self.apply(act);
        }
    }
}

/// A classic hex + ASCII dump of up to `cap` bytes, 16 per line.
fn hexdump(data: &[u8], cap: usize) -> String {
    let n = data.len().min(cap);
    let mut s = String::with_capacity(n * 4 + 64);
    for (off, chunk) in data[..n].chunks(16).enumerate() {
        s.push_str(&format!("{:04X}: ", off * 16));
        for i in 0..16 {
            if i < chunk.len() {
                s.push_str(&format!("{:02X} ", chunk[i]));
            } else {
                s.push_str("   ");
            }
            if i == 7 {
                s.push(' ');
            }
        }
        s.push(' ');
        for &b in chunk {
            s.push(if (0x20..=0x7E).contains(&b) { b as char } else { '.' });
        }
        s.push('\n');
    }
    if data.len() > n {
        s.push_str(&format!("… ({} of {} bytes shown)\n", n, data.len()));
    }
    s
}

/// A CBM file name derived from a host path: the stem, uppercased, ≤16 chars.
fn cbm_name_from(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    stem.to_uppercase().chars().take(16).collect()
}

/// Replace characters unsafe in a host file name.
fn sanitize(name: &str) -> String {
    let n: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || " .-_".contains(c) { c } else { '_' })
        .collect();
    let n = n.trim();
    if n.is_empty() {
        "file".into()
    } else {
        n.to_string()
    }
}

enum Opened {
    Disk(Disk),
    List(String, Vec<Node>),
}

/// Build the listing (title, items) from a disk.
fn disk_nodes(disk: &Disk) -> (String, Vec<Node>) {
    let header = disk.header();
    let entries = disk.directory().unwrap_or_default();
    let mut nodes = Vec::new();
    for e in &entries {
        let (splat, lock) = e.flags();
        nodes.push(Node {
            name: e.name.clone(),
            detail: format!("{} blk  {}{}{}", e.blocks, splat, e.file_type.mnemonic(), lock),
            data: disk.read_entry(e).map_err(|x| x.to_string()),
        });
    }
    let free = disk.blocks_free().map(|f| format!("{f} free")).unwrap_or_default();
    let title = match header {
        Ok(h) => format!("Disk  0 \"{}\" {} {}   {}", h.name, h.id, h.dos_type, free),
        Err(e) => format!("Disk (header error: {e})"),
    };
    (title, nodes)
}

/// Open a path as an editable disk or a read-only listing.
fn open_path(path: &Path) -> Result<Opened, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    if let Ok(disk) = Disk::load(bytes.clone()) {
        // The sector formats are editable, as is a CMD-native (DNP) D2M (its
        // header byte at 1/1 is 'H'); G64 and 1581-partition D2Ms are read-only.
        let dnp_d2m = ext == "d2m" && bytes.get(258) == Some(&b'H');
        if matches!(ext.as_str(), "d64" | "d71" | "d81") || dnp_d2m {
            return Ok(Opened::Disk(disk));
        }
        let (title, nodes) = disk_nodes(&disk);
        return Ok(Opened::List(format!("{title}   (read-only)"), nodes));
    }

    let import_nodes = |files: Vec<sector64::ImportFile>| {
        files
            .into_iter()
            .map(|f| Node {
                detail: format!("{} bytes  {}", f.data.len(), f.file_type.mnemonic()),
                name: f.name,
                data: Ok(f.data),
            })
            .collect::<Vec<_>>()
    };
    let is_p00 = {
        let b = ext.as_bytes();
        b.len() == 3
            && matches!(b[0], b'p' | b's' | b'u' | b'r' | b'd')
            && b[1].is_ascii_digit()
            && b[2].is_ascii_digit()
    };

    let (kind, nodes) = match ext.as_str() {
        "arc" | "sda" => {
            let parse = if bytes.starts_with(&[0x01, 0x08]) { parse_sda } else { parse_arc };
            let nodes = parse(&bytes)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|e| Node {
                    detail: format!("{} bytes  {}", e.orig_size, e.method.label()),
                    data: e.decode().map_err(|x| x.to_string()),
                    name: e.name,
                })
                .collect();
            ("ARC/SDA archive", nodes)
        }
        "lha" | "lzh" | "sfx" => {
            let nodes = parse_lha(&bytes)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|e| Node {
                    detail: format!("{} → {} bytes  {}", e.compressed_size, e.original_size, e.method_str()),
                    data: e.decode().map_err(|x| x.to_string()),
                    name: e.name,
                })
                .collect();
            ("LHA/SFX archive", nodes)
        }
        "lbr" => ("LBR library", import_nodes(parse_lbr(&bytes).map_err(|e| e.to_string())?)),
        "t64" => ("T64 tape archive", import_nodes(parse_t64(&bytes).map_err(|e| e.to_string())?)),
        "lnx" => ("Lynx archive", import_nodes(parse_lnx(&bytes).map_err(|e| e.to_string())?)),
        "wr3" => {
            let nodes = parse_wr3(&bytes)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|e| Node {
                    detail: format!("{} bytes packed", e.compressed.len()),
                    data: Err("Wraptor compression not decoded".to_string()),
                    name: e.name,
                })
                .collect();
            ("Wraptor-3 archive", nodes)
        }
        _ if is_p00 => ("PC64 container", import_nodes(vec![parse_p00(&bytes).map_err(|e| e.to_string())?])),
        other => return Err(format!("unrecognized file type (.{other})")),
    };
    Ok(Opened::List(kind.to_string(), nodes))
}

/// A small procedurally-drawn 3.5" floppy icon (48×48 RGBA) for the window.
fn make_icon() -> egui::IconData {
    let (w, h) = (48usize, 48usize);
    let mut rgba = vec![0u8; w * h * 4];
    let mut fill = |x0: usize, y0: usize, x1: usize, y1: usize, c: [u8; 4]| {
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                let i = (y * w + x) * 4;
                rgba[i..i + 4].copy_from_slice(&c);
            }
        }
    };
    let body = [0x2b, 0x4b, 0x7a, 0xff];
    let shutter = [0xb8, 0xbe, 0xc8, 0xff];
    let label = [0xef, 0xef, 0xe6, 0xff];
    fill(5, 5, 43, 43, body); // disk body
    fill(24, 7, 40, 19, shutter); // metal shutter
    fill(30, 9, 34, 17, body); // shutter window
    fill(10, 24, 38, 40, label); // write-on label
    // clip the top-right corner diagonally
    for y in 5..14 {
        for x in 34..43 {
            if (x - 34) + (43 - y) > 9 {
                let i = (y * w + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    egui::IconData { rgba, width: w as u32, height: h as u32 }
}

fn main() -> eframe::Result<()> {
    let initial = std::env::args().nth(1);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([820.0, 560.0])
            .with_icon(std::sync::Arc::new(make_icon())),
        ..Default::default()
    };
    eframe::run_native(
        "Sector64",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(initial)))),
    )
}
