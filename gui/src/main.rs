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
use egui::Color32;
use sector64::{
    parse_arc, parse_lbr, parse_lha, parse_lnx, parse_p00, parse_sda, parse_t64, write_lbr,
    write_lnx, write_p00, write_t64, Disk, FileType, ImportFile,
};

/// The Commodore 64 palette this UI is themed around (Pepto values).
mod c64 {
    use eframe::egui::Color32;
    /// Dark-blue screen background (colour 6).
    pub const BG: Color32 = Color32::from_rgb(0x35, 0x28, 0x79);
    /// A slightly darker blue for framing panels.
    pub const BG_PANEL: Color32 = Color32::from_rgb(0x2a, 0x20, 0x60);
    /// The darkest blue, for text-input wells.
    pub const WELL: Color32 = Color32::from_rgb(0x1e, 0x17, 0x48);
    /// Faint stripe on alternate rows.
    pub const STRIPE: Color32 = Color32::from_rgb(0x3d, 0x2f, 0x87);
    /// Light-blue accent (colour 14) — the classic border/cursor colour.
    pub const ACCENT: Color32 = Color32::from_rgb(0x7c, 0x6f, 0xd0);
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x55, 0x4a, 0x9c);
    /// Bright light-blue text, lifted from the true value for legibility.
    pub const TEXT: Color32 = Color32::from_rgb(0xc9, 0xc2, 0xf0);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8e, 0x82, 0xc8);
    /// Near-white for emphasis (colour 1).
    pub const BRIGHT: Color32 = Color32::from_rgb(0xf2, 0xf0, 0xff);
    // File-type badge colours.
    pub const PRG: Color32 = Color32::from_rgb(0x8c, 0xc0, 0x70);
    pub const SEQ: Color32 = Color32::from_rgb(0x78, 0xc0, 0xc0);
    pub const USR: Color32 = Color32::from_rgb(0xb0, 0x80, 0xd0);
    pub const DEL: Color32 = Color32::from_rgb(0xa8, 0xa8, 0xa8);
    pub const REL: Color32 = Color32::from_rgb(0xc8, 0xc0, 0x60);
    pub const WARN: Color32 = Color32::from_rgb(0xd8, 0x80, 0x70);
    pub const OK: Color32 = Color32::from_rgb(0x8c, 0xc0, 0x70);
}

/// Install the "C64 Pro Mono" PETSCII font as the UI typeface when it is
/// present on the system. The font is loaded from the user's installed copy at
/// runtime (not bundled), so when it is absent the app keeps the built-in
/// monospace. Returns true if the PETSCII font was installed.
fn load_c64_font(ctx: &egui::Context) -> bool {
    let Some(bytes) = find_c64_font() else {
        return false;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("c64".to_owned(), egui::FontData::from_owned(bytes));
    for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "c64".to_owned());
    }
    ctx.set_fonts(fonts);
    true
}

/// Locate the "C64 Pro Mono" TrueType file, via fontconfig first and then a few
/// common font directories. Only a path that actually names a C64 font is used
/// (fontconfig falls back to a default otherwise).
fn find_c64_font() -> Option<Vec<u8>> {
    if let Ok(out) = std::process::Command::new("fc-match")
        .args(["--format=%{file}", "C64 Pro Mono"])
        .output()
    {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if p.to_lowercase().contains("c64") {
                if let Ok(b) = std::fs::read(&p) {
                    return Some(b);
                }
            }
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs = [
        format!("{home}/.local/share/fonts"),
        format!("{home}/.fonts"),
        "/usr/share/fonts".into(),
        "/usr/local/share/fonts".into(),
    ];
    let names = [
        "C64_Pro_Mono-STYLE.ttf",
        "C64 Pro Mono.ttf",
        "C64_Pro_Mono.ttf",
    ];
    for dir in dirs {
        for name in names {
            if let Ok(b) = std::fs::read(format!("{dir}/{name}")) {
                return Some(b);
            }
        }
    }
    None
}

/// Apply the retro C64 theme: monospace throughout, dark-blue panels,
/// light-blue text, square (un-rounded) widgets, a light-blue selection.
fn apply_c64_theme(ctx: &egui::Context) {
    use egui::{FontFamily::Monospace, FontId, Rounding, Stroke, TextStyle};
    let mut style = (*ctx.style()).clone();

    // Sizes are multiples of 8 so the C64 Pro pixel grid renders crisp (2×/3×).
    style.text_styles = [
        (TextStyle::Heading, FontId::new(24.0, Monospace)),
        (TextStyle::Body, FontId::new(16.0, Monospace)),
        (TextStyle::Monospace, FontId::new(16.0, Monospace)),
        (TextStyle::Button, FontId::new(16.0, Monospace)),
        (TextStyle::Small, FontId::new(16.0, Monospace)),
    ]
    .into();

    let mut v = egui::Visuals::dark();
    v.panel_fill = c64::BG;
    v.window_fill = c64::BG_PANEL;
    v.extreme_bg_color = c64::WELL;
    v.faint_bg_color = c64::STRIPE;
    v.override_text_color = Some(c64::TEXT);
    v.hyperlink_color = c64::ACCENT;
    v.selection.bg_fill = c64::ACCENT;
    v.selection.stroke = Stroke::new(1.0, c64::BRIGHT);
    v.window_rounding = Rounding::ZERO;
    v.menu_rounding = Rounding::ZERO;

    let square = Rounding::ZERO;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.rounding = square;
    }
    v.widgets.noninteractive.bg_fill = c64::BG_PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c64::ACCENT_DIM);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c64::TEXT);
    v.widgets.inactive.bg_fill = c64::BG_PANEL;
    v.widgets.inactive.weak_bg_fill = c64::BG_PANEL;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, c64::ACCENT_DIM);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, c64::TEXT);
    v.widgets.hovered.bg_fill = c64::ACCENT_DIM;
    v.widgets.hovered.weak_bg_fill = c64::ACCENT_DIM;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, c64::ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, c64::BRIGHT);
    v.widgets.active.bg_fill = c64::ACCENT;
    v.widgets.active.weak_bg_fill = c64::ACCENT;
    v.widgets.active.bg_stroke = Stroke::new(1.0, c64::BRIGHT);
    v.widgets.active.fg_stroke = Stroke::new(1.0, c64::BG);

    style.visuals = v;
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    ctx.set_style(style);
}

