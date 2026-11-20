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
    write_lnx, write_p00, write_t64, BamState, BasicDialect, BlockMap, BlockStatus, Disk, FileType,
    GfxKind, GfxSheet, ImportFile, Owner,
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
    /// Disk map: allocated blocks nothing uses.
    pub const ORPHAN: Color32 = Color32::from_rgb(0xd8, 0xb0, 0x50);
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
    #[cfg(not(windows))]
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
    // The folder holding the executable comes first, so a portable install
    // (zip next to the .exe) can keep a user-supplied font beside it.
    let mut dirs: Vec<String> = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.display().to_string()))
        .into_iter()
        .collect();
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(format!("{local}/Microsoft/Windows/Fonts"));
        }
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:/Windows".into());
        dirs.push(format!("{windir}/Fonts"));
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        dirs.push(format!("{home}/.local/share/fonts"));
        dirs.push(format!("{home}/.fonts"));
        dirs.push("/usr/share/fonts".into());
        dirs.push("/usr/local/share/fonts".into());
    }
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
    // egui keeps separate dark and light styles and, by default, follows the
    // OS theme; `set_style` alone only replaces the one active right now. On a
    // light-themed OS (common on Windows) egui would then switch to its stock
    // light style. Use the C64 look for both and pin the app to dark.
    let style = std::sync::Arc::new(style);
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
    ctx.set_theme(egui::ThemePreference::Dark);
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

/// The PACKED column text for compressed archive members: each packed size
/// right-aligned to the widest one, so the method names after it line up.
fn packed_details(items: impl Iterator<Item = (usize, String)>) -> Vec<String> {
    let items: Vec<_> = items.collect();
    let w = items
        .iter()
        .map(|(n, _)| n.to_string().len())
        .max()
        .unwrap_or(1);
    items
        .into_iter()
        .map(|(n, m)| format!("{n:>w$} BYTES  {m}"))
        .collect()
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

/// The second disk in the Copy window: files are copied onto it from the
/// main listing, and back from it onto the open disk.
struct OtherDisk {
    disk: Disk,
    path: Option<PathBuf>,
    dirty: bool,
    /// Earlier states for the window's Undo (disk, dirty), newest last.
    undo: Vec<(Disk, bool)>,
    /// Rows selected in its list.
    marked: std::collections::BTreeSet<usize>,
}

/// How many edits Undo can step back through.
const UNDO_MAX: usize = 32;

/// The editable content at one point in its history, for Undo / Redo.
enum Snapshot {
    /// An open disk image and whether it differed from the file on disk.
    Disk(Disk, bool),
    /// A new archive's staged files.
    Archive(Vec<ImportFile>),
}

/// One undoable step: the content before (on the undo stack) or after (on the
/// redo stack) an edit, and what the edit was, for the status line.
struct UndoStep {
    snapshot: Snapshot,
    label: String,
}

/// Which column the listing is sorted by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Type,
    Name,
    Blocks,
    Size,
}

/// How the preview pane renders the selected file's bytes.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PreviewMode {
    /// Offset + hex + ASCII gutter (the default).
    #[default]
    Hex,
    /// The bytes as PETSCII text, with CR/LF as line breaks.
    Text,
    /// A BASIC program listed the way the C64's LIST shows it.
    Basic,
    /// The bytes drawn as sprites or characters.
    Gfx,
}

/// The sprite/character sheet the GFX preview shows, and what it was drawn
/// from: (node index, kind, multicolour, cells per row).
struct GfxView {
    key: (usize, GfxKind, bool, usize),
    sheet: GfxSheet,
    tex: egui::TextureHandle,
    /// The PRG load address the cells were aligned to, if any.
    load: Option<u16>,
    /// Whole cells in the file; the sheet may show fewer.
    total: usize,
}

/// A pending action blocked by unsaved changes.
enum Confirm {
    /// Open this file once the user discards changes.
    OpenFile(PathBuf),
    /// Quit once the user discards changes.
    Quit,
}

/// The user manual, embedded so Help works with no external files.
const MANUAL: &str = include_str!("../MANUAL.md");

/// How long a toast stays fully visible before it starts to fade, and how long
/// the fade-out lasts (seconds).
const TOAST_TTL: f64 = 4.5;
const TOAST_FADE: f64 = 0.6;

/// Severity of a transient toast, which picks its border colour and tag.
#[derive(Clone, Copy, PartialEq)]
enum ToastKind {
    Success,
    Error,
    Info,
}

impl ToastKind {
    /// Classify a status message into a toast severity by its wording.
    fn of(msg: &str) -> ToastKind {
        let l = msg.to_ascii_lowercase();
        if l.contains("fail") || l.contains("error") || l.contains("cannot") || l.contains("unknown")
        {
            ToastKind::Error
        } else if l.starts_with("select ")
            || l.starts_with("enter ")
            || l.contains("at least")
            || l.contains("nothing to")
            || l.contains("holds exactly")
        {
            ToastKind::Info
        } else {
            ToastKind::Success
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            ToastKind::Success => c64::OK,
            ToastKind::Error => c64::WARN,
            ToastKind::Info => c64::ACCENT,
        }
    }

    fn tag(self) -> &'static str {
        match self {
            ToastKind::Success => "OK",
            ToastKind::Error => "ERR",
            ToastKind::Info => "i",
        }
    }
}

/// A transient on-screen notification, stacked at the bottom-right.
struct Toast {
    kind: ToastKind,
    text: String,
    /// `ctx.input().time` at which this toast was created.
    born: f64,
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
    /// Last status text turned into a toast, so each change toasts once.
    last_status: String,
    /// Active transient notifications (newest first in the stack).
    toasts: Vec<Toast>,
    /// Path of the currently-open file (shown on the "Open:" line).
    opened_path: Option<String>,
    /// Recently opened or saved files, newest first (File ▸ Open Recent).
    recent: Vec<String>,
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
    /// The listing is a read-only disk (G64, a 1581-partition D2M, a Zip-Code
    /// set): its rows carry block counts, so the BLK column applies.
    read_only_disk: bool,
    /// The read-only disk itself, kept for the disk map.
    ro_disk: Option<Disk>,
    /// Whether the Disk Map window is open.
    show_map: bool,
    /// Whether the Copy window is open, and the disk it copies to and from.
    show_copy: bool,
    other: Option<OtherDisk>,
    /// The disk map, built when the window opens and dropped on every change.
    map_cache: Option<BlockMap>,
    // staging list when building a new archive
    archive: Option<Vec<ImportFile>>,
    /// Edits that Undo steps back through, newest last.
    undo: Vec<UndoStep>,
    /// Edits that Redo re-applies, newest last; cleared by any new edit.
    redo: Vec<UndoStep>,
    // preview/validation pane
    /// Cached rendered preview: (node index, mode it was rendered in, text).
    hex_cache: Option<(usize, PreviewMode, Option<BasicDialect>, String)>,
    /// How the preview pane renders bytes (hex or PETSCII text).
    preview_mode: PreviewMode,
    /// The BASIC dialect the BASIC preview lists in; `None` detects it.
    basic_dialect: Option<BasicDialect>,
    /// What the GFX preview draws the bytes as, and whether in multicolour.
    gfx_kind: GfxKind,
    gfx_multi: bool,
    /// The GFX preview's sheet, redrawn when the selection or options change.
    gfx_cache: Option<GfxView>,
    validation: String,
    // whether the validation-result popover is showing
    show_validation: bool,
    // whether the preview pane is visible (toggled from the menu bar)
    show_preview: bool,
    // whether the Help / user-manual window is open
    show_help: bool,
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
    /// A file was dropped on the window with nothing editable open: open it.
    /// The count is how many other dropped files were ignored.
    OpenDropped(PathBuf, usize),
    /// Show a status message.
    Status(String),
    /// Open a file from the recent-files list.
    OpenRecent(PathBuf),
    /// Empty the recent-files list.
    ClearRecent,
    Undo,
    Redo,
    Rename,
    Delete,
    Save,
    SaveAs,
    Validate,
    NewArchive,
    SaveArchiveAs,
    /// Copy window: open a disk to copy to, or make a blank one (0 D64,
    /// 1 D71, 2 D81).
    OtherOpen,
    OtherNew(u8),
    /// Save the other disk (true: Save As).
    OtherSave(bool),
    /// Drop the other disk and its unsaved changes.
    OtherDiscard,
    OtherUndo,
    /// Click a row of the other disk's list (true: Ctrl-click toggles it).
    OtherMark(usize, bool),
    /// Copy the main listing's selected files onto the other disk.
    CopyToOther,
    /// Copy the other disk's selected files onto the open disk or archive.
    CopyFromOther,
}

impl App {
    fn new(initial: Option<String>, storage: Option<&dyn eframe::Storage>) -> Self {
        let mut app = App {
            new_name: "blank".into(),
            new_id: "01".into(),
            ..Default::default()
        };
        // Restore the remembered output folder (window geometry is restored by
        // eframe itself).
        if let Some(s) = storage {
            if let Some(dir) = eframe::get_value::<String>(s, "out_dir") {
                app.out_dir = dir;
            }
            if let Some(list) = eframe::get_value::<Vec<String>>(s, "recent_files") {
                app.recent = list;
                app.recent.truncate(RECENT_MAX);
            }
        }
        if let Some(p) = initial {
            app.open(Path::new(&p));
        }
        app
    }

    /// Whether add/rename/delete apply (an editable disk or a new archive).
    fn editable(&self) -> bool {
        self.disk.is_some() || self.archive.is_some()
    }

    /// The disk the map shows: the editable one or a read-only one.
    fn map_disk(&self) -> Option<&Disk> {
        self.disk
            .as_ref()
            .map(|e| &e.disk)
            .or(self.ro_disk.as_ref())
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
        self.clear_history();
        self.nodes.clear();
        self.selected = None;
        self.marked.clear();
        self.filter.clear();
        self.disk = None;
        self.archive = None;
        self.read_only_disk = false;
        self.ro_disk = None;
        self.map_cache = None;
        self.title.clear();
        self.hex_cache = None;
        self.gfx_cache = None;
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
            Ok(Opened::ReadOnlyDisk(title, nodes, disk)) => {
                self.title = title;
                self.nodes = nodes;
                self.read_only_disk = true;
                self.ro_disk = Some(disk);
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
        remember_recent(&mut self.recent, path);
        // Select the first entry so the preview pane shows something at once.
        self.selected = (!self.nodes.is_empty()).then_some(0);
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
                    detail: String::new(), // size is in the SIZE column
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
        // dropped (indices may have shifted). The disk may have changed too.
        self.map_cache = None;
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
            Act::OpenDropped(p, ignored) => {
                self.open(&p);
                if ignored > 0 && !self.status.starts_with("Error") {
                    self.status = format!(
                        "{} (opened the first dropped file; {ignored} other(s) ignored)",
                        self.status
                    );
                }
            }
            Act::Status(msg) => self.status = msg,
            Act::OpenRecent(p) => {
                if !p.exists() {
                    // Just the name: a full path would push the message
                    // off the end of the status line.
                    let shown = p.file_name().unwrap_or(p.as_os_str()).to_string_lossy();
                    self.status =
                        format!("Error: {shown} no longer exists (removed from recent files)");
                    self.recent.retain(|r| Path::new(r) != p);
                } else if self.is_dirty() {
                    self.confirm = Some(Confirm::OpenFile(p));
                } else {
                    self.open(&p);
                }
            }
            Act::Undo => self.undo(),
            Act::Redo => self.redo(),
            Act::ClearRecent => {
                self.recent.clear();
                self.status = "Cleared the recent-files list".into();
            }
            Act::Sort(key) => self.cycle_sort(key),
            Act::ExtractSelected => self.extract_selected(),
            Act::ExtractAll => self.extract_all(),
            Act::Create => {
                let disk = match self.new_fmt {
                    1 => Disk::format_d71(&self.new_name, &self.new_id),
                    2 => Disk::format_d81(&self.new_name, &self.new_id),
                    _ => Disk::format(&self.new_name, &self.new_id),
                };
                self.clear_history();
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
                self.clear_history();
                self.disk = None;
                self.read_only_disk = false;
                self.archive = Some(Vec::new());
                self.selected = None;
                self.hex_cache = None;
                self.gfx_cache = None;
                self.refresh();
                self.opened_path = None;
                self.status = "New archive — add files, then Save archive as…".into();
            }
            Act::SaveArchiveAs => self.save_archive(),
            Act::OtherOpen => {
                let dlg = rfd::FileDialog::new()
                    .add_filter("disk image", &["d64", "d71", "d81", "d2m"])
                    .add_filter("All files", &["*"]);
                if let Some(p) = dlg.pick_file() {
                    self.open_other(&p);
                }
            }
            Act::OtherNew(fmt) => {
                let disk = match fmt {
                    1 => Disk::format_d71(&self.new_name, &self.new_id),
                    2 => Disk::format_d81(&self.new_name, &self.new_id),
                    _ => Disk::format(&self.new_name, &self.new_id),
                };
                self.other = Some(OtherDisk {
                    disk,
                    path: None,
                    dirty: true,
                    undo: Vec::new(),
                    marked: Default::default(),
                });
                self.status = "Made a blank disk to copy to (unsaved)".into();
            }
            Act::OtherSave(save_as) => self.save_other(save_as),
            Act::OtherDiscard => {
                self.other = None;
                self.status = "Closed the other disk".into();
            }
            Act::OtherUndo => {
                if let Some(o) = &mut self.other {
                    if let Some((disk, dirty)) = o.undo.pop() {
                        o.disk = disk;
                        o.dirty = dirty;
                        o.marked.clear();
                        self.status = "Undone the last copy onto the other disk".into();
                    }
                }
            }
            Act::OtherMark(i, toggle) => {
                if let Some(o) = &mut self.other {
                    if !toggle {
                        o.marked.clear();
                        o.marked.insert(i);
                    } else if !o.marked.insert(i) {
                        o.marked.remove(&i);
                    }
                }
            }
            Act::CopyToOther => self.copy_to_other(),
            Act::CopyFromOther => self.copy_from_other(),
        }
    }