/// 8×8 pixel-art button icons (`#` = ink, anything else = transparent),
/// doubled to 16×16 so they render as chunky C64-style pixels beside 16px text.
mod art {
    pub const FOLDER: [&str; 8] = [
        "........", ".###....", "######..", "#.....#.", "#.....#.", "#.....#.", "########",
        "........",
    ];
    pub const PLUS: [&str; 8] = [
        "........", "...##...", "...##...", "########", "########", "...##...", "...##...",
        "........",
    ];
    pub const FLOPPY: [&str; 8] = [
        "########", "#..##..#", "#..##..#", "#......#", "#.####.#", "#.#..#.#", "#.####.#",
        "########",
    ];
    pub const DOWN: [&str; 8] = [
        "...##...", "...##...", "...##...", "...##...", ".######.", "..####..", "...##...",
        "........",
    ];
    pub const TRASH: [&str; 8] = [
        "..####..", "########", "........", ".######.", ".#.##.#.", ".#.##.#.", ".######.",
        "..####..",
    ];
    pub const PENCIL: [&str; 8] = [
        "......##", ".....###", "....###.", "...###..", "..###...", ".###....", "###.....",
        "#.......",
    ];
    pub const DOC: [&str; 8] = [
        "######..", "#....#..", "#.##.#..", "#....#..", "#.##.#..", "#....#..", "#.##.#..",
        "######..",
    ];
}

/// Button-icon textures, built once from the pixel art.
#[derive(Clone)]
struct Icons {
    folder: egui::TextureHandle,
    plus: egui::TextureHandle,
    floppy: egui::TextureHandle,
    down: egui::TextureHandle,
    trash: egui::TextureHandle,
    pencil: egui::TextureHandle,
    doc: egui::TextureHandle,
}

impl Icons {
    fn load(ctx: &egui::Context) -> Self {
        let mk = |name: &str, a: &[&str; 8]| {
            ctx.load_texture(
                name,
                pixel_image(a, c64::BRIGHT),
                egui::TextureOptions::NEAREST,
            )
        };
        Icons {
            folder: mk("i_folder", &art::FOLDER),
            plus: mk("i_plus", &art::PLUS),
            floppy: mk("i_floppy", &art::FLOPPY),
            down: mk("i_down", &art::DOWN),
            trash: mk("i_trash", &art::TRASH),
            pencil: mk("i_pencil", &art::PENCIL),
            doc: mk("i_doc", &art::DOC),
        }
    }
}

/// Turn an 8×8 pixel-art grid into a 16×16 `ColorImage` (each cell a 2×2 block).
fn pixel_image(grid: &[&str; 8], ink: Color32) -> egui::ColorImage {
    let mut pixels = vec![Color32::TRANSPARENT; 16 * 16];
    for (y, row) in grid.iter().enumerate() {
        for (x, ch) in row.chars().enumerate().take(8) {
            if ch == '#' {
                for dy in 0..2 {
                    for dx in 0..2 {
                        pixels[(y * 2 + dy) * 16 + (x * 2 + dx)] = ink;
                    }
                }
            }
        }
    }
    egui::ColorImage {
        size: [16, 16],
        pixels,
    }
}

/// A 16×16-icon button carrying `label` text.
fn icon_button<'a>(tex: &egui::TextureHandle, label: &'a str) -> egui::Button<'a> {
    let img = egui::Image::new(egui::load::SizedTexture::new(
        tex.id(),
        egui::vec2(16.0, 16.0),
    ));
    egui::Button::image_and_text(img, label)
}

/// The accent colour for a file-type mnemonic badge.
fn badge_color(mnemonic: &str) -> Color32 {
    match mnemonic {
        "prg" => c64::PRG,
        "seq" => c64::SEQ,
        "usr" => c64::USR,
        "del" => c64::DEL,
        "rel" => c64::REL,
        _ => c64::TEXT_DIM,
    }
}

/// One listed item: name, a short type badge, a detail line, and its decoded
/// bytes (or a reason it can't be extracted).
struct Node {
    name: String,
    /// Short type mnemonic for the colour badge (e.g. "prg"); empty if none.
    badge: String,
    detail: String,
    /// Block count for sorting (disk entries); 0 when not applicable.
    blocks: u32,
    data: Result<Vec<u8>, String>,
}

/// An open, editable disk and where it came from.
struct EditDisk {
    disk: Disk,
    path: Option<PathBuf>,
    dirty: bool,
}

/// Which column the listing is sorted by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Type,
    Name,
    Blocks,
    Size,
}

/// A pending action blocked by unsaved changes.
enum Confirm {
    /// Open this file once the user discards changes.
    OpenFile(PathBuf),
    /// Quit once the user discards changes.
    Quit,
}

#[derive(Default)]
struct App {
    // new-image controls
    new_fmt: usize, // 0=d64, 1=d71, 2=d81
    new_name: String,
    new_id: String,
    // editing
    rename_to: String,
    add_type: usize, // 0=prg,1=seq,2=usr
    // inline rename: index of the row whose name is being edited, and whether
    // the edit field should grab focus this frame
    renaming: Option<usize>,
    rename_focus: bool,
    // bottom bar
    out_dir: String,
    status: String,
    /// Path of the currently-open file (shown on the "Open:" line).
    opened_path: Option<String>,
    // state
    title: String,
    nodes: Vec<Node>,
    selected: Option<usize>,
    /// Extra rows marked for a multi-file extract (Ctrl/Shift-click).
    marked: std::collections::BTreeSet<usize>,
    /// Column sort: `(key, ascending)`; `None` is directory order.
    sort: Option<(SortKey, bool)>,
    /// Case-insensitive name filter; empty shows everything.
    filter: String,
    /// A pending confirmation (unsaved changes before open/quit).
    confirm: Option<Confirm>,
    disk: Option<EditDisk>,
    // staging list when building a new archive
    archive: Option<Vec<ImportFile>>,
    // preview/validation pane
    hex_cache: Option<(usize, String)>,
    validation: String,
    // whether the validation-result popover is showing
    show_validation: bool,
    // button-icon textures, built lazily on the first frame
    icons: Option<Icons>,
}