    /// Open `path` as the Copy window's disk. Only disks that can be written
    /// (D64, D71, D81, CMD-native D2M) qualify.
    fn open_other(&mut self, path: &Path) {
        match open_path(path) {
            Ok(Opened::Disk(disk)) => {
                self.other = Some(OtherDisk {
                    disk,
                    path: Some(path.to_path_buf()),
                    dirty: false,
                    undo: Vec::new(),
                    marked: Default::default(),
                });
                self.status = format!("Opened {} to copy to", path.display());
            }
            Ok(_) => {
                self.status =
                    "Copy to a D64, D71, D81 or CMD-native D2M disk; this one is read-only".into()
            }
            Err(e) => self.status = format!("Error: {e}"),
        }
    }

    fn save_other(&mut self, save_as: bool) {
        let Some(o) = &mut self.other else { return };
        let path = match (&o.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let dlg = rfd::FileDialog::new()
                    .add_filter("disk image", &["d64", "d71", "d81", "d2m"])
                    .set_file_name(default_disk_name(&o.disk));
                match dlg.save_file() {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        match std::fs::write(&path, o.disk.as_bytes()) {
            Ok(()) => {
                o.path = Some(path.clone());
                o.dirty = false;
                for (_, dirty) in &mut o.undo {
                    *dirty = true;
                }
                remember_recent(&mut self.recent, &path);
                self.status = format!("Saved {}", path.display());
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    /// The main listing's chosen rows: the multi-select set if any, else the
    /// active row.
    fn chosen_rows(&self) -> Vec<usize> {
        let mut rows: Vec<usize> = if self.marked.is_empty() {
            self.selected.into_iter().collect()
        } else {
            self.marked.iter().copied().collect()
        };
        rows.retain(|&i| i < self.nodes.len());
        rows
    }

    /// Copy the main listing's chosen files onto the other disk, as one
    /// step of its Undo.
    fn copy_to_other(&mut self) {
        let rows = self.chosen_rows();
        let Some(o) = &mut self.other else { return };
        if rows.is_empty() {
            self.status = "Select files in the list to copy".into();
            return;
        }
        let before = o.disk.clone();
        // From a disk, copy directory entries (exact names, lock flags);
        // from an archive, the extracted bytes under the listed name.
        let src = self
            .disk
            .as_ref()
            .map(|e| &e.disk)
            .or(self.ro_disk.as_ref());
        let entries = src.map(|d| d.directory().unwrap_or_default());
        let mut done = Vec::new();
        let mut failed = Vec::new();
        for i in rows {
            let node = &self.nodes[i];
            let r = match (src, entries.as_ref().and_then(|e| e.get(i))) {
                (Some(src), Some(entry)) => {
                    o.disk.copy_file_from(src, entry).map_err(|e| e.to_string())
                }
                _ => copy_bytes(&mut o.disk, node),
            };
            match r {
                Ok(()) => done.push(node.name.clone()),
                Err(e) => failed.push(format!("\"{}\": {e}", node.name)),
            }
        }
        if !done.is_empty() {
            o.undo.push((before, o.dirty));
            if o.undo.len() > UNDO_MAX {
                o.undo.remove(0);
            }
            o.dirty = true;
            o.marked.clear();
        }
        self.status = copy_report(&done, &failed, "the other disk");
    }

    /// Copy the other disk's selected files onto the open disk (or into a
    /// new archive), as one step of the main Undo.
    fn copy_from_other(&mut self) {
        let Some(o) = &self.other else { return };
        let entries = o.disk.directory().unwrap_or_default();
        let picked: Vec<&sector64::Entry> =
            o.marked.iter().filter_map(|&i| entries.get(i)).collect();
        if picked.is_empty() {
            self.status = "Select files on the other disk to copy".into();
            return;
        }
        let before = self.snapshot();
        let mut done = Vec::new();
        let mut failed = Vec::new();
        for e in picked {
            let r = if let Some(ed) = &mut self.disk {
                ed.disk
                    .copy_file_from(&o.disk, e)
                    .map_err(|x| x.to_string())
            } else if let Some(stage) = &mut self.archive {
                match (e.file_type, o.disk.read_entry(e)) {
                    (FileType::Rel, _) => Err(sector64::Error::CannotCopy("REL files").to_string()),
                    (_, Err(x)) => Err(x.to_string()),
                    (file_type, Ok(data)) => {
                        stage.push(ImportFile {
                            name: e.name.clone(),
                            file_type,
                            data,
                        });
                        Ok(())
                    }
                }
            } else {
                self.status = "Open a disk or start a new archive to copy into".into();
                return;
            };
            match r {
                Ok(()) => done.push(e.name.clone()),
                Err(x) => failed.push(format!("\"{}\": {x}", e.name)),
            }
        }
        let status = copy_report(&done, &failed, "the open disk");
        if !done.is_empty() {
            if let Some(ed) = &mut self.disk {
                ed.dirty = true;
            }
            self.record(
                before,
                &format!("Copied {} file(s) from the other disk", done.len()),
            );
            self.hex_cache = None;
            self.gfx_cache = None;
            self.refresh();
        }
        self.status = status;
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
        let before = self.snapshot();
        let done = format!("Added \"{name}\" ({} bytes)", bytes.len());
        if let Some(ed) = &mut self.disk {
            match ed.disk.write_file(&name, ft, bytes) {
                Ok(()) => {
                    ed.dirty = true;
                    self.record(before, &done);
                    self.refresh();
                    self.status = done;
                }
                Err(e) => self.status = format!("Add failed: {e}"),
            }
        } else if let Some(stage) = &mut self.archive {
            stage.push(ImportFile {
                name: name.clone(),
                file_type: ft,
                data: bytes.to_vec(),
            });
            self.record(before, &done);
            self.refresh();
            self.status = done;
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
        let before = self.snapshot();
        let done = format!("Renamed \"{old}\" → \"{new}\"");
        if let Some(ed) = &mut self.disk {
            match ed.disk.rename(&old, &new) {
                Ok(()) => {
                    ed.dirty = true;
                    self.record(before, &done);
                    self.refresh();
                    self.status = done;
                }
                Err(e) => self.status = format!("Rename failed: {e}"),
            }
        } else if let Some(stage) = &mut self.archive {
            stage[i].name = new.clone();
            self.record(before, &done);
            self.refresh();
            self.status = done;
        }
    }

    fn delete(&mut self) {
        let Some(i) = self.selected else {
            self.status = "Select a file to delete".into();
            return;
        };
        let name = self.nodes[i].name.clone();
        let before = self.snapshot();
        if let Some(ed) = &mut self.disk {
            match ed.disk.scratch(&name) {
                Ok(n) => {
                    ed.dirty = true;
                    let done = format!("Deleted \"{name}\" ({n} blocks freed)");
                    self.record(before, &done);
                    self.selected = None;
                    self.refresh();
                    self.status = done;
                }
                Err(e) => self.status = format!("Delete failed: {e}"),
            }
        } else if let Some(stage) = &mut self.archive {
            stage.remove(i);
            let done = format!("Removed \"{name}\"");
            self.record(before, &done);
            self.selected = None;
            self.refresh();
            self.status = done;
        }
    }

    /// The editable content as it is now, if a disk or new archive is open.
    fn snapshot(&self) -> Option<Snapshot> {
        if let Some(ed) = &self.disk {
            Some(Snapshot::Disk(ed.disk.clone(), ed.dirty))
        } else {
            self.archive.as_ref().map(|a| Snapshot::Archive(a.clone()))
        }
    }

    /// Put `snapshot` back as the editable content.
    fn restore(&mut self, snapshot: Snapshot) {
        match snapshot {
            Snapshot::Disk(disk, dirty) => {
                if let Some(ed) = &mut self.disk {
                    ed.disk = disk;
                    ed.dirty = dirty;
                }
            }
            Snapshot::Archive(files) => self.archive = Some(files),
        }
        self.hex_cache = None;
        self.gfx_cache = None;
        self.refresh();
    }

    /// Record a successful edit: `before` goes on the undo stack and the redo
    /// stack is dropped (a new edit starts a new branch of history).
    fn record(&mut self, before: Option<Snapshot>, label: &str) {
        let Some(snapshot) = before else { return };
        self.undo.push(UndoStep {
            snapshot,
            label: label.to_string(),
        });
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn undo(&mut self) {
        let (Some(step), Some(now)) = (self.undo.pop(), self.snapshot()) else {
            self.status = "Nothing to undo".into();
            return;
        };
        self.restore(step.snapshot);
        self.status = format!("Undone: {}", step.label);
        self.redo.push(UndoStep {
            snapshot: now,
            label: step.label,
        });
    }

    fn redo(&mut self) {
        let (Some(step), Some(now)) = (self.redo.pop(), self.snapshot()) else {
            self.status = "Nothing to redo".into();
            return;
        };
        self.restore(step.snapshot);
        self.status = format!("Redone: {}", step.label);
        self.undo.push(UndoStep {
            snapshot: now,
            label: step.label,
        });
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
                // Every other point in the history now differs from the file.
                for step in self.undo.iter_mut().chain(self.redo.iter_mut()) {
                    if let Snapshot::Disk(_, dirty) = &mut step.snapshot {
                        *dirty = true;
                    }
                }
                self.opened_path = Some(path.display().to_string());
                remember_recent(&mut self.recent, &path);
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

    /// Turn a new status line into a toast (once per change).
    fn pump_toasts(&mut self, now: f64) {
        if self.status.is_empty() || self.status == self.last_status {
            return;
        }
        self.last_status = self.status.clone();
        self.toasts.push(Toast {
            kind: ToastKind::of(&self.status),
            text: self.status.clone(),
            born: now,
        });
        // Keep only the few most recent so the stack can't grow unbounded.
        let n = self.toasts.len();
        if n > 4 {
            self.toasts.drain(0..n - 4);
        }
    }

    /// Draw the toast stack (bottom-right) and drop expired or clicked ones.
    fn show_toasts(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.toasts.retain(|t| now - t.born < TOAST_TTL);
        if self.toasts.is_empty() {
            return;
        }
        // Repaint while toasts live so they fade and expire even when idle.
        ctx.request_repaint_after(std::time::Duration::from_millis(80));

        let mut dismiss: Option<usize> = None;
        egui::Area::new(egui::Id::new("toasts"))
            // Upper-right, clear of the menu bar's left-aligned items.
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 52.0))
            // Foreground so the stack paints above the menu-bar panel.
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                // Top-down with the newest first, so it sits at the top of the
                // stack (nearest the corner) and older ones push down.
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    for (idx, t) in self.toasts.iter().enumerate().rev() {
                        let remain = TOAST_TTL - (now - t.born);
                        let a = (remain / TOAST_FADE).clamp(0.0, 1.0) as f32;
                        let accent = t.kind.color().gamma_multiply(a);
                        let resp = egui::Frame::none()
                            .fill(c64::BG_PANEL.gamma_multiply(a))
                            .stroke(egui::Stroke::new(1.0, accent))
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .outer_margin(egui::Margin {
                                top: 6.0,
                                ..Default::default()
                            })
                            .show(ui, |ui| {
                                ui.set_max_width(360.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(t.kind.tag()).color(accent).strong(),
                                    );
                                    ui.add(egui::Label::new(
                                        egui::RichText::new(&t.text)
                                            .color(c64::TEXT.gamma_multiply(a)),
                                    ));
                                });
                            })
                            .response;
                        if resp.interact(egui::Sense::click()).clicked() {
                            dismiss = Some(idx);
                        }
                    }
                });
            });
        if let Some(i) = dismiss {
            self.toasts.remove(i);
        }
    }
}