/// Actions collected from the UI and applied after the frame (keeps egui
/// closures free of `&mut self` conflicts).
enum Act {
    OpenDialog,
    Select(usize),
    /// Ctrl-click: toggle this row in the multi-select set.
    ToggleMark(usize),
    /// Shift-click: mark the range from the active row to this one.
    MarkRange(usize),
    PickOutDir,
    CopiedPath,
    Sort(SortKey),
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

    /// Whether the open disk has unsaved edits.
    fn is_dirty(&self) -> bool {
        self.disk.as_ref().is_some_and(|e| e.dirty)
    }

    /// Node indices in display order: filtered by `filter` (case-insensitive
    /// name match), then sorted (directory order unless a sort is set).
    fn sorted_order(&self, filter: &str) -> Vec<usize> {
        let f = filter.trim().to_lowercase();
        let mut order: Vec<usize> = (0..self.nodes.len())
            .filter(|&i| f.is_empty() || self.nodes[i].name.to_lowercase().contains(&f))
            .collect();
        if let Some((key, asc)) = self.sort {
            let size = |n: &Node| n.data.as_ref().map(|d| d.len()).unwrap_or(0);
            order.sort_by(|&a, &b| {
                let (na, nb) = (&self.nodes[a], &self.nodes[b]);
                let o = match key {
                    SortKey::Type => na.badge.cmp(&nb.badge).then_with(|| na.name.cmp(&nb.name)),
                    SortKey::Name => na.name.to_uppercase().cmp(&nb.name.to_uppercase()),
                    SortKey::Blocks => na.blocks.cmp(&nb.blocks),
                    SortKey::Size => size(na).cmp(&size(nb)),
                };
                if asc {
                    o
                } else {
                    o.reverse()
                }
            });
        }
        order
    }

    /// Cycle the sort on a column: off → ascending → descending → off.
    fn cycle_sort(&mut self, key: SortKey) {
        self.sort = match self.sort {
            Some((k, true)) if k == key => Some((key, false)),
            Some((k, false)) if k == key => None,
            _ => Some((key, true)),
        };
    }

    fn open(&mut self, path: &Path) {
        self.nodes.clear();
        self.selected = None;
        self.marked.clear();
        self.filter.clear();
        self.disk = None;
        self.archive = None;
        self.title.clear();
        self.hex_cache = None;
        self.validation.clear();
        self.renaming = None;
        match open_path(path) {
            Ok(Opened::Disk(disk)) => {
                self.disk = Some(EditDisk {
                    disk,
                    path: Some(path.to_path_buf()),
                    dirty: false,
                });
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
        if self.out_dir.is_empty() {
            if let Some(parent) = path.parent() {
                self.out_dir = parent.display().to_string();
            }
        }
        self.opened_path = Some(path.display().to_string());
        self.status = format!("{} item(s) loaded", self.nodes.len());
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
                    badge: f.file_type.mnemonic().to_string(),
                    detail: format!("{} BYTES", f.data.len()),
                    blocks: 0,
                    data: Ok(f.data.clone()),
                })
                .collect();
        }
        if let Some(i) = self.selected {
            if i >= self.nodes.len() {
                self.selected = None;
            }
        }
        // The list was rebuilt; any in-place rename is finished and marks are
        // dropped (indices may have shifted).
        self.renaming = None;
        self.marked.clear();
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
                    if self.is_dirty() {
                        self.confirm = Some(Confirm::OpenFile(p));
                    } else {
                        self.open(&p);
                    }
                }
            }
            Act::Select(i) => {
                self.selected = Some(i);
                self.marked.clear();
            }
            Act::ToggleMark(i) => {
                if !self.marked.insert(i) {
                    self.marked.remove(&i);
                }
                self.selected = Some(i);
            }
            Act::MarkRange(i) => {
                // Range in display order, so Shift-click follows what's shown.
                let order = self.sorted_order(&self.filter.clone());
                let anchor = self.selected.unwrap_or(i);
                let pa = order.iter().position(|&x| x == anchor);
                let pi = order.iter().position(|&x| x == i);
                if let (Some(pa), Some(pi)) = (pa, pi) {
                    for &node in &order[pa.min(pi)..=pa.max(pi)] {
                        self.marked.insert(node);
                    }
                }
                self.selected = Some(i);
            }
            Act::PickOutDir => {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.out_dir = d.display().to_string();
                }
            }
            Act::CopiedPath => self.status = "Copied path to clipboard".into(),
            Act::Sort(key) => self.cycle_sort(key),
            Act::ExtractSelected => self.extract_selected(),
            Act::ExtractAll => self.extract_all(),
            Act::Create => {
                let disk = match self.new_fmt {
                    1 => Disk::format_d71(&self.new_name, &self.new_id),
                    2 => Disk::format_d81(&self.new_name, &self.new_id),
                    _ => Disk::format(&self.new_name, &self.new_id),
                };
                self.disk = Some(EditDisk {
                    disk,
                    path: None,
                    dirty: true,
                });
                self.selected = None;
                self.refresh();
                self.opened_path = None;
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
                self.opened_path = None;
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
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
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
        self.show_validation = true;
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
            stage.push(ImportFile {
                name: name.clone(),
                file_type: ft,
                data: bytes.to_vec(),
            });
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
            let dlg =
                rfd::FileDialog::new().add_filter("disk image", &["d64", "d71", "d81", "d2m"]);
            let dlg = if let Some(p) = &ed.path {
                dlg.set_directory(p.parent().unwrap_or(Path::new(".")))
                    .set_file_name(p.file_name().and_then(|n| n.to_str()).unwrap_or("disk.d64"))
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
                self.opened_path = Some(path.display().to_string());
                self.status = format!("Saved {}", path.display());
                self.refresh();
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn extract_selected(&mut self) {
        // The target rows: the multi-select set if any, else the active row.
        let mut targets: Vec<usize> = if self.marked.is_empty() {
            self.selected.into_iter().collect()
        } else {
            self.marked.iter().copied().collect()
        };
        targets.retain(|&i| i < self.nodes.len());
        if targets.is_empty() {
            self.status = "Select a file first".into();
            return;
        }
        if targets.len() == 1 {
            let i = targets[0];
            let (name, data) = (self.nodes[i].name.clone(), self.nodes[i].data.clone());
            let data = match data {
                Ok(d) => d,
                Err(e) => {
                    self.status = format!("Cannot extract \"{name}\": {e}");
                    return;
                }
            };
            let dir = if self.out_dir.is_empty() {
                ".".into()
            } else {
                self.out_dir.clone()
            };
            let dest = PathBuf::from(&dir).join(sanitize(&name));
            match std::fs::write(&dest, &data) {
                Ok(()) => self.status = format!("Wrote {} ({} bytes)", dest.display(), data.len()),
                Err(e) => self.status = format!("Write failed: {e}"),
            }
            return;
        }
        // Several files.
        let dir = if self.out_dir.is_empty() {
            ".".into()
        } else {
            self.out_dir.clone()
        };
        let (mut ok, mut skipped) = (0, 0);
        for i in targets {
            match &self.nodes[i].data {
                Ok(d) => {
                    let dest = PathBuf::from(&dir).join(sanitize(&self.nodes[i].name));
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

    fn extract_all(&mut self) {
        let dir = if self.out_dir.is_empty() {
            ".".into()
        } else {
            self.out_dir.clone()
        };
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
        if self.icons.is_none() {
            self.icons = Some(Icons::load(ctx));
        }
        let ic = self.icons.clone().unwrap();

        // Guard the window close against unsaved disk edits.
        if ctx.input(|i| i.viewport().close_requested()) && self.is_dirty() {
            if self.confirm.is_none() {
                self.confirm = Some(Confirm::Quit);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        let editable = self.editable();
        let is_disk = self.disk.is_some();
        let is_archive = self.archive.is_some();

        // Refresh the hex preview cache when the selection changes (done before
        // the panels so the closures only read `self` immutably).
        match self.selected {
            Some(i) if i < self.nodes.len() => {
                let stale = self
                    .hex_cache
                    .as_ref()
                    .map(|(ci, _)| *ci != i)
                    .unwrap_or(true);
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
        let preview = self
            .hex_cache
            .as_ref()
            .map(|(_, s)| s.clone())
            .unwrap_or_default();
        let preview_name = self
            .selected
            .and_then(|i| self.nodes.get(i))
            .map(|n| n.name.clone());

        // Context flags that drive which buttons are enabled.
        let sel_now = self.selected;
        let has_sel = self.selected.is_some();
        let sel_extractable = self
            .selected
            .and_then(|i| self.nodes.get(i))
            .map(|n| n.data.is_ok())
            .unwrap_or(false);
        let any_extractable = self.nodes.iter().any(|n| n.data.is_ok());

        // Local copies of edited strings (write back after the frame).
        let mut out_dir = std::mem::take(&mut self.out_dir);
        let mut new_name = std::mem::take(&mut self.new_name);
        let mut new_id = std::mem::take(&mut self.new_id);
        let mut rename_to = std::mem::take(&mut self.rename_to);
        let mut new_fmt = self.new_fmt;
        let mut add_type = self.add_type;
        let mut renaming = self.renaming.filter(|&i| i < self.nodes.len());
        let mut rename_focus = self.rename_focus;
        let mut filter = std::mem::take(&mut self.filter);

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(2.0);
            egui::menu::bar(ui, |ui| {
                ui.label(
                    egui::RichText::new("◆ SECTOR64")
                        .heading()
                        .color(c64::ACCENT)
                        .strong(),
                );
                ui.separator();

                ui.menu_button("File", |ui| {
                    ui.set_min_width(180.0);
                    // New ▸ (disk formats + empty archive)
                    egui::menu::menu_custom_button(ui, icon_button(&ic.doc, "New ▸"), |ui| {
                        ui.set_min_width(200.0);
                        ui.label(egui::RichText::new("New disk image").color(c64::TEXT_DIM));
                        ui.horizontal(|ui| {
                            ui.label("name");
                            ui.add(egui::TextEdit::singleline(&mut new_name).desired_width(120.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label("id  ");
                            ui.add(egui::TextEdit::singleline(&mut new_id).desired_width(40.0));
                        });
                        ui.horizontal(|ui| {
                            for (i, label) in ["D64", "D71", "D81"].iter().enumerate() {
                                if ui.button(*label).clicked() {
                                    new_fmt = i;
                                    acts.push(Act::Create);
                                    ui.close_menu();
                                }
                            }
                        });
                        ui.separator();
                        if ui.button("Empty archive").clicked() {
                            acts.push(Act::NewArchive);
                            ui.close_menu();
                        }
                    });
                    if ui.add(icon_button(&ic.folder, "Open…")).clicked() {
                        acts.push(Act::OpenDialog);
                        ui.close_menu();
                    }
                    // Add file ▸ (type + pick) — only when a disk/archive is open.
                    ui.add_enabled_ui(editable, |ui| {
                        egui::menu::menu_custom_button(
                            ui,
                            icon_button(&ic.plus, "Add file ▸"),
                            |ui| {
                                ui.set_min_width(160.0);
                                ui.label(egui::RichText::new("as type").color(c64::TEXT_DIM));
                                ui.horizontal(|ui| {
                                    for (i, t) in ["prg", "seq", "usr"].iter().enumerate() {
                                        if ui.selectable_label(add_type == i, *t).clicked() {
                                            add_type = i;
                                        }
                                    }
                                });
                                if ui.add(icon_button(&ic.plus, "Add file…")).clicked() {
                                    acts.push(Act::AddFile);
                                    ui.close_menu();
                                }
                            },
                        );
                    });
                    ui.separator();
                    ui.add_enabled_ui(is_disk, |ui| {
                        if ui.add(icon_button(&ic.floppy, "Save")).clicked() {
                            acts.push(Act::Save);
                            ui.close_menu();
                        }
                        if ui.add(icon_button(&ic.floppy, "Save As…")).clicked() {
                            acts.push(Act::SaveAs);
                            ui.close_menu();
                        }
                    });
                    if is_archive
                        && ui
                            .add(icon_button(&ic.floppy, "Save archive as…"))
                            .clicked()
                    {
                        acts.push(Act::SaveArchiveAs);
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.add_enabled_ui(has_sel, |ui| {
                        // Rename the selected row in place.
                        if ui.add(icon_button(&ic.pencil, "Rename")).clicked() {
                            if let Some(s) = sel_now {
                                renaming = Some(s);
                                rename_to = self.nodes[s].name.clone();
                                rename_focus = true;
                            }
                            ui.close_menu();
                        }
                        let del = if is_archive { "Remove" } else { "Delete" };
                        if ui.add(icon_button(&ic.trash, del)).clicked() {
                            acts.push(Act::Delete);
                            ui.close_menu();
                        }
                    });
                });

                ui.separator();

                ui.menu_button("Extract", |ui| {
                    ui.set_min_width(170.0);
                    if ui
                        .add_enabled(sel_extractable, icon_button(&ic.down, "Extract Selected"))
                        .clicked()
                    {
                        acts.push(Act::ExtractSelected);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(any_extractable, icon_button(&ic.down, "Extract All"))
                        .clicked()
                    {
                        acts.push(Act::ExtractAll);
                        ui.close_menu();
                    }
                });

                if is_disk {
                    ui.separator();
                    if ui.button("Validate Disk").clicked() {
                        acts.push(Act::Validate);
                    }
                    ui.separator();
                }
            });
            ui.add_space(2.0);
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.add_space(2.0);
            // "Open:" and "Output:" are equal-width buttons ("Open:  " padded to
            // match "Output:"), so the two path fields line up on the left.
            ui.horizontal(|ui| {
                if ui
                    .button("Open:  ")
                    .on_hover_text("open a disk or archive")
                    .clicked()
                {
                    acts.push(Act::OpenDialog);
                }
                let mut opened = self.opened_path.clone().unwrap_or_default();
                let r = ui.add(
                    egui::TextEdit::singleline(&mut opened)
                        .interactive(false)
                        .hint_text("no file open")
                        .desired_width(f32::INFINITY),
                );
                if !opened.is_empty() {
                    let r = r
                        .on_hover_text("click to copy path")
                        .interact(egui::Sense::click());
                    if r.clicked() {
                        ui.ctx().copy_text(opened.clone());
                        acts.push(Act::CopiedPath);
                    }
                }
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui
                    .button("Output:")
                    .on_hover_text("choose output folder")
                    .clicked()
                {
                    acts.push(Act::PickOutDir);
                }
                ui.add(
                    egui::TextEdit::singleline(&mut out_dir)
                        .hint_text("folder to extract into")
                        .desired_width(f32::INFINITY),
                );
            });
            ui.add_space(2.0);
            egui::Frame::none()
                .fill(c64::WELL)
                .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("►").color(c64::ACCENT));
                        let msg = if self.status.is_empty() {
                            "ready"
                        } else {
                            &self.status
                        };
                        ui.add(
                            egui::Label::new(egui::RichText::new(msg).color(c64::TEXT)).truncate(),
                        );
                    });
                });
        });

        // Validation results appear in a dismiss-on-click popover (built below).

        egui::SidePanel::right("preview")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                let header = match &preview_name {
                    Some(n) => format!("PREVIEW — {n}"),
                    None => "PREVIEW".to_string(),
                };
                ui.label(egui::RichText::new(header).color(c64::ACCENT).strong());
                egui::ScrollArea::vertical()
                    .id_salt("preview_scroll")
                    .show(ui, |ui| {
                        if preview.is_empty() {
                            ui.label(
                                egui::RichText::new("Select a file to preview its bytes.")
                                    .color(c64::TEXT_DIM),
                            );
                        } else {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&preview).monospace()).wrap(),
                            );
                        }
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.nodes.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(
                            "◆ SECTOR64\n\nOpen a disk or archive, or make a new image,\nthen drag files in to add them.",
                        )
                        .color(c64::TEXT_DIM),
                    );
                });
                return;
            }
            // Disk/archive header, shown as a fixed title row above the files
            // (like a 1541 directory's header line).
            if !self.title.is_empty() {
                egui::Frame::none()
                    .fill(c64::BG_PANEL)
                    .stroke(egui::Stroke::new(1.0, c64::ACCENT_DIM))
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&self.title).color(c64::BRIGHT).strong(),
                            )
                            .truncate(),
                        );
                    });
                ui.add_space(2.0);
            }
            // Filter box: live, case-insensitive name filter.
            let order = self.sorted_order(&filter);
            ui.horizontal(|ui| {
                ui.label("Filter:");
                if !filter.trim().is_empty() {
                    ui.label(
                        egui::RichText::new(format!("({}/{})", order.len(), self.nodes.len()))
                            .color(c64::TEXT_DIM),
                    );
                    if ui.button("X").on_hover_text("clear filter").clicked() {
                        filter.clear();
                    }
                }
                ui.add(
                    egui::TextEdit::singleline(&mut filter)
                        .hint_text("type to filter by name")
                        .desired_width(f32::INFINITY),
                );
            });
            ui.add_space(2.0);
            // Auto-pad columns to the widest row so they line up.
            let cols = Cols {
                detail_chars: self.nodes.iter().map(|n| n.detail.chars().count()).max().unwrap_or(0),
                size_digits: self
                    .nodes
                    .iter()
                    .filter_map(|n| n.data.as_ref().ok())
                    .map(|d| d.len().to_string().len())
                    .max()
                    .unwrap_or(1)
                    .max(1),
            };
            // Column header (reserves the scrollbar width so it lines up with
            // the rows below), clickable to sort.
            let bar = ui.spacing().scroll.bar_width + 2.0;
            if let Some(key) = header_row(ui, cols, is_disk, bar, self.sort) {
                acts.push(Act::Sort(key));
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (pos, &i) in order.iter().enumerate() {
                    let node = &self.nodes[i];
                    let sel = self.selected == Some(i) || self.marked.contains(&i);
                    let extractable = node.data.is_ok();
                    let editing = renaming == Some(i);
                    let edit = if editing { Some(&mut rename_to) } else { None };
                    let (resp, result) = file_row(ui, node, sel, pos, cols, edit, editing && rename_focus);
                    if editing {
                        rename_focus = false;
                        match result {
                            RenameEdit::Commit => {
                                acts.push(Act::Select(i));
                                acts.push(Act::Rename);
                                renaming = None;
                            }
                            RenameEdit::Cancel => renaming = None,
                            RenameEdit::None => {}
                        }
                        continue; // a row being renamed takes no other interaction
                    }
                    if resp.clicked() {
                        let mods = ui.input(|i| i.modifiers);
                        if mods.command {
                            acts.push(Act::ToggleMark(i));
                        } else if mods.shift {
                            acts.push(Act::MarkRange(i));
                        } else {
                            acts.push(Act::Select(i));
                        }
                    }
                    if resp.double_clicked() {
                        acts.push(Act::Select(i));
                        acts.push(Act::ExtractSelected);
                    }
                    // Right-click actions for this file.
                    resp.context_menu(|ui| {
                        // Target this row, but don't disturb an existing
                        // multi-selection that already includes it.
                        if !sel {
                            acts.push(Act::Select(i));
                        }
                        ui.set_min_width(150.0);
                        if ui.add_enabled(extractable, icon_button(&ic.down, "Extract")).clicked() {
                            acts.push(Act::ExtractSelected);
                            ui.close_menu();
                        }
                        if editable {
                            ui.separator();
                            if ui.add(icon_button(&ic.pencil, "Rename")).clicked() {
                                renaming = Some(i);
                                rename_to = node.name.clone();
                                rename_focus = true;
                                ui.close_menu();
                            }
                            let del = if is_archive { "Remove" } else { "Delete" };
                            if ui.add(icon_button(&ic.trash, del)).clicked() {
                                acts.push(Act::Select(i)); // delete this row specifically
                                acts.push(Act::Delete);
                                ui.close_menu();
                            }
                        }
                    });
                }
            });
        });

        // Validation result popover — a centred window that dismisses when
        // clicked anywhere on it.
        if self.show_validation {
            let text = self.validation.clone();
            let warn = text.contains('⚠');
            let win = egui::Window::new(
                egui::RichText::new("DISK VALIDATION")
                    .color(c64::ACCENT)
                    .strong(),
            )
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let col = if warn { c64::WARN } else { c64::OK };
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(egui::RichText::new(&text).monospace().color(col))
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
                ui.add_space(6.0);
                ui.label(egui::RichText::new("click to dismiss").color(c64::TEXT_DIM));
            });
            // Dismiss on a mouse click anywhere inside the popover.
            if let Some(inner) = win {
                let clicked_inside = ctx.input(|i| {
                    i.pointer.primary_clicked()
                        && i.pointer
                            .interact_pos()
                            .is_some_and(|p| inner.response.rect.contains(p))
                });
                if clicked_inside {
                    self.show_validation = false;
                }
            }
        }

        // Unsaved-changes confirmation (before opening another file or quitting).
        if self.confirm.is_some() {
            let mut action: Option<u8> = None; // 0 discard, 1 cancel, 2 save
            egui::Window::new(
                egui::RichText::new("UNSAVED CHANGES")
                    .color(c64::WARN)
                    .strong(),
            )
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("The open disk has unsaved changes.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if is_disk && ui.button("💾 Save first").clicked() {
                        action = Some(2);
                    }
                    if ui.button("Discard").clicked() {
                        action = Some(0);
                    }
                    if ui.button("Cancel").clicked() {
                        action = Some(1);
                    }
                });
            });
            match action {
                Some(0) => match self.confirm.take() {
                    Some(Confirm::OpenFile(p)) => self.open(&p),
                    Some(Confirm::Quit) => {
                        if let Some(ed) = &mut self.disk {
                            ed.dirty = false;
                        }
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    None => {}
                },
                Some(1) => self.confirm = None,
                Some(2) => {
                    self.save(false);
                    if !self.is_dirty() {
                        match self.confirm.take() {
                            Some(Confirm::OpenFile(p)) => self.open(&p),
                            Some(Confirm::Quit) => {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close)
                            }
                            None => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Drag-and-drop a host file onto the window to add it (editable disks).
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if editable {
            for p in dropped {
                if let Ok(bytes) = std::fs::read(&p) {
                    acts.push(Act::AddBytes(cbm_name_from(&p), bytes));
                }
            }
        }

        // Keyboard shortcuts. Single-key actions are suppressed while a text
        // field has focus; Ctrl combos always apply.
        let typing = ctx.wants_keyboard_input();
        let n = self.nodes.len();
        let sel_name = sel_now
            .and_then(|s| self.nodes.get(s))
            .map(|x| x.name.clone());
        let order = self.sorted_order(&filter);
        ctx.input(|i| {
            if i.modifiers.command && i.key_pressed(egui::Key::O) {
                acts.push(Act::OpenDialog);
            }
            if i.modifiers.command && i.key_pressed(egui::Key::S) && is_disk {
                acts.push(Act::Save);
            }
            if !typing {
                if i.key_pressed(egui::Key::Enter) && sel_extractable {
                    acts.push(Act::ExtractSelected);
                }
                if i.key_pressed(egui::Key::Delete) && editable && has_sel {
                    acts.push(Act::Delete);
                }
                // F2: rename the selected row in place.
                if i.key_pressed(egui::Key::F2) && editable && has_sel {
                    renaming = sel_now;
                    rename_to = sel_name.clone().unwrap_or_default();
                    rename_focus = true;
                }
                // Arrow keys move through the current display order.
                let cur_pos = sel_now.and_then(|s| order.iter().position(|&x| x == s));
                if n > 0 && i.key_pressed(egui::Key::ArrowDown) {
                    let p = cur_pos.map_or(0, |p| (p + 1).min(n - 1));
                    acts.push(Act::Select(order[p]));
                }
                if n > 0 && i.key_pressed(egui::Key::ArrowUp) {
                    let p = cur_pos.map_or(0, |p| p.saturating_sub(1));
                    acts.push(Act::Select(order[p]));
                }
            }
        });

        // Write edited strings back, then apply actions.
        self.filter = filter;
        self.out_dir = out_dir;
        self.new_name = new_name;
        self.new_id = new_id;
        self.rename_to = rename_to;
        self.new_fmt = new_fmt;
        self.add_type = add_type;
        self.renaming = renaming;
        self.rename_focus = rename_focus;
        for act in acts {
            self.apply(act);
        }
    }
}

/// Fixed column widths (in characters) for the listing, computed once per
/// frame from the widest row so the columns line up.
#[derive(Clone, Copy)]
struct Cols {
    detail_chars: usize,
    size_digits: usize,
}

/// Outcome of an in-place name edit on a row.
enum RenameEdit {
    /// Not editing this row.
    None,
    /// Enter pressed — apply the new name.
    Commit,
    /// Focus lost or Escape — discard.
    Cancel,
}

/// Clickable column header, using the same column math as [`file_row`] so the
/// labels sit over their columns. Returns the column whose header was clicked.
/// `scrollbar` reserves the list's scrollbar width.
fn header_row(
    ui: &mut egui::Ui,
    cols: Cols,
    is_disk: bool,
    scrollbar: f32,
    sort: Option<(SortKey, bool)>,
) -> Option<SortKey> {
    use egui::{pos2, vec2, Align2, FontId, Sense, Stroke};
    let mono = FontId::monospace(16.0);
    let cw = ui.fonts(|f| f.glyph_width(&mono, '0')).max(6.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    let resp = resp.on_hover_text("click a column to sort (again to reverse / clear)");
    let painter = ui.painter().clone();
    let pad = 8.0;
    let cy = rect.center().y;
    let right_edge = rect.right() - pad - scrollbar;
    let size_field_w = (cols.size_digits + 6) as f32 * cw;
    let status_x = right_edge - size_field_w;
    let detail_field_w = cols.detail_chars as f32 * cw;
    let detail_x = status_x - 2.0 * cw - detail_field_w;
    let type_x = rect.left() + pad;
    let name_x = type_x + cw * 6.0;

    let label = |key: SortKey, text: &str| -> (String, egui::Color32) {
        match sort {
            Some((k, asc)) if k == key => (
                format!("{text}{}", if asc { " ▲" } else { " ▼" }),
                c64::ACCENT,
            ),
            _ => (text.to_string(), c64::TEXT_DIM),
        }
    };
    let draw = |painter: &egui::Painter, x: f32, key: SortKey, text: &str| {
        let (s, c) = label(key, text);
        painter.text(pos2(x, cy), Align2::LEFT_CENTER, s, mono.clone(), c);
    };
    draw(&painter, type_x, SortKey::Type, "TYPE");
    draw(&painter, name_x, SortKey::Name, "NAME");
    if is_disk {
        draw(&painter, detail_x, SortKey::Blocks, "BLK");
    }
    draw(&painter, status_x, SortKey::Size, "SIZE");
    painter.hline(
        rect.x_range(),
        rect.bottom() - 1.0,
        Stroke::new(1.0, c64::ACCENT_DIM),
    );

    if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            return Some(if p.x >= status_x - cw {
                SortKey::Size
            } else if is_disk && p.x >= detail_x - cw {
                SortKey::Blocks
            } else if p.x < name_x {
                SortKey::Type
            } else {
                SortKey::Name
            });
        }
    }
    None
}

/// Draw one file as a full-width, painted, selectable row: a colour-coded type
/// badge, the name, its detail, and a right-aligned size (or an error). When
/// `edit` is `Some`, the name is shown as an editable field instead.
fn file_row(
    ui: &mut egui::Ui,
    node: &Node,
    selected: bool,
    index: usize,
    cols: Cols,
    edit: Option<&mut String>,
    focus: bool,
) -> (egui::Response, RenameEdit) {
    use egui::{pos2, vec2, Align2, FontId, Rect, Rounding, Sense};
    let row_h = 24.0;
    let mono = FontId::monospace(16.0);
    // Monospace advance width, measured so layout is correct for either the C64
    // font or the fallback.
    let cw = ui.fonts(|f| f.glyph_width(&mono, '0')).max(6.0);

    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());

    // Cloned painter so the `ui` is free for an inline TextEdit widget later.
    let painter = ui.painter().clone();
    if selected {
        painter.rect_filled(rect, Rounding::ZERO, c64::ACCENT);
    } else if resp.hovered() {
        painter.rect_filled(rect, Rounding::ZERO, c64::ACCENT_DIM);
    } else if index % 2 == 1 {
        painter.rect_filled(rect, Rounding::ZERO, c64::STRIPE);
    }

    let text_col = if selected { c64::BG } else { c64::TEXT };
    let dim_col = if selected {
        c64::BG_PANEL
    } else {
        c64::TEXT_DIM
    };
    let pad = 8.0;
    let cy = rect.center().y;

    // Type badge: a boxed, colour-coded mnemonic sized to its text.
    let mut name_x = rect.left() + pad;
    if !node.badge.is_empty() {
        let bw = cw * node.badge.chars().count() as f32 + 8.0;
        let brect = Rect::from_min_size(
            pos2(rect.left() + pad, rect.top() + 3.0),
            vec2(bw, row_h - 6.0),
        );
        let bcol = if selected {
            c64::BG
        } else {
            badge_color(&node.badge)
        };
        painter.rect_stroke(brect, Rounding::ZERO, egui::Stroke::new(1.0, bcol));
        painter.text(
            brect.center(),
            Align2::CENTER_CENTER,
            node.badge.to_uppercase(),
            mono.clone(),
            bcol,
        );
        name_x = brect.right() + pad;
    }

    // Fixed, left-justified columns. The size column holds a
    // right-padded number so the digits (and the "bytes"/"blk" units) line up;
    // the detail column is left-aligned at a fixed x computed from the widest
    // row, so every row's columns align regardless of content.
    let right_edge = rect.right() - pad;
    let size_field_w = (cols.size_digits + 6) as f32 * cw; // "NNNNN bytes"
    let status_x = right_edge - size_field_w;
    let detail_field_w = cols.detail_chars as f32 * cw;
    let detail_x = status_x - 2.0 * cw - detail_field_w;

    match &node.data {
        Ok(d) => {
            let status = format!("{:>w$} BYTES", d.len(), w = cols.size_digits);
            painter.text(
                pos2(status_x, cy),
                Align2::LEFT_CENTER,
                status,
                mono.clone(),
                dim_col,
            );
        }
        Err(_) => {
            let col = if selected { c64::BG } else { c64::WARN };
            painter.text(
                pos2(right_edge, cy),
                Align2::RIGHT_CENTER,
                "⚠ can't extract",
                mono.clone(),
                col,
            );
        }
    }

    if !node.detail.is_empty() {
        painter.text(
            pos2(detail_x, cy),
            Align2::LEFT_CENTER,
            &node.detail,
            mono.clone(),
            dim_col,
        );
    }

    // Name zone (left-aligned), or an inline editable field when renaming.
    let name_end = detail_x - cw;
    if let Some(buf) = edit {
        let name_rect = Rect::from_min_max(
            pos2(name_x, rect.top() + 2.0),
            pos2(name_end.max(name_x + 4.0 * cw), rect.bottom() - 2.0),
        );
        let te = ui.put(
            name_rect,
            egui::TextEdit::singleline(buf)
                .font(FontId::monospace(16.0))
                .margin(egui::vec2(2.0, 0.0)),
        );
        if focus {
            te.request_focus();
        }
        let result = if te.lost_focus() {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                RenameEdit::Commit
            } else {
                RenameEdit::Cancel
            }
        } else {
            RenameEdit::None
        };
        return (resp, result);
    }

    let max_chars = ((name_end - name_x) / cw).max(3.0) as usize;
    let name = if node.name.chars().count() > max_chars {
        let mut s: String = node
            .name
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect();
        s.push('…');
        s
    } else {
        node.name.clone()
    };
    painter.text(pos2(name_x, cy), Align2::LEFT_CENTER, &name, mono, text_col);

    (
        resp.on_hover_text("double-click to extract · right-click for actions"),
        RenameEdit::None,
    )
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
            s.push(if (0x20..=0x7E).contains(&b) {
                b as char
            } else {
                '.'
            });
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