impl eframe::App for App {
    /// Persist app state (window geometry is saved by eframe itself).
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "out_dir", &self.out_dir);
        eframe::set_value(storage, "recent_files", &self.recent);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut acts: Vec<Act> = Vec::new();
        if self.icons.is_none() {
            self.icons = Some(Icons::load(ctx));
        }
        let ic = self.icons.clone().unwrap();

        // Guard the window close against unsaved disk edits.
        let other_dirty = self.other.as_ref().is_some_and(|o| o.dirty);
        if ctx.input(|i| i.viewport().close_requested()) && (self.is_dirty() || other_dirty) {
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
                let mode = self.preview_mode;
                let dialect = self.basic_dialect;
                let stale = self
                    .hex_cache
                    .as_ref()
                    .map(|(ci, cm, cd, _)| *ci != i || *cm != mode || *cd != dialect)
                    .unwrap_or(true);
                if stale {
                    let text = match &self.nodes[i].data {
                        Ok(d) => match mode {
                            PreviewMode::Hex => hexdump(d, 16 * 1024),
                            PreviewMode::Text => petscii_text(d, 64 * 1024),
                            PreviewMode::Basic => basic_preview(d, dialect),
                            // Drawn from the bytes by the pane itself; this
                            // only marks that there is something to show.
                            PreviewMode::Gfx => "gfx".into(),
                        },
                        Err(e) => format!("(not extractable: {e})"),
                    };
                    self.hex_cache = Some((i, mode, dialect, text));
                }
            }
            _ => {
                self.hex_cache = None;
                self.gfx_cache = None;
            }
        }
        let preview = self
            .hex_cache
            .as_ref()
            .map(|(_, _, _, s)| s.clone())
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
        let mut preview_mode = self.preview_mode;
        let mut basic_dialect = self.basic_dialect;
        let mut gfx_kind = self.gfx_kind;
        let mut gfx_multi = self.gfx_multi;
        let mut gfx_cache = self.gfx_cache.take();
        let mut show_preview = self.show_preview;
        let mut show_help = self.show_help;
        let has_map = self.map_disk().is_some();
        let mut show_map = self.show_map && has_map;
        let mut show_copy = self.show_copy;

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
                    // Open Recent ▸ (newest first; disabled when empty)
                    ui.add_enabled_ui(!self.recent.is_empty(), |ui| {
                        egui::menu::menu_custom_button(
                            ui,
                            icon_button(&ic.folder, "Open Recent ▸"),
                            |ui| {
                                ui.set_min_width(220.0);
                                for (label, path) in recent_labels(&self.recent) {
                                    if ui.button(label).on_hover_text(&path).clicked() {
                                        acts.push(Act::OpenRecent(PathBuf::from(path)));
                                        ui.close_menu();
                                    }
                                }
                                ui.separator();
                                if ui.button("Clear list").clicked() {
                                    acts.push(Act::ClearRecent);
                                    ui.close_menu();
                                }
                            },
                        );
                    });
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
                    // Undo / Redo, with what they would undo or redo on hover.
                    let undo_next = self.undo.last().map(|s| s.label.clone());
                    let redo_next = self.redo.last().map(|s| s.label.clone());
                    let undo = ui
                        .add_enabled(
                            undo_next.is_some(),
                            egui::Button::new("Undo").shortcut_text("Ctrl+Z"),
                        )
                        .on_hover_text(undo_next.as_deref().unwrap_or("nothing to undo"));
                    if undo.clicked() {
                        acts.push(Act::Undo);
                        ui.close_menu();
                    }
                    let redo = ui
                        .add_enabled(
                            redo_next.is_some(),
                            egui::Button::new("Redo").shortcut_text("Ctrl+Y"),
                        )
                        .on_hover_text(redo_next.as_deref().unwrap_or("nothing to redo"));
                    if redo.clicked() {
                        acts.push(Act::Redo);
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
                }
                if has_map {
                    ui.separator();
                    if ui
                        .selectable_label(show_map, "Disk Map")
                        .on_hover_text("show which file owns each block (Ctrl+M)")
                        .clicked()
                    {
                        show_map = !show_map;
                    }
                }
                ui.separator();
                if ui
                    .selectable_label(show_copy, "Copy")
                    .on_hover_text("copy files to and from a second disk (Ctrl+K)")
                    .clicked()
                {
                    show_copy = !show_copy;
                }
                ui.separator();
                if ui
                    .selectable_label(show_preview, "Preview")
                    .on_hover_text("show or hide the preview pane")
                    .clicked()
                {
                    show_preview = !show_preview;
                }
                ui.separator();
                if ui.button("Help").clicked() {
                    show_help = true;
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

        // Preview pane — shown only when toggled on from the menu bar. All of
        // its functionality (HEX/TEXT toggle, scrolling) is unchanged.
        // Column widths for the listing, shared by the preview sizing below and
        // the list itself.
        let cols = Cols::of(&self.nodes);

        if show_preview {
            let pane_w = preview_width(ctx, preview_mode, &preview, preview_name.as_deref(), cols);
            egui::SidePanel::right("preview")
                .resizable(false)
                .exact_width(pane_w)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let header = match &preview_name {
                            Some(n) => format!("PREVIEW — {n}"),
                            None => "PREVIEW".to_string(),
                        };
                        // Hex / Text segmented toggle, right-aligned and laid
                        // out first, so the title is shortened to the space
                        // left beside it instead of running underneath.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.selectable_value(&mut preview_mode, PreviewMode::Gfx, "GFX")
                                .on_hover_text("draw the bytes as sprites or characters");
                            ui.selectable_value(&mut preview_mode, PreviewMode::Basic, "BASIC")
                                .on_hover_text("list a BASIC program, like LIST on a C64");
                            ui.selectable_value(&mut preview_mode, PreviewMode::Text, "TEXT")
                                .on_hover_text("show the bytes as PETSCII text");
                            ui.selectable_value(&mut preview_mode, PreviewMode::Hex, "HEX")
                                .on_hover_text("show an offset + hex + ASCII dump");
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(header).color(c64::ACCENT).strong(),
                                        )
                                        .truncate(),
                                    );
                                },
                            );
                        });
                    });
                    if preview_mode == PreviewMode::Basic {
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut basic_dialect, None, "AUTO")
                                .on_hover_text(
                                    "guess: BASIC 7.0 for C128 programs, Simons' BASIC when its \
                                 keywords are used, otherwise V2",
                                );
                            for (d, label, tip) in [
                                (BasicDialect::V2, "V2", "Commodore BASIC V2 (C64, VIC-20)"),
                                (BasicDialect::V7, "7.0", "the C128's BASIC 7.0"),
                                (BasicDialect::Simons, "SIMONS", "Simons' BASIC (C64)"),
                            ] {
                                ui.selectable_value(&mut basic_dialect, Some(d), label)
                                    .on_hover_text(tip);
                            }
                        });
                    }
                    if preview_mode == PreviewMode::Gfx {
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut gfx_kind, GfxKind::Sprites, "SPRITES")
                                .on_hover_text("24×21 sprites, 64 bytes each");
                            ui.selectable_value(&mut gfx_kind, GfxKind::Chars, "CHARS")
                                .on_hover_text("8×8 characters, 8 bytes each");
                            ui.separator();
                            ui.selectable_value(&mut gfx_multi, false, "HIRES")
                                .on_hover_text("one bit per pixel");
                            ui.selectable_value(&mut gfx_multi, true, "MULTI")
                                .on_hover_text("multicolour: two bits per double-wide pixel");
                        });
                    }
                    // Hex lines never wrap (the pane is sized to fit them, with
                    // a horizontal scrollbar if the window is too narrow);
                    // PETSCII text and BASIC listings wrap at the pane's 40
                    // columns, like a C64 screen.
                    let hex = preview_mode == PreviewMode::Hex;
                    egui::ScrollArea::new([hex, true])
                        .id_salt("preview_scroll")
                        .show(ui, |ui| {
                            if preview.is_empty() {
                                ui.label(
                                    egui::RichText::new("Select a file to preview its bytes.")
                                        .color(c64::TEXT_DIM),
                                );
                            } else if let (PreviewMode::Gfx, Some((i, Ok(data), badge))) = (
                                preview_mode,
                                self.selected.and_then(|i| {
                                    self.nodes.get(i).map(|n| (i, n.data.as_ref(), &n.badge))
                                }),
                            ) {
                                gfx_ui(ui, &mut gfx_cache, i, data, badge, gfx_kind, gfx_multi);
                            } else if preview_mode == PreviewMode::Basic {
                                let job = basic_layout(
                                    &preview,
                                    egui::TextStyle::Monospace.resolve(ui.style()),
                                    ui.visuals().text_color(),
                                    ui.available_width(),
                                );
                                ui.add(egui::Label::new(job));
                            } else {
                                let label =
                                    egui::Label::new(egui::RichText::new(&preview).monospace());
                                ui.add(if hex { label.extend() } else { label.wrap() });
                            }
                        });
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.nodes.is_empty() {
                let intro = "◆ SECTOR64\n\nOpen a disk or archive, or make a new image,\nthen drag files in to add them.";
                if self.recent.is_empty() || self.editable() || self.opened_path.is_some() {
                    ui.centered_and_justified(|ui| {
                        ui.label(egui::RichText::new(intro).color(c64::TEXT_DIM));
                    });
                } else {
                    // Start screen: the intro, then the recent files as links.
                    ui.vertical_centered(|ui| {
                        ui.add_space((ui.available_height() * 0.25).max(8.0));
                        ui.label(egui::RichText::new(intro).color(c64::TEXT_DIM));
                        ui.add_space(16.0);
                        ui.label(egui::RichText::new("RECENT FILES").color(c64::ACCENT));
                        ui.add_space(4.0);
                        for (label, path) in recent_labels(&self.recent) {
                            if ui.link(label).on_hover_text(&path).clicked() {
                                acts.push(Act::OpenRecent(PathBuf::from(path)));
                            }
                        }
                    });
                }
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
                        // Stable id so focus survives the count/clear widgets
                        // appearing before the field once it has text.
                        .id(egui::Id::new("filter_box"))
                        .hint_text("type to filter by name")
                        .desired_width(f32::INFINITY),
                );
            });
            ui.add_space(2.0);
            // Auto-pad columns to the widest row so they line up.
            // Column header (reserves the scrollbar width so it lines up with
            // the rows below), clickable to sort.
            let bar = ui.spacing().scroll.bar_width + 2.0;
            let disk_columns = is_disk || self.read_only_disk;
            if let Some(key) = header_row(ui, cols, disk_columns, bar, self.sort) {
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

        // Help / user manual window — a resizable, scrollable panel showing the
        // embedded MANUAL.md. Esc or the Close button dismisses it.
        if show_help {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                show_help = false;
            }
            let screen = ctx.screen_rect();
            egui::Window::new(
                egui::RichText::new("SECTOR64 — HELP")
                    .color(c64::ACCENT)
                    .strong(),
            )
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .default_height((screen.height() * 0.8).min(680.0))
            .max_height(screen.height() - 60.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("User manual").color(c64::TEXT_DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            show_help = false;
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("help_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(MANUAL).monospace())
                                .wrap_mode(egui::TextWrapMode::Extend),
                        );
                    });
            });
        }

        // Disk map window: every block, coloured by owner. Clicking a file's
        // block selects it in the listing.
        if show_map {
            if self.map_cache.is_none() {
                self.map_cache = self.map_disk().map(Disk::block_map);
            }
            if !show_help && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                show_map = false;
            }
            if let Some(map) = &self.map_cache {
                let screen = ctx.screen_rect();
                egui::Window::new(egui::RichText::new("DISK MAP").color(c64::ACCENT).strong())
                    .collapsible(false)
                    .resizable(true)
                    .default_width((screen.width() * 0.8).min(760.0))
                    .max_height(screen.height() - 60.0)
                    .pivot(egui::Align2::CENTER_CENTER)
                    .default_pos(screen.center())
                    .show(ctx, |ui| {
                        ui.horizontal_top(|ui| {
                            // The summary wraps in the space left of the button.
                            let w = (ui.available_width() - 90.0).max(100.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(w, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(map_summary(map))
                                                .color(c64::TEXT_DIM),
                                        )
                                        .wrap(),
                                    );
                                },
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                if ui.button("Close").clicked() {
                                    show_map = false;
                                }
                            });
                        });
                        ui.separator();
                        if let Some(i) = disk_map_ui(ui, map, sel_now) {
                            acts.push(Act::Select(i));
                        }
                    });
            }
        }

        // Copy window: a second disk to copy files to and from.
        if show_copy {
            if !show_help && !show_map && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                show_copy = false;
            }
            let screen = ctx.screen_rect();
            let chosen = self.chosen_rows().len();
            let can_receive = self.editable();
            egui::Window::new(egui::RichText::new("COPY").color(c64::ACCENT).strong())
                .collapsible(false)
                .resizable(true)
                .default_width(620.0_f32.min(screen.width() - 40.0))
                .min_width(560.0_f32.min(screen.width() - 40.0))
                .max_height(screen.height() - 60.0)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(screen.center())
                .show(ctx, |ui| {
                    copy_window_ui(
                        ui,
                        self.other.as_ref(),
                        chosen,
                        can_receive,
                        &mut show_copy,
                        &mut acts,
                    );
                });
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
                let quitting = matches!(self.confirm, Some(Confirm::Quit));
                let main_dirty = self.is_dirty();
                ui.label(match (main_dirty, quitting && other_dirty) {
                    (true, true) => {
                        "The open disk and the Copy window's disk have unsaved changes."
                    }
                    (false, true) => "The Copy window's disk has unsaved changes.",
                    _ => "The open disk has unsaved changes.",
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if (is_disk || quitting && other_dirty) && ui.button("💾 Save first").clicked()
                    {
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
                        if let Some(o) = &mut self.other {
                            o.dirty = false;
                        }
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    None => {}
                },
                Some(1) => self.confirm = None,
                Some(2) => {
                    if self.is_dirty() {
                        self.save(false);
                    }
                    let quitting = matches!(self.confirm, Some(Confirm::Quit));
                    if quitting && self.other.as_ref().is_some_and(|o| o.dirty) {
                        self.save_other(false);
                    }
                    let other_dirty = quitting && self.other.as_ref().is_some_and(|o| o.dirty);
                    if !self.is_dirty() && !other_dirty {
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

        // Drag-and-drop. With an editable disk or a new archive open, dropped
        // host files are added to it; otherwise the first dropped file is
        // opened as a disk image or archive.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if editable {
            for p in dropped {
                match std::fs::read(&p) {
                    Ok(bytes) => acts.push(Act::AddBytes(cbm_name_from(&p), bytes)),
                    Err(e) => acts.push(Act::Status(format!("Can't add {}: {e}", p.display()))),
                }
            }
        } else if let Some(first) = dropped.first() {
            acts.push(Act::OpenDropped(first.clone(), dropped.len() - 1));
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
            if i.modifiers.command && i.key_pressed(egui::Key::M) && has_map {
                show_map = !show_map;
            }
            if i.modifiers.command && i.key_pressed(egui::Key::K) {
                show_copy = !show_copy;
            }
            if !typing {
                // Undo / redo (text fields keep Ctrl+Z for their own text).
                if i.modifiers.command && i.key_pressed(egui::Key::Z) {
                    acts.push(if i.modifiers.shift {
                        Act::Redo
                    } else {
                        Act::Undo
                    });
                }
                if i.modifiers.command && i.key_pressed(egui::Key::Y) {
                    acts.push(Act::Redo);
                }
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
        self.preview_mode = preview_mode;
        self.basic_dialect = basic_dialect;
        self.gfx_kind = gfx_kind;
        self.gfx_multi = gfx_multi;
        self.gfx_cache = gfx_cache;
        self.show_preview = show_preview;
        self.show_help = show_help;
        self.show_map = show_map;
        self.show_copy = show_copy;
        for act in acts {
            self.apply(act);
        }

        // Surface any new status line as a transient toast, then draw the stack.
        let now = ctx.input(|i| i.time);
        self.pump_toasts(now);
        self.show_toasts(ctx);
    }
}

/// Fixed column widths (in characters) for the listing, computed once per
/// frame from the widest row so the columns line up.
#[derive(Clone, Copy)]
struct Cols {
    detail_chars: usize,
    size_digits: usize,
    /// Some row can't be extracted, so the size column must also fit the
    /// "⚠ can't extract" marker.
    has_errors: bool,
    /// The longest name in the listing.
    name_chars: usize,
}

/// The marker shown in the size column for a row whose bytes can't be decoded.
const CANT_EXTRACT: &str = "⚠ can't extract";

/// The fewest name characters a row keeps before the listing starts dropping
/// columns to make room.
const MIN_NAME_CHARS: usize = 8;

impl Cols {
    /// Column widths for a listing: the widest detail text and size.
    fn of(nodes: &[Node]) -> Cols {
        Cols {
            detail_chars: nodes
                .iter()
                .map(|n| n.detail.chars().count())
                .max()
                .unwrap_or(0),
            size_digits: nodes
                .iter()
                .filter_map(|n| n.data.as_ref().ok())
                .map(|d| d.len().to_string().len())
                .max()
                .unwrap_or(1)
                .max(1),
            has_errors: nodes.iter().any(|n| n.data.is_err()),
            name_chars: nodes
                .iter()
                .map(|n| n.name.chars().count())
                .max()
                .unwrap_or(0),
        }
    }

    /// Width of the size column in characters ("NNNNN BYTES", or the
    /// can't-extract marker if wider).
    fn size_chars(self) -> usize {
        let bytes = self.size_digits + 6;
        if self.has_errors {
            bytes.max(CANT_EXTRACT.chars().count())
        } else {
            bytes
        }
    }

    /// Characters a row needs for the type badge, a minimum-length name and
    /// the given optional columns, including the gaps between them.
    fn row_chars(self, detail: bool, size: bool) -> usize {
        self.row_chars_with_name(MIN_NAME_CHARS, detail, size)
    }

    /// As [`Cols::row_chars`], with room for a `name`-character name.
    fn row_chars_with_name(self, name: usize, detail: bool, size: bool) -> usize {
        let mut n = 6 + name;
        if detail && self.detail_chars > 0 {
            n += 1 + self.detail_chars + 2;
        } else if size {
            n += 2;
        }
        if size {
            n += self.size_chars();
        }
        n
    }
}

/// Where a listing row's columns go at a given width. Columns that don't fit
/// beside a [`MIN_NAME_CHARS`] name are dropped (detail first, then size),
/// so text never overlaps.
struct Placement {
    /// Left edge of the size column, if shown.
    status_x: Option<f32>,
    /// Left edge of the detail (BLK / PACKED) column, if shown.
    detail_x: Option<f32>,
    /// Right limit for the name.
    name_end: f32,
}

/// Lay out a row from `left` (the row's left edge, inside padding) to
/// `right_edge`. Header and rows call this with the same inputs, so they
/// always agree on which columns are shown.
fn place_cols(left: f32, right_edge: f32, cw: f32, cols: Cols) -> Placement {
    let avail = right_edge - left;
    let fits = |chars: usize| chars as f32 * cw <= avail;
    let size_w = cols.size_chars() as f32 * cw;
    if cols.detail_chars > 0 && fits(cols.row_chars(true, true)) {
        let status_x = right_edge - size_w;
        let detail_x = status_x - 2.0 * cw - cols.detail_chars as f32 * cw;
        Placement {
            status_x: Some(status_x),
            detail_x: Some(detail_x),
            name_end: detail_x - cw,
        }
    } else if fits(cols.row_chars(false, true)) {
        let status_x = right_edge - size_w;
        Placement {
            status_x: Some(status_x),
            detail_x: None,
            name_end: status_x - 2.0 * cw,
        }
    } else {
        Placement {
            status_x: None,
            detail_x: None,
            name_end: right_edge,
        }
    }
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
    let type_x = rect.left() + pad;
    let place = place_cols(type_x, right_edge, cw, cols);
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
    if let Some(detail_x) = place.detail_x {
        if is_disk {
            draw(&painter, detail_x, SortKey::Blocks, "BLK");
        } else {
            // Compressed archives: packed size and method (not sortable).
            painter.text(
                pos2(detail_x, cy),
                Align2::LEFT_CENTER,
                "PACKED",
                mono.clone(),
                c64::TEXT_DIM,
            );
        }
    }
    if let Some(status_x) = place.status_x {
        draw(&painter, status_x, SortKey::Size, "SIZE");
    }
    painter.hline(
        rect.x_range(),
        rect.bottom() - 1.0,
        Stroke::new(1.0, c64::ACCENT_DIM),
    );

    if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let past = |x: Option<f32>| x.is_some_and(|x| p.x >= x - cw);
            return Some(if past(place.status_x) {
                SortKey::Size
            } else if is_disk && past(place.detail_x) {
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
    // Columns are dropped (detail, then size) when the row is too narrow,
    // using the same decision as the header row.
    let right_edge = rect.right() - pad;
    let place = place_cols(rect.left() + pad, right_edge, cw, cols);

    if let Some(status_x) = place.status_x {
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
                    CANT_EXTRACT,
                    mono.clone(),
                    col,
                );
            }
        }
    }

    if let (Some(detail_x), false) = (place.detail_x, node.detail.is_empty()) {
        painter.text(
            pos2(detail_x, cy),
            Align2::LEFT_CENTER,
            &node.detail,
            mono.clone(),
            dim_col,
        );
    }

    // Name zone (left-aligned), or an inline editable field when renaming.
    let name_end = place.name_end;
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

    let max_chars = ((name_end - name_x) / cw).max(0.0) as usize;
    let name = if max_chars == 0 {
        String::new()
    } else if node.name.chars().count() > max_chars {
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

/// Columns of PETSCII text the preview shows per line: a C64 screen's 40.
const TEXT_PREVIEW_COLS: usize = 40;

/// One-line totals for the disk map's header.
fn map_summary(map: &BlockMap) -> String {
    let (free, file, sys, orphan, conflict) = map.counts();
    let total = free + file + sys + orphan + conflict;
    let mut s = format!("{total} blocks · {file} in files · {sys} DOS · {free} free");
    if orphan > 0 {
        s += &format!(" · {orphan} allocated but unused");
    }
    if conflict > 0 {
        s += &format!(" · {conflict} in conflict");
    }
    s
}

/// A disk-map block's fill colour: files by type (the selected one bright),
/// the DOS's blocks in the accent, free blocks dark.
fn block_color(map: &BlockMap, block: &sector64::MapBlock, selected: Option<usize>) -> Color32 {
    match block.status() {
        BlockStatus::Free if block.bam == BamState::Unknown => c64::STRIPE,
        BlockStatus::Free => c64::WELL,
        BlockStatus::File(i) if Some(i) == selected => c64::BRIGHT,
        BlockStatus::File(i) => badge_color(map.files[i].file_type.mnemonic()),
        BlockStatus::System => c64::ACCENT,
        BlockStatus::Orphan => c64::ORPHAN,
        BlockStatus::Conflict => c64::WARN,
    }
}

/// The hover text for one block of the disk map.
fn block_tooltip(map: &BlockMap, track: u8, sector: u8) -> String {
    let Some(b) = map.get(track, sector) else {
        return String::new();
    };
    let mut lines = vec![format!("Track {track}, sector {sector}")];
    let describe = |i: usize| {
        let e = &map.files[i];
        let chain = &map.chains[i];
        let pos = chain
            .iter()
            .position(|&x| x == (track, sector))
            .map_or(0, |p| p + 1);
        format!(
            "\"{}\" {} · block {pos} of {}",
            e.name,
            e.file_type.mnemonic().to_uppercase(),
            chain.len()
        )
    };
    match b.owner {
        Owner::File(i) => lines.push(describe(i)),
        Owner::System => lines.push("DOS: header, BAM or directory".into()),
        Owner::None => lines.push(
            match b.bam {
                BamState::Allocated => "Allocated in the BAM, but nothing uses it",
                BamState::Free => "Free",
                BamState::Unknown => "Unused (the BAM doesn't cover this track)",
            }
            .into(),
        ),
    }
    if b.shared {
        for (i, chain) in map.chains.iter().enumerate() {
            if b.owner != Owner::File(i) && chain.contains(&(track, sector)) {
                lines.push(format!("⚠ also used by {}", describe(i)));
            }
        }
    } else if b.owner != Owner::None && b.bam == BamState::Free {
        lines.push("⚠ in use, but free in the BAM: a new file could overwrite it".into());
    }
    lines.join("\n")
}

/// Draw the disk map: a legend, any chain faults, then a grid with a column
/// per track and a row per sector. Returns the file whose block was clicked.
fn disk_map_ui(ui: &mut egui::Ui, map: &BlockMap, selected: Option<usize>) -> Option<usize> {
    let tracks = map.tracks.len();
    let rows = map.tracks.iter().map(Vec::len).max().unwrap_or(0);
    if tracks == 0 || rows == 0 {
        ui.label("(this disk has no blocks to map)");
        return None;
    }

    // Legend.
    // Each swatch stays on the same line as its label when the legend wraps.
    let swatch = |ui: &mut egui::Ui, colors: &[Color32], text: &str| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for &c in colors {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 1.0, c);
            }
            ui.label(egui::RichText::new(text).color(c64::TEXT_DIM));
        });
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        swatch(
            ui,
            &[c64::PRG, c64::SEQ, c64::USR, c64::REL],
            "files by type",
        );
        swatch(ui, &[c64::BRIGHT], "selected");
        swatch(ui, &[c64::ACCENT], "DOS");
        swatch(ui, &[c64::WELL], "free");
        swatch(ui, &[c64::ORPHAN], "allocated, unused");
        swatch(ui, &[c64::WARN], "conflict");
    });
    for p in map.problems.iter().take(8) {
        ui.label(egui::RichText::new(format!("⚠ {p}")).color(c64::WARN));
    }
    if map.problems.len() > 8 {
        ui.label(
            egui::RichText::new(format!("… and {} more", map.problems.len() - 8)).color(c64::WARN),
        );
    }
    ui.add_space(4.0);

    // Cell size: fit the tracks across, within reason; tall CMD-native tracks
    // (256 sectors) get short rows and scroll.
    let font = egui::FontId::monospace(10.0);
    let (label_w, head_h) = (24.0, 14.0);
    let fit = ((ui.available_width() - label_w - 16.0) / tracks as f32).floor();
    let cell_w = fit.clamp(6.0, 18.0);
    let cell_h = if rows > 64 { 6.0 } else { cell_w.min(14.0) };
    let gap = if cell_w >= 8.0 { 1.0 } else { 0.0 };
    // Label every track when the widest number fits, else every 2nd, 5th or
    // 10th (the C64 font is wide).
    let label_px = ui.fonts(|f| f.glyph_width(&font, '0')) * tracks.to_string().len() as f32 + 4.0;
    let every = [1usize, 2, 5, 10]
        .into_iter()
        .find(|&k| cell_w * k as f32 >= label_px)
        .unwrap_or(10);
    let row_every = if rows > 64 { 16 } else { 5 };

    let mut clicked = None;
    egui::ScrollArea::both()
        .id_salt("disk_map_scroll")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            // Room on the right for half of the last track's label.
            let size = egui::vec2(
                label_w + tracks as f32 * cell_w + label_px / 2.0,
                head_h + rows as f32 * cell_h,
            );
            let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
            let painter = ui.painter_at(rect);
            let origin = rect.min + egui::vec2(label_w, head_h);
            let cell_at = |t: usize, s: usize| {
                egui::Rect::from_min_size(
                    origin + egui::vec2(t as f32 * cell_w, s as f32 * cell_h),
                    egui::vec2(cell_w - gap, cell_h - gap),
                )
            };

            // Track 1 is labelled too, unless it would crowd track 2's label.
            for t in (1..=tracks).filter(|&t| t % every == 0 || (t == 1 && every != 2)) {
                let x = origin.x + (t as f32 - 0.5) * cell_w;
                painter.text(
                    egui::pos2(x, rect.min.y),
                    egui::Align2::CENTER_TOP,
                    t.to_string(),
                    font.clone(),
                    c64::TEXT_DIM,
                );
            }
            for s in (0..rows).step_by(row_every) {
                let y = origin.y + (s as f32 + 0.5) * cell_h;
                painter.text(
                    egui::pos2(origin.x - 4.0, y),
                    egui::Align2::RIGHT_CENTER,
                    s.to_string(),
                    font.clone(),
                    c64::TEXT_DIM,
                );
            }

            let hovered = resp.hover_pos().and_then(|p| {
                let (dx, dy) = (p.x - origin.x, p.y - origin.y);
                if dx < 0.0 || dy < 0.0 {
                    return None;
                }
                let (t, s) = ((dx / cell_w) as usize, (dy / cell_h) as usize);
                map.tracks.get(t)?.get(s)?;
                Some((t as u8 + 1, s as u8))
            });
            let owner_of = |(t, s): (u8, u8)| match map.get(t, s).map(|b| b.owner) {
                Some(Owner::File(i)) => Some(i),
                _ => None,
            };
            let hovered_file = hovered.and_then(owner_of);

            for (t, row) in map.tracks.iter().enumerate() {
                for (s, b) in row.iter().enumerate() {
                    painter.rect_filled(cell_at(t, s), 1.0, block_color(map, b, selected));
                }
            }
            // Outline the hovered file's whole chain, so its spread shows.
            if let Some(i) = hovered_file {
                for &(t, s) in &map.chains[i] {
                    let r = cell_at(t as usize - 1, s as usize).expand(0.5);
                    painter.rect_stroke(r, 1.0, egui::Stroke::new(1.5, c64::BRIGHT));
                }
            }
            if let Some((t, s)) = hovered {
                let r = cell_at(t as usize - 1, s as usize).expand(1.0);
                painter.rect_stroke(r, 1.0, egui::Stroke::new(2.0, c64::BRIGHT));
            }

            if resp.clicked() {
                clicked = hovered_file;
            }
            if let Some((t, s)) = hovered {
                resp.on_hover_text_at_pointer(block_tooltip(map, t, s));
            }
        });
    clicked
}

/// Width of the preview pane: wide enough for its content (a full hex line,
/// or 40 columns of text) and its header, but never so wide that the file
/// list loses columns it would otherwise show. If the window is too narrow
/// for both, the pane gives way first, down to a floor.
fn preview_width(
    ctx: &egui::Context,
    mode: PreviewMode,
    preview: &str,
    name: Option<&str>,
    cols: Cols,
) -> f32 {
    let mono = egui::FontId::monospace(16.0);
    let cw = ctx.fonts(|f| f.glyph_width(&mono, '0')).max(6.0);
    let style = ctx.style();
    let scroll = &style.spacing.scroll;
    let bar = scroll.bar_width + scroll.bar_inner_margin + scroll.bar_outer_margin;
    // Panel frame margins on both sides, plus a little slack.
    let chrome = 2.0 * 8.0 + bar + 8.0;

    let content_chars = match mode {
        PreviewMode::Hex => preview
            .lines()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0),
        PreviewMode::Text | PreviewMode::Basic | PreviewMode::Gfx => TEXT_PREVIEW_COLS,
    };
    // "PREVIEW — NAME" plus the HEX / TEXT / BASIC / GFX toggle.
    let header_chars = "PREVIEW — ".chars().count() + name.map_or(0, |n| n.chars().count()) + 22;
    let want = content_chars.max(header_chars) as f32 * cw + chrome;

    // Leave the list beside it room for its longest name and all its
    // columns if possible, but keep at least ~20 characters of preview;
    // below that the list shortens names, then drops columns (`place_cols`).
    let screen = ctx.screen_rect().width();
    let list_chrome = 2.0 * 8.0 + 2.0 * 8.0 + bar + 8.0;
    let name = cols.name_chars.max(MIN_NAME_CHARS);
    let list_full = cols.row_chars_with_name(name, true, true) as f32 * cw + list_chrome;
    let floor = 20.0 * cw + chrome;
    want.min(screen - list_full).max(floor.min(want))
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