enum Opened {
    Disk(Disk),
    List(String, Vec<Node>),
}

/// Build the listing (title, items) from a disk.
fn disk_nodes(disk: &Disk) -> (String, Vec<Node>) {
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
    let free = disk
        .blocks_free()
        .map(|f| format!("{f} free"))
        .unwrap_or_default();
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
                badge: f.file_type.mnemonic().to_string(),
                detail: format!("{} BYTES", f.data.len()),
                name: f.name,
                blocks: 0,
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
            let parse = if bytes.starts_with(&[0x01, 0x08]) {
                parse_sda
            } else {
                parse_arc
            };
            let nodes = parse(&bytes)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|e| Node {
                    badge: e.file_type.mnemonic().to_string(),
                    detail: format!("{} BYTES  {}", e.orig_size, e.method.label()),
                    blocks: 0,
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
                    badge: String::new(),
                    detail: format!(
                        "{} → {} BYTES  {}",
                        e.compressed_size,
                        e.original_size,
                        e.method_str()
                    ),
                    blocks: 0,
                    data: e.decode().map_err(|x| x.to_string()),
                    name: e.name,
                })
                .collect();
            ("LHA/SFX archive", nodes)
        }
        "lbr" => (
            "LBR library",
            import_nodes(parse_lbr(&bytes).map_err(|e| e.to_string())?),
        ),
        "t64" => (
            "T64 tape archive",
            import_nodes(parse_t64(&bytes).map_err(|e| e.to_string())?),
        ),
        "lnx" => (
            "Lynx archive",
            import_nodes(parse_lnx(&bytes).map_err(|e| e.to_string())?),
        ),
        "wr3" => {
            // Members share one sliding window, so decode the whole archive.
            let nodes = sector64::decompress_wr3(&bytes)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|f| Node {
                    badge: String::new(),
                    detail: format!("{} BYTES", f.content.len()),
                    blocks: 0,
                    data: Ok(f.content),
                    name: f.name,
                })
                .collect();
            ("Wraptor-3 archive", nodes)
        }
        _ if is_p00 => (
            "PC64 container",
            import_nodes(vec![parse_p00(&bytes).map_err(|e| e.to_string())?]),
        ),
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
    egui::IconData {
        rgba,
        width: w as u32,
        height: h as u32,
    }
}

fn main() -> eframe::Result<()> {
    let initial = std::env::args().nth(1);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_icon(std::sync::Arc::new(make_icon())),
        ..Default::default()
    };
    eframe::run_native(
        "Sector64",
        options,
        Box::new(move |cc| {
            load_c64_font(&cc.egui_ctx);
            apply_c64_theme(&cc.egui_ctx);
            Ok(Box::new(App::new(initial)))
        }),
    )
}