/// Render bytes as PETSCII text for the preview pane. CR/LF become line
/// breaks; printable ASCII passes through (the C64 font draws it as PETSCII);
/// PETSCII uppercase ($C1-$DA) and shifted space ($A0) are folded to their
/// base glyph; everything else shows as `.`. Good for reading SEQ text files.
fn petscii_text(data: &[u8], cap: usize) -> String {
    let n = data.len().min(cap);
    let mut s = String::with_capacity(n + 64);
    for &b in &data[..n] {
        let c = match b {
            0x0D | 0x0A | 0x8D => '\n',
            0x09 => '\t',
            0xA0 => ' ',
            0x20..=0x7E => b as char,
            0xC1..=0xDA => (b - 0x80) as char, // shifted/upper PETSCII letters
            _ => '.',
        };
        s.push(c);
    }
    if data.len() > n {
        s.push_str(&format!("\n… ({} of {} bytes shown)", n, data.len()));
    }
    s
}

/// Screen colours for the GFX preview's colour roles: black background,
/// white for the cell's own colour, light red and yellow for the two shared
/// multicolours, and the pane showing through the gutters.
fn gfx_color(role: u8) -> Color32 {
    match role {
        sector64::gfx::BG => Color32::BLACK,
        sector64::gfx::MC1 => Color32::from_rgb(0xb8, 0x69, 0x62),
        sector64::gfx::MC2 => Color32::from_rgb(0xbf, 0xce, 0x72),
        sector64::gfx::FG => Color32::WHITE,
        _ => Color32::TRANSPARENT,
    }
}

/// Draw `data` as a sheet of `kind` cells. A PRG's cells line up with the
/// memory it loads to, like the VIC-II sees them; other files start at
/// byte 0. At most `max_cells` are drawn.
fn gfx_sheet(
    data: &[u8],
    is_prg: bool,
    kind: GfxKind,
    multi: bool,
    cols: usize,
    max_cells: usize,
) -> (GfxSheet, Option<u16>, usize) {
    use sector64::gfx;
    let load = (is_prg && data.len() >= 2).then(|| u16::from_le_bytes([data[0], data[1]]));
    let start = gfx::first_cell(load, kind);
    let total = gfx::cell_count(data.len(), start, kind);
    (
        gfx::render(data, kind, multi, start, cols, max_cells),
        load,
        total,
    )
}

/// Hover text for cell `i`: its number, file offset and, for a PRG, the
/// address it loads to with the sprite pointer or character code that shows
/// it (within its 16K VIC bank or 2K character set).
fn gfx_cell_info(sheet: &GfxSheet, load: Option<u16>, i: usize) -> String {
    let off = sheet.offset_of(i);
    let mut s = format!("{} {i} · offset ${off:04X}", sheet.kind.noun());
    if let Some(load) = load {
        let addr = (load as usize + off - 2) & 0xffff;
        s.push_str(&format!("\nloads at ${addr:04X}"));
        match sheet.kind {
            GfxKind::Sprites => {
                s.push_str(&format!(" · sprite pointer ${:02X}", (addr % 0x4000) / 64))
            }
            GfxKind::Chars => s.push_str(&format!(" · screen code ${:02X}", (addr % 0x800) / 8)),
        }
    }
    s
}

/// The GFX preview: the sheet scaled up to fill the pane, a summary line, and
/// each cell's details on hover. `cache` keeps the texture between frames.
#[allow(clippy::too_many_arguments)]
fn gfx_ui(
    ui: &mut egui::Ui,
    cache: &mut Option<GfxView>,
    node: usize,
    data: &[u8],
    badge: &str,
    kind: GfxKind,
    multi: bool,
) {
    let avail = ui.available_width().max(1.0);
    let (cw, ch) = kind.cell_size();
    // Sprites fill the pane at 2× (or more); characters sit 16 to a row like
    // a character set, when the pane is wide enough.
    let cols = match kind {
        GfxKind::Sprites => ((avail + 1.0) / (2.0 * (cw + 1) as f32)).floor().max(1.0) as usize,
        GfxKind::Chars => (((avail + 1.0) / (ch + 1) as f32).floor() as usize).clamp(1, 16),
    };
    let key = (node, kind, multi, cols);
    if cache.as_ref().map(|c| c.key) != Some(key) {
        // Keep the texture within what the GPU accepts.
        let max_side = ui.ctx().input(|i| i.max_texture_side).max(256);
        let max_cells = (max_side / (ch + 1)) * cols;
        let (sheet, load, total) = gfx_sheet(data, badge == "prg", kind, multi, cols, max_cells);
        let img = egui::ColorImage {
            size: [sheet.width, sheet.height],
            pixels: sheet.pixels.iter().map(|&r| gfx_color(r)).collect(),
        };
        let tex = ui
            .ctx()
            .load_texture("gfx-preview", img, egui::TextureOptions::NEAREST);
        *cache = Some(GfxView {
            key,
            sheet,
            tex,
            load,
            total,
        });
    }
    let view = cache.as_ref().unwrap();
    let sheet = &view.sheet;
    let noun = kind.noun();
    let mut summary = match (view.total, view.load) {
        (0, _) => format!("(too short for a {noun})"),
        (n, Some(load)) => format!(
            "{n} {noun}{} from ${:04X}",
            if n == 1 { "" } else { "s" },
            (load as usize + sheet.start - 2) & 0xffff
        ),
        (n, None) => format!("{n} {noun}{}", if n == 1 { "" } else { "s" }),
    };
    if sheet.count < view.total {
        summary.push_str(&format!(" (first {} shown)", sheet.count));
    }
    ui.label(egui::RichText::new(summary).color(c64::TEXT_DIM));
    if sheet.count == 0 {
        return;
    }
    let scale = ((avail + 1.0) / (sheet.width + 1) as f32)
        .floor()
        .clamp(1.0, 4.0);
    let size = egui::vec2(sheet.width as f32, sheet.height as f32) * scale;
    let resp = ui.add(egui::Image::new((view.tex.id(), size)).sense(egui::Sense::hover()));
    if let Some(pos) = resp.hover_pos() {
        let p = (pos - resp.rect.min) / scale;
        if let Some(i) = sheet.cell_at(p.x.max(0.0) as usize, p.y.max(0.0) as usize) {
            resp.on_hover_text_at_pointer(gfx_cell_info(sheet, view.load, i));
        }
    }
}

/// The BASIC preview: the program listed as `LIST` shows it, or a note when
/// the file isn't BASIC. `data` is the whole file, load address included;
/// `dialect` is the one to list in, or `None` to detect it (a detected
/// BASIC 7.0 or Simons' BASIC program says so on its first line).
fn basic_preview(data: &[u8], dialect: Option<BasicDialect>) -> String {
    let listing = match dialect {
        Some(d) => sector64::detokenize_basic_as(data, d),
        None => sector64::detokenize_basic(data),
    };
    let Some(l) = listing else {
        return "(not a BASIC program; try HEX or TEXT)".into();
    };
    let mut out = String::new();
    if dialect.is_none() && l.dialect != BasicDialect::V2 {
        out.push_str(&format!("(listed as {})\n", l.dialect.name()));
    }
    out.push_str(&l.text);
    if l.trailing_bytes > 0 {
        out.push_str(&format!(
            "\n(followed by {} bytes of machine code or data)",
            l.trailing_bytes
        ));
    }
    out
}

/// Lay out a BASIC listing: line numbers in the accent colour so each line's
/// start stands out, notes in parentheses dimmed, and long lines broken at
/// any character (as on a C64 screen) rather than only at spaces.
fn basic_layout(
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    job.wrap.break_anywhere = true;
    let fmt = |c| egui::TextFormat::simple(font.clone(), c);
    for line in text.split_inclusive('\n') {
        let digits = line.bytes().take_while(u8::is_ascii_digit).count();
        if line.starts_with('(') {
            job.append(line, 0.0, fmt(c64::TEXT_DIM));
        } else if digits > 0 {
            job.append(&line[..digits], 0.0, fmt(c64::ACCENT));
            job.append(&line[digits..], 0.0, fmt(color));
        } else {
            job.append(line, 0.0, fmt(color));
        }
    }
    job
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
    /// A disk shown as a listing only (title, rows with block counts, and the
    /// disk itself for the map).
    ReadOnlyDisk(String, Vec<Node>, Disk),
    /// An archive or container listing (title, rows).
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
    (disk_title(disk, header), nodes)
}

/// A disk's header line, as the listing's title shows it.
fn disk_title(disk: &Disk, header: sector64::Result<sector64::Header>) -> String {
    let free = disk
        .blocks_free()
        .map(|f| format!("{f} free"))
        .unwrap_or_default();
    match header {
        Ok(h) => format!("Disk  0 \"{}\" {} {}   {}", h.name, h.id, h.dos_type, free),
        Err(e) => format!("Disk (header error: {e})"),
    }
}

/// The Copy window's contents: the other disk's header, its files, and the
/// buttons that open, make, save it and copy files across.
fn copy_window_ui(
    ui: &mut egui::Ui,
    other: Option<&OtherDisk>,
    chosen: usize,
    can_receive: bool,
    show: &mut bool,
    acts: &mut Vec<Act>,
) {
    let dirty = other.is_some_and(|o| o.dirty);
    ui.horizontal(|ui| {
        let open_tip = if dirty {
            "save or discard this disk's changes first"
        } else {
            "open a D64, D71, D81 or CMD-native D2M to copy to"
        };
        if ui
            .add_enabled(!dirty, egui::Button::new("Open…"))
            .on_hover_text(open_tip)
            .on_disabled_hover_text(open_tip)
            .clicked()
        {
            acts.push(Act::OtherOpen);
        }
        ui.add_enabled_ui(!dirty, |ui| {
            ui.menu_button("New ▸", |ui| {
                for (fmt, label) in [(0u8, "D64 (1541)"), (1, "D71 (1571)"), (2, "D81 (1581)")] {
                    if ui.button(label).clicked() {
                        acts.push(Act::OtherNew(fmt));
                        ui.close_menu();
                    }
                }
            })
            .response
            .on_hover_text("a blank disk, named like File ▸ New")
            .on_disabled_hover_text(open_tip);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("Close")
                .on_hover_text("hide this window (Esc); the disk stays open")
                .clicked()
            {
                *show = false;
            }
        });
    });
    let Some(o) = other else {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Open a disk, or make a new one, to copy files to and from.\n\
                 Then select files in the main list and press Copy Here.",
            )
            .color(c64::TEXT_DIM),
        );
        return;
    };
    ui.separator();
    let title = disk_title(&o.disk, o.disk.header());
    let entries = o.disk.directory().unwrap_or_default();
    ui.add(egui::Label::new(egui::RichText::new(title).color(c64::BRIGHT).strong()).truncate());
    // Unsaved changes are flagged first, so a long path can't hide it.
    let where_ = match (&o.path, dirty) {
        (None, _) => "(not saved yet)".to_string(),
        (Some(p), true) => format!("•modified  {}", p.display()),
        (Some(p), false) => p.display().to_string(),
    };
    ui.add(egui::Label::new(egui::RichText::new(where_).color(c64::TEXT_DIM)).truncate());
    ui.horizontal(|ui| {
        if ui.add_enabled(dirty, egui::Button::new("Save")).clicked() {
            acts.push(Act::OtherSave(false));
        }
        if ui.button("Save As…").clicked() {
            acts.push(Act::OtherSave(true));
        }
        if ui
            .add_enabled(!o.undo.is_empty(), egui::Button::new("Undo"))
            .on_hover_text("undo the last copy onto this disk")
            .clicked()
        {
            acts.push(Act::OtherUndo);
        }
        let close = if dirty { "Discard" } else { "Close Disk" };
        if ui
            .button(close)
            .on_hover_text(if dirty {
                "close this disk and lose its unsaved changes"
            } else {
                "close this disk"
            })
            .clicked()
        {
            acts.push(Act::OtherDiscard);
        }
    });
    ui.separator();
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let list_h = (ui.available_height() - 60.0).max(120.0);
    egui::ScrollArea::vertical()
        .id_salt("other_list")
        .max_height(list_h)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if entries.is_empty() {
                ui.label(egui::RichText::new("(no files)").color(c64::TEXT_DIM));
            }
            for (i, e) in entries.iter().enumerate() {
                // A directory line, as LIST "$" shows it.
                let badge = e.file_type.mnemonic();
                let text = format!(
                    "{:<5}{:<19}{}",
                    e.blocks,
                    format!("\"{}\"", e.name),
                    badge.to_uppercase()
                );
                let r = ui.add(egui::SelectableLabel::new(
                    o.marked.contains(&i),
                    egui::RichText::new(text)
                        .font(font.clone())
                        .color(badge_color(badge)),
                ));
                if r.clicked() {
                    let toggle = ui.input(|i| i.modifiers.command);
                    acts.push(Act::OtherMark(i, toggle));
                }
            }
        });
    ui.separator();
    ui.horizontal(|ui| {
        let here = match chosen {
            0 => "Copy Here".to_string(),
            n => format!("Copy Here ({n})"),
        };
        if ui
            .add_enabled(chosen > 0, egui::Button::new(here))
            .on_hover_text("copy the files selected in the main list onto this disk")
            .on_disabled_hover_text("select files in the main list first")
            .clicked()
        {
            acts.push(Act::CopyToOther);
        }
        let n = o.marked.len();
        let back = match n {
            0 => "Copy to Open Disk".to_string(),
            n => format!("Copy to Open Disk ({n})"),
        };
        let tip = if !can_receive {
            "open an editable disk (or start a new archive) in the main window first"
        } else {
            "select files above first (Ctrl-click for several)"
        };
        if ui
            .add_enabled(n > 0 && can_receive, egui::Button::new(back))
            .on_hover_text("copy the files selected above onto the disk open in the main window")
            .on_disabled_hover_text(tip)
            .clicked()
        {
            acts.push(Act::CopyFromOther);
        }
    });
}

/// Write an archive member (or any listed file without a directory entry)
/// onto `disk` under its listed name, refusing names already there and REL
/// files.
fn copy_bytes(disk: &mut Disk, node: &Node) -> Result<(), String> {
    let data = node.data.as_ref().map_err(|e| e.clone())?;
    let file_type = match node.badge.as_str() {
        "rel" => return Err(sector64::Error::CannotCopy("REL files").to_string()),
        "seq" => FileType::Seq,
        "usr" => FileType::Usr,
        "del" => FileType::Del,
        _ => FileType::Prg,
    };
    if disk.find_entry(&node.name).ok().flatten().is_some() {
        return Err(sector64::Error::FileExists.to_string());
    }
    disk.write_file(&node.name, file_type, data)
        .map_err(|e| e.to_string())
}

/// The status line after a copy: how many went to `dest`, and why any
/// didn't.
fn copy_report(done: &[String], failed: &[String], dest: &str) -> String {
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
fn default_disk_name(disk: &Disk) -> &'static str {
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
fn zipcode_set(path: &Path) -> Option<Result<Vec<u8>, String>> {
    let file = path.file_name()?.to_str()?;
    let rest = match file.as_bytes() {
        [b'1'..=b'4', b'!', ..] if file.len() > 2 => &file[2..],
        _ => return None,
    };
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut parts = Vec::with_capacity(4);
    for n in 1..=4 {
        let p = dir.join(format!("{n}!{rest}"));
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
fn open_path(path: &Path) -> Result<Opened, String> {
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

    let import_nodes = |files: Vec<sector64::ImportFile>| {
        files
            .into_iter()
            .map(|f| Node {
                badge: f.file_type.mnemonic().to_string(),
                detail: String::new(), // size is in the SIZE column
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
            let entries = parse(&bytes).map_err(|e| e.to_string())?;
            let details = packed_details(
                entries
                    .iter()
                    .map(|e| (e.packed.len(), e.method.label().to_string())),
            );
            let nodes = entries
                .into_iter()
                .zip(details)
                .map(|(e, detail)| Node {
                    badge: e.file_type.mnemonic().to_string(),
                    detail,
                    blocks: 0,
                    data: e.decode().map_err(|x| x.to_string()),
                    name: e.name,
                })
                .collect();
            ("ARC/SDA archive", nodes)
        }
        "lha" | "lzh" | "sfx" => {
            let entries = parse_lha(&bytes).map_err(|e| e.to_string())?;
            let details = packed_details(
                entries
                    .iter()
                    .map(|e| (e.compressed_size as usize, e.method_str().to_string())),
            );
            let nodes = entries
                .into_iter()
                .zip(details)
                .map(|(e, detail)| Node {
                    badge: String::new(),
                    detail,
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
                    detail: String::new(), // size is in the SIZE column
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

/// Whether `SECTOR64_X11` asks for the X11 back-end: set to anything except
/// empty or "0".
#[cfg(any(target_os = "linux", test))]
fn force_x11(v: Option<&std::ffi::OsStr>) -> bool {
    v.is_some_and(|v| !v.is_empty() && v != "0")
}

/// How many files File ▸ Open Recent remembers.
const RECENT_MAX: usize = 10;

/// Put `path` at the front of the recent-files list, dropping any older
/// entry for the same file and keeping at most `RECENT_MAX`.
fn remember_recent(list: &mut Vec<String>, path: &Path) {
    let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let full = full.display().to_string();
    list.retain(|p| *p != full);
    list.insert(0, full);
    list.truncate(RECENT_MAX);
}

/// Menu labels for the recent files: the file name, with its folder added
/// when two entries share a name. Returns `(label, full path)` pairs.
fn recent_labels(list: &[String]) -> Vec<(String, String)> {
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

fn main() -> eframe::Result<()> {
    let initial = std::env::args().nth(1);
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("sector64-gui")
            .with_inner_size([960.0, 640.0])
            .with_icon(std::sync::Arc::new(make_icon())),
        persist_window: true,
        // See gui/Cargo.toml: wgpu on Windows, OpenGL (glow) everywhere else.
        #[cfg(windows)]
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    // Linux: native Wayland by default. SECTOR64_X11=1 runs the window
    // through X11 (XWayland on a Wayland desktop) instead, because winit 0.30
    // only delivers drag-and-drop on X11.
    #[cfg(target_os = "linux")]
    if force_x11(std::env::var_os("SECTOR64_X11").as_deref()) {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::x11::EventLoopBuilderExtX11 as _;
            builder.with_x11();
        }));
    }
    eframe::run_native(
        "Sector64",
        options,
        Box::new(move |cc| {
            load_c64_font(&cc.egui_ctx);
            apply_c64_theme(&cc.egui_ctx);
            Ok(Box::new(App::new(initial, cc.storage)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `f` inside one egui frame of a `width`-wide screen, so font
    /// metrics and the screen size are available.
    fn in_frame<R>(width: f32, f: impl FnOnce(&egui::Context) -> R) -> R {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 700.0),
            )),
            ..Default::default()
        };
        let mut out = None;
        let mut f = Some(f);
        let _ = ctx.run(input, |ctx| {
            if let Some(f) = f.take() {
                out = Some(f(ctx));
            }
        });
        out.unwrap()
    }

    fn sample_cols() -> [Cols; 3] {
        let disk = Cols {
            detail_chars: 7, // "162 BLK"
            size_digits: 5,
            has_errors: false,
            name_chars: 16,
        };
        let arc = Cols {
            detail_chars: 22, // "19374 BYTES  huffman+rle"
            size_digits: 5,
            has_errors: false,
            name_chars: 16,
        };
        let broken = Cols {
            detail_chars: 0,
            size_digits: 2,
            has_errors: true,
            name_chars: 16,
        };
        [disk, arc, broken]
    }

    #[test]
    fn list_columns_never_overlap_at_any_width() {
        let cw = 10.0;
        let left = 8.0;
        for cols in sample_cols() {
            let mut shown_before = 0;
            for w in (0..2000).step_by(5) {
                let right = left + w as f32;
                let p = place_cols(left, right, cw, cols);
                let mut shown = 0;
                if let Some(sx) = p.status_x {
                    shown += 1;
                    assert!(sx + cols.size_chars() as f32 * cw <= right + 0.01);
                    if let Some(dx) = p.detail_x {
                        shown += 1;
                        assert!(dx + cols.detail_chars as f32 * cw <= sx);
                        assert!(p.name_end <= dx);
                    } else {
                        assert!(p.name_end <= sx);
                    }
                    // A column is only shown with a minimum-length name
                    // beside it (after the 6-char type badge).
                    assert!(p.name_end - (left + 6.0 * cw) >= MIN_NAME_CHARS as f32 * cw - 0.01);
                } else {
                    assert!(p.detail_x.is_none());
                    assert!(p.name_end <= right);
                }
                // Widening the row never hides a column.
                assert!(
                    shown >= shown_before,
                    "width {w}: columns went {shown_before} → {shown}"
                );
                shown_before = shown;
            }
            assert_eq!(shown_before, if cols.detail_chars > 0 { 2 } else { 1 });
        }
    }

    #[test]
    fn preview_pane_fits_hex_lines_and_gives_way_to_the_list() {
        let dump = hexdump(&[0x41; 64], 64);
        let line = dump.lines().map(|l| l.chars().count()).max().unwrap();
        for cols in sample_cols() {
            let mut last = 0.0;
            for screen in (400..3000).step_by(20) {
                let screen = screen as f32;
                let (pane, cw) = in_frame(screen, |ctx| {
                    let cw = ctx.fonts(|f| f.glyph_width(&egui::FontId::monospace(16.0), '0'));
                    (
                        preview_width(ctx, PreviewMode::Hex, &dump, Some("STARFALL.GAME"), cols),
                        cw,
                    )
                });
                let want_min = line as f32 * cw;
                let floor = 20.0 * cw;
                // Never wider than a full hex line needs (plus chrome).
                assert!(pane <= want_min + 120.0, "screen {screen}: pane {pane}");
                // Never narrower than the floor.
                assert!(
                    pane >= floor,
                    "screen {screen}: pane {pane} < floor {floor}"
                );
                // Wide screens: the whole hex line fits.
                let list_full = cols.row_chars_with_name(16, true, true) as f32 * cw + 80.0;
                if screen >= want_min + 120.0 + list_full {
                    assert!(
                        pane >= want_min,
                        "screen {screen}: pane {pane} < line {want_min}"
                    );
                }
                // Widening the window never shrinks the pane.
                assert!(pane >= last);
                last = pane;
            }
        }
    }

    #[test]
    fn text_preview_is_forty_columns_wide() {
        let cols = sample_cols()[0];
        let (pane, cw) = in_frame(2400.0, |ctx| {
            let cw = ctx.fonts(|f| f.glyph_width(&egui::FontId::monospace(16.0), '0'));
            (
                preview_width(ctx, PreviewMode::Text, "HELLO", Some("README"), cols),
                cw,
            )
        });
        assert!(pane >= TEXT_PREVIEW_COLS as f32 * cw);
        assert!(pane <= (TEXT_PREVIEW_COLS + 12) as f32 * cw);
    }

    #[test]
    #[cfg(unix)] // the expected strings are Unix absolute paths
    fn remember_recent_dedupes_and_caps() {
        let mut list = Vec::new();
        for i in 0..12 {
            remember_recent(&mut list, Path::new(&format!("/disks/{i}.d64")));
        }
        assert_eq!(list.len(), RECENT_MAX);
        assert_eq!(list[0], "/disks/11.d64");
        assert_eq!(list[RECENT_MAX - 1], "/disks/2.d64");
        // Re-opening an older file moves it to the front, once.
        remember_recent(&mut list, Path::new("/disks/5.d64"));
        assert_eq!(list[0], "/disks/5.d64");
        assert_eq!(list.iter().filter(|p| *p == "/disks/5.d64").count(), 1);
        assert_eq!(list.len(), RECENT_MAX);
        // Relative paths are stored absolute, so they still work from
        // another working directory.
        remember_recent(&mut list, Path::new("rel.d64"));
        assert!(Path::new(&list[0]).is_absolute());
    }

    #[test]
    fn recent_labels_add_the_folder_only_for_clashing_names() {
        let list: Vec<String> = ["/a/games/disk.d64", "/a/tools/disk.d64", "/a/x/demo.lnx"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let labels: Vec<String> = recent_labels(&list).into_iter().map(|(l, _)| l).collect();
        assert_eq!(
            labels,
            ["disk.d64  (games)", "disk.d64  (tools)", "demo.lnx"]
        );
        assert_eq!(recent_labels(&list)[1].1, "/a/tools/disk.d64");
    }

    #[test]
    fn gfx_preview_aligns_prg_cells_to_memory() {
        // A PRG loading at $2FFF: the first sprite boundary is $3000, one
        // byte in, which is sprite pointer $C0 in its VIC bank.
        let mut prg = vec![0xff, 0x2f, 0xaa];
        prg.extend(std::iter::repeat_n(0x80, 64 * 2));
        let (sheet, load, total) = gfx_sheet(&prg, true, GfxKind::Sprites, false, 8, 100);
        assert_eq!(
            (load, sheet.start, total, sheet.count),
            (Some(0x2fff), 3, 2, 2)
        );
        assert_eq!(
            gfx_cell_info(&sheet, load, 1),
            "sprite 1 · offset $0043\nloads at $3040 · sprite pointer $C1"
        );
        // The same bytes as a SEQ file start at byte 0 and have no address.
        let (seq, load, _) = gfx_sheet(&prg, false, GfxKind::Chars, true, 16, 3);
        assert_eq!((load, seq.start, seq.count), (None, 0, 3));
        assert_eq!(gfx_cell_info(&seq, None, 2), "char 2 · offset $0010");
        let (chars, load, _) = gfx_sheet(&prg, true, GfxKind::Chars, false, 16, 100);
        assert_eq!(
            gfx_cell_info(&chars, load, 0),
            "char 0 · offset $0003\nloads at $3000 · screen code $00"
        );
        assert_eq!(gfx_color(sector64::gfx::GAP), Color32::TRANSPARENT);
    }

    #[test]
    fn basic_preview_lists_programs_and_flags_the_rest() {
        // 10 PRINT "HI"  (link $080D), end marker, then 2 bytes of code.
        let prg = [
            0x01, 0x08, 0x0D, 0x08, 0x0A, 0x00, 0x99, 0x20, 0x22, 0x48, 0x49, 0x22, 0x00, 0x00,
            0x00, 0xEA, 0x60,
        ];
        assert_eq!(
            basic_preview(&prg, None),
            "10 PRINT \"HI\"\n\n(followed by 2 bytes of machine code or data)"
        );
        assert_eq!(basic_preview(&prg[..15], None), "10 PRINT \"HI\"\n");
        assert!(basic_preview(b"\x00\xc0\xa9\x00\x60", None).starts_with("(not a BASIC program"));

        // 10 HIRES 0 in Simons' BASIC: detected and labelled, or listed as
        // chosen.
        let simons = [
            0x01, 0x08, 0x0C, 0x08, 0x0A, 0x00, 0x64, 0x01, 0x20, 0x30, 0x00, 0x00, 0x00,
        ];
        assert_eq!(
            basic_preview(&simons, None),
            "(listed as Simons' BASIC)\n10 HIRES 0\n"
        );
        assert_eq!(
            basic_preview(&simons, Some(BasicDialect::Simons)),
            "10 HIRES 0\n"
        );
        assert_eq!(
            basic_preview(&simons, Some(BasicDialect::V2)),
            "10 {$64}{$01} 0\n"
        );
    }

    /// An app with a blank, saved D64 open (as if loaded from a file).
    fn app_with_disk() -> App {
        let mut app = App {
            disk: Some(EditDisk {
                disk: Disk::format("UNDO TEST", "UT"),
                path: None,
                dirty: false,
            }),
            ..Default::default()
        };
        app.refresh();
        app
    }

    fn names(app: &App) -> Vec<String> {
        app.nodes.iter().map(|n| n.name.clone()).collect()
    }

    #[test]
    fn undo_and_redo_step_through_disk_edits() {
        let mut app = app_with_disk();
        app.add("ONE".into(), b"\x01\x08");
        app.add("TWO".into(), b"\x01\x08");
        app.selected = Some(0);
        app.rename_to = "FIRST".into();
        app.rename();
        app.selected = Some(1);
        app.delete();
        assert_eq!(names(&app), ["FIRST"]);

        app.undo(); // delete
        assert_eq!(names(&app), ["FIRST", "TWO"]);
        assert_eq!(app.status, "Undone: Deleted \"TWO\" (1 blocks freed)");
        app.undo(); // rename
        assert_eq!(names(&app), ["ONE", "TWO"]);
        app.undo(); // add TWO
        app.undo(); // add ONE
        assert!(names(&app).is_empty());
        assert!(!app.is_dirty(), "back to the state that was loaded");
        app.undo();
        assert_eq!(app.status, "Nothing to undo");

        app.redo();
        app.redo();
        assert_eq!(names(&app), ["ONE", "TWO"]);
        assert!(app.is_dirty());
        // A new edit drops the redo history.
        app.add("NEW".into(), b"\x01\x08");
        app.redo();
        assert_eq!(app.status, "Nothing to redo");
        assert_eq!(names(&app), ["ONE", "TWO", "NEW"]);
    }

    fn other_names(app: &App) -> Vec<String> {
        let o = app.other.as_ref().unwrap();
        o.disk
            .directory()
            .unwrap()
            .iter()
            .map(|e| e.name.clone())
            .collect()
    }

    #[test]
    fn copy_window_copies_both_ways_with_undo() {
        let mut app = app_with_disk();
        app.add("ONE".into(), b"\x01\x08\x60");
        app.add("TWO".into(), &[7u8; 600]);
        app.add("THREE".into(), b"\x01\x08");
        // Nothing to copy to yet.
        app.marked.extend([0, 2]);
        app.apply(Act::CopyToOther);
        assert!(app.other.is_none());

        app.apply(Act::OtherNew(2));
        assert!(app.other.as_ref().unwrap().dirty);
        app.apply(Act::CopyToOther);
        assert_eq!(other_names(&app), ["ONE", "THREE"]);
        assert_eq!(app.status, "Copied 2 files to the other disk");
        // Copying one again is refused, and nothing changes.
        app.marked.clear();
        app.selected = Some(0);
        app.apply(Act::CopyToOther);
        assert_eq!(
            app.status,
            "Nothing copied; skipped \"ONE\": a file with that name already exists"
        );
        assert_eq!(app.other.as_ref().unwrap().undo.len(), 1);

        // Back the other way: delete ONE here, copy it back from the other
        // disk; the main Undo takes it away again.
        app.selected = Some(0);
        app.delete();
        assert_eq!(names(&app), ["TWO", "THREE"]);
        app.apply(Act::OtherMark(0, false));
        app.apply(Act::OtherMark(1, true));
        app.apply(Act::CopyFromOther);
        assert_eq!(
            app.status,
            "Copied \"ONE\" to the open disk; skipped \"THREE\": a file with that name already exists"
        );
        assert_eq!(names(&app), ["ONE", "TWO", "THREE"]);
        assert_eq!(app.nodes[0].data.as_deref().unwrap(), b"\x01\x08\x60");
        app.undo();
        assert_eq!(names(&app), ["TWO", "THREE"]);

        // The other disk's own Undo.
        app.apply(Act::OtherUndo);
        assert!(other_names(&app).is_empty());
        app.apply(Act::OtherDiscard);
        assert!(app.other.is_none());
    }

    #[test]
    fn copy_window_copies_archive_members_and_into_new_archives() {
        let mut app = App {
            nodes: vec![
                Node {
                    name: "DOC".into(),
                    badge: "seq".into(),
                    detail: String::new(),
                    blocks: 0,
                    data: Ok(b"TEXT".to_vec()),
                },
                Node {
                    name: "DATA".into(),
                    badge: "rel".into(),
                    detail: String::new(),
                    blocks: 0,
                    data: Ok(vec![1; 10]),
                },
            ],
            ..Default::default()
        };
        app.apply(Act::OtherNew(0));
        app.marked.extend([0, 1]);
        app.apply(Act::CopyToOther);
        assert_eq!(other_names(&app), ["DOC"]);
        assert!(app
            .status
            .ends_with("skipped \"DATA\": can't copy REL files"));
        let o = app.other.as_ref().unwrap();
        let e = &o.disk.directory().unwrap()[0];
        assert_eq!(e.file_type, FileType::Seq);
        assert_eq!(o.disk.read_entry(e).unwrap(), b"TEXT");

        // A read-only listing can't receive; a new archive can.
        app.apply(Act::OtherMark(0, false));
        app.apply(Act::CopyFromOther);
        assert_eq!(
            app.status,
            "Open a disk or start a new archive to copy into"
        );
        app.apply(Act::NewArchive);
        app.apply(Act::CopyFromOther);
        assert_eq!(names(&app), ["DOC"]);
        assert_eq!(app.archive.as_ref().unwrap()[0].file_type, FileType::Seq);
    }

    #[test]
    fn failed_edits_are_not_recorded_and_history_is_capped() {
        let mut app = app_with_disk();
        app.selected = None;
        app.delete(); // nothing selected: no change
        assert!(app.undo.is_empty());
        for i in 0..UNDO_MAX + 5 {
            app.add(format!("F{i}"), b"\x01\x08");
        }
        assert_eq!(app.undo.len(), UNDO_MAX);
    }

    #[test]
    fn undo_after_save_marks_the_disk_unsaved() {
        let dir = std::env::temp_dir().join(format!("s64-undo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("u.d64");
        let mut app = app_with_disk();
        app.disk.as_mut().unwrap().path = Some(path.clone());
        app.add("ONE".into(), b"\x01\x08");
        app.save(false);
        assert!(!app.is_dirty());
        app.undo();
        assert!(app.is_dirty(), "the disk no longer matches the saved file");
        app.redo();
        assert!(!app.is_dirty(), "back to exactly what was saved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_works_on_a_new_archive_and_opening_clears_history() {
        let mut app = App::default();
        app.apply(Act::NewArchive);
        app.add("A".into(), b"aaa");
        app.add("B".into(), b"bbb");
        app.undo();
        assert_eq!(names(&app), ["A"]);
        app.apply(Act::NewArchive);
        assert!(app.undo.is_empty() && app.redo.is_empty());
    }

    #[test]
    fn sector64_x11_values() {
        use std::ffi::OsStr;
        assert!(!force_x11(None));
        assert!(!force_x11(Some(OsStr::new(""))));
        assert!(!force_x11(Some(OsStr::new("0"))));
        assert!(force_x11(Some(OsStr::new("1"))));
        assert!(force_x11(Some(OsStr::new("yes"))));
    }

    #[test]
    fn packed_details_align_the_method_column() {
        let d = packed_details(
            [(6068, "lz"), (992, "lz"), (19374, "huffman+rle")]
                .into_iter()
                .map(|(n, m)| (n, m.to_string())),
        );
        assert_eq!(
            d,
            [
                " 6068 BYTES  lz",
                "  992 BYTES  lz",
                "19374 BYTES  huffman+rle"
            ]
        );
    }
    use sector64::FileType;

    /// A fresh, empty scratch folder under the system temp dir.
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("s64gui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A small D64 with one file on it.
    fn sample_d64() -> Vec<u8> {
        let mut disk = Disk::format("ZIP TEST", "ZT");
        let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        disk.write_file("HELLO", FileType::Prg, &data).unwrap();
        disk.as_bytes().to_vec()
    }

    #[test]
    fn disk_map_is_rebuilt_after_edits_and_covers_read_only_disks() {
        let mut app = app_with_disk();
        app.map_cache = app.map_disk().map(Disk::block_map);
        assert_eq!(app.map_cache.as_ref().unwrap().files.len(), 0);
        app.add("NEW".into(), &[1u8; 600]);
        assert!(app.map_cache.is_none(), "an edit drops the stale map");
        let map = app.map_disk().unwrap().block_map();
        assert_eq!(map.blocks_of(0).len(), 3);
        app.map_cache = Some(map);
        app.undo();
        assert!(app.map_cache.is_none(), "undo drops it too");

        // A Zip-Code set opens read-only but still has a map.
        let dir = scratch("map");
        for (i, part) in sector64::write_zipcode(&sample_d64())
            .unwrap()
            .iter()
            .enumerate()
        {
            std::fs::write(dir.join(format!("{}!GAME", i + 1)), part).unwrap();
        }
        app.open(&dir.join("1!GAME"));
        assert!(app.disk.is_none() && app.read_only_disk);
        let map = app
            .map_disk()
            .expect("read-only disk is mapped")
            .block_map();
        assert_eq!(map.files[0].name, "HELLO");
        // Opening a plain archive leaves nothing to map.
        app.archive = Some(Vec::new());
        app.open(&dir.join("missing.d64"));
        assert!(app.map_disk().is_none());
    }

    #[test]
    fn disk_map_tooltips_and_summary() {
        let mut disk = Disk::format("TIPS", "01");
        disk.write_file("GAME", FileType::Prg, &[1u8; 600]).unwrap();
        let map = disk.block_map();
        let (t, s) = map.chains[0][1];
        assert_eq!(
            block_tooltip(&map, t, s),
            format!("Track {t}, sector {s}\n\"GAME\" PRG · block 2 of 3")
        );
        assert_eq!(
            block_tooltip(&map, 18, 0),
            "Track 18, sector 0\nDOS: header, BAM or directory"
        );
        assert_eq!(block_tooltip(&map, 35, 0), "Track 35, sector 0\nFree");
        assert_eq!(block_tooltip(&map, 99, 0), "");
        assert_eq!(
            map_summary(&map),
            "683 blocks · 3 in files · 2 DOS · 678 free"
        );

        // Point a second file's start at GAME's last block: both claim it.
        disk.write_file("COPY", FileType::Seq, &[2u8; 10]).unwrap();
        let mut bytes = disk.as_bytes().to_vec();
        let dir_off = (17 * 21 + 1) * 256; // track 18, sector 1
        let (lt, ls) = map.chains[0][2];
        bytes[dir_off + 32 + 3] = lt;
        bytes[dir_off + 32 + 4] = ls;
        let map = Disk::load(bytes).unwrap().block_map();
        let tip = block_tooltip(&map, lt, ls);
        assert!(
            tip.contains("⚠ also used by \"COPY\" SEQ · block 1 of 1"),
            "{tip}"
        );
        assert!(map_summary(&map).ends_with("· 1 allocated but unused · 1 in conflict"));
    }

    #[test]
    fn zipcode_set_opens_from_any_part() {
        let d64 = sample_d64();
        let dir = scratch("zip");
        for (i, part) in sector64::write_zipcode(&d64).unwrap().iter().enumerate() {
            std::fs::write(dir.join(format!("{}!GAME", i + 1)), part).unwrap();
        }
        for n in 1..=4 {
            let got = zipcode_set(&dir.join(format!("{n}!GAME"))).expect("is a zip-code part");
            assert_eq!(got.unwrap(), d64, "opened via part {n}");
        }
        match open_path(&dir.join("3!GAME")).unwrap() {
            Opened::ReadOnlyDisk(title, nodes, _) => {
                assert!(
                    title.contains("ZIP TEST") && title.contains("read-only"),
                    "{title}"
                );
                assert!(nodes.iter().any(|n| n.name == "HELLO"));
            }
            _ => panic!("a Zip-Code set must open as a read-only disk"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zipcode_set_reports_a_missing_part() {
        let dir = scratch("zipmissing");
        let parts = sector64::write_zipcode(&sample_d64()).unwrap();
        for i in [0, 1, 3] {
            std::fs::write(dir.join(format!("{}!GAME", i + 1)), &parts[i]).unwrap();
        }
        let err = zipcode_set(&dir.join("1!GAME")).unwrap().unwrap_err();
        assert!(err.contains("3!GAME"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_zipcode_names_are_ignored() {
        for name in ["disk.d64", "5!GAME", "1!", "!GAME", "x1!GAME"] {
            assert!(zipcode_set(Path::new(name)).is_none(), "{name}");
        }
    }

    /// The real sample set, when present (see the repo's tests/ for the convention).
    #[test]
    fn zipcode_sample_set() {
        let dir = match std::env::var("SECTOR64_SAMPLES") {
            Ok(p) => PathBuf::from(p),
            Err(_) => match std::env::var("HOME") {
                Ok(h) => PathBuf::from(h).join("Downloads/Sector64 Test Files"),
                Err(_) => return,
            },
        };
        let first = dir.join("1!ZIP-CODE-2");
        if !first.is_file() {
            eprintln!("skipping: Zip-Code sample not present");
            return;
        }
        match open_path(&first).unwrap() {
            Opened::ReadOnlyDisk(title, nodes, _) => {
                assert!(title.contains("Zip-Code"), "{title}");
                assert!(!nodes.is_empty());
            }
            _ => panic!("a Zip-Code set must open as a read-only disk"),
        }
    }
}
