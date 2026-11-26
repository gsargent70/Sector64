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

#[cfg(target_os = "macos")]
mod mac;
mod sector;

use std::path::{Path, PathBuf};

use eframe::egui;
use egui::Color32;
use sector64::compare::{Item as CompareItem, Row as CompareRow, Status as CompareStatus};
use sector64::repair::{Fix, RepairPlan};
use sector64::tokenize::{tokenize, TextProblem, Tokenized};
use sector64::{
    write_lbr, write_lnx, write_p00, write_t64, BamState, BasicDialect, BlockMap, BlockStatus,
    Disk, FileType, GfxKind, GfxSheet, ImportFile, Owner, PicFormat,
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
    #[cfg(target_os = "macos")]
    {
        // Font Book installs to the first of these.
        let home = std::env::var("HOME").unwrap_or_default();
        dirs.push(format!("{home}/Library/Fonts"));
        dirs.push("/Library/Fonts".into());
    }
    #[cfg(not(any(windows, target_os = "macos")))]
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
    /// A transparent icon, so menu items without one line up with those with.
    blank: egui::TextureHandle,
    /// The app's floppy-disk logo (the window icon), for the menu bar.
    logo: egui::TextureHandle,
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
            blank: mk("i_blank", &["........"; 8]),
            logo: {
                let icon = make_icon();
                let img = egui::ColorImage::from_rgba_unmultiplied(
                    [icon.width as usize, icon.height as usize],
                    &icon.rgba,
                );
                ctx.load_texture("i_logo", img, egui::TextureOptions::LINEAR)
            },
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

/// Shortcut keys as this platform names them: "Ctrl+K" is "Cmd+K" on a Mac
/// (the shortcuts test egui's `command` modifier, which is ⌘ there).
fn key_name(keys: &str) -> String {
    if cfg!(target_os = "macos") {
        keys.replace("Ctrl", "Cmd")
    } else {
        keys.to_string()
    }
}

/// An Open dialog that lists `exts` first, with an "All files" choice. macOS
/// dialogs have no such choice: they merge every filter into one and take no
/// wildcard, which would leave files like Zip-Code's `1!NAME` unselectable,
/// so there the dialog just shows everything.
fn open_dialog(name: &str, exts: &[&str]) -> rfd::FileDialog {
    let dlg = rfd::FileDialog::new();
    if cfg!(target_os = "macos") {
        dlg
    } else {
        dlg.add_filter(name, exts).add_filter("All files", &["*"])
    }
}

/// A menu item's shortcut, set off from its name by a dash ("— Ctrl+K"). The
/// leading spaces keep a gap before the dash on the menu's widest item, and
/// the keys are padded to one width (the font is monospaced) so the dashes
/// line up even though shortcuts are right-aligned.
fn key_hint(keys: &str) -> String {
    format!("    — {:<6}", key_name(keys))
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
    /// The bytes disassembled as 6502 machine code.
    Dis,
    /// A paint program's bitmap picture.
    Pic,
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

/// The picture the PIC preview shows, decoded when the selection or the
/// chosen format changes.
struct PicView {
    key: (usize, Option<PicFormat>),
    /// The decoded picture and its texture, or why there's none.
    pic: Result<(sector64::Picture, egui::TextureHandle), String>,
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
            || l.starts_with("run in vice: ")
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

/// One open disk or archive and everything about how it's listed: the
/// files, the selection, sort and filter, undo history and preview caches.
#[derive(Default)]
struct Pane {
    /// Path of the open file (shown on the "Open:" line).
    opened_path: Option<String>,
    /// The listing's heading (disk name and ID, or what kind of archive).
    title: String,
    nodes: Vec<Node>,
    selected: Option<usize>,
    /// Extra rows marked for a multi-file extract (Ctrl/Shift-click).
    marked: std::collections::BTreeSet<usize>,
    /// Column sort: `(key, ascending)`; `None` is directory order.
    sort: Option<(SortKey, bool)>,
    /// Case-insensitive name filter; empty shows everything.
    filter: String,
    /// Inline rename: the row whose name is being edited, the text being
    /// typed, and whether the edit field should grab focus this frame.
    renaming: Option<usize>,
    rename_to: String,
    rename_focus: bool,
    disk: Option<EditDisk>,
    /// The listing is a read-only disk (G64, a 1581-partition D2M, a Zip-Code
    /// set): its rows carry block counts, so the BLK column applies.
    read_only_disk: bool,
    /// The read-only disk itself, kept for the disk map.
    ro_disk: Option<Disk>,
    /// The disk map, built when the window opens and dropped on every change.
    map_cache: Option<BlockMap>,
    // staging list when building a new archive
    archive: Option<Vec<ImportFile>>,
    /// Edits that Undo steps back through, newest last.
    undo: Vec<UndoStep>,
    /// Edits that Redo re-applies, newest last; cleared by any new edit.
    redo: Vec<UndoStep>,
    /// Cached rendered preview: (node index, mode it was rendered in, text).
    hex_cache: Option<(usize, PreviewMode, Option<BasicDialect>, String)>,
    /// The GFX preview's sheet, redrawn when the selection or options change.
    gfx_cache: Option<GfxView>,
    /// The PIC preview's decoded picture.
    pic_cache: Option<PicView>,
}

/// One file the Find window matched.
struct FindHit {
    /// Its row in the listing, and its name then (to notice a changed list).
    node: usize,
    name: String,
    badge: String,
    /// Whether the name itself matches (text searches only).
    in_name: bool,
    /// The first few match offsets, and how many there are in all.
    offsets: Vec<usize>,
    total: usize,
    /// Whether its matches can be shown in the Sector Editor (a plain file
    /// on a disk image).
    on_disk: bool,
}

/// How many offsets the Find window lists per file.
const FIND_OFFSETS: usize = 24;

/// The Find window: the query, the last search's results, and the match
/// the hex preview highlights.
#[derive(Default)]
struct FindState {
    query: String,
    /// Search for hex bytes (`true`) or text.
    hex: bool,
    /// Matches open in the Sector Editor instead of the preview.
    in_sector: bool,
    hits: Vec<FindHit>,
    /// What the last search found (or why it couldn't run).
    summary: String,
    /// How long a match is, for the highlight.
    len: usize,
    /// Which listing the results are for (see `App::sector_source`).
    source: String,
    /// The match the hex preview highlights: (row, offset, length).
    mark: Option<(usize, usize, usize)>,
    /// Scroll the preview to the mark on the next frame.
    scroll: bool,
    /// Put the keyboard in the query box on the next frame.
    focus: bool,
}

/// What the BASIC editor's two load buttons read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BasicSource {
    File,
    Selected,
}

/// The BASIC editor: a program's text, and where it's to go.
struct BasicEditor {
    text: String,
    name: String,
    dialect: BasicDialect,
    /// Where the program loads, when that isn't the dialect's usual address.
    load: Option<u16>,
    /// The text has changed since it was loaded, saved or added.
    modified: bool,
    /// A load button was pressed while the text was modified: press it again
    /// to lose the changes.
    confirm: Option<BasicSource>,
    /// One of its fields had the keyboard last frame (Esc takes it away
    /// before this frame's widgets can tell).
    typing: bool,
    /// The text tokenized as it stands; `None` when it needs doing.
    checked: Option<Result<Tokenized, Vec<TextProblem>>>,
}

impl Default for BasicEditor {
    fn default() -> Self {
        BasicEditor {
            text: String::new(),
            name: String::new(),
            dialect: BasicDialect::V2,
            load: None,
            modified: false,
            confirm: None,
            typing: false,
            checked: None,
        }
    }
}

impl BasicEditor {
    /// The text tokenized, or what's wrong with it.
    fn check(&mut self) -> &Result<Tokenized, Vec<TextProblem>> {
        self.checked
            .get_or_insert_with(|| tokenize(&self.text, self.dialect, self.load))
    }

    /// Start again with other text, unmodified.
    fn set_text(&mut self, text: String, load: Option<u16>) {
        self.text = text;
        self.load = load;
        self.modified = false;
        self.confirm = None;
        self.checked = None;
    }

    /// The program's name as a disk keeps it.
    fn disk_name(&self) -> String {
        self.name.trim().to_uppercase().chars().take(16).collect()
    }
}

/// Put a BASIC program on `disk`, replacing the program of that name if
/// there is one. Only an unlocked, plain BASIC PRG is replaced. Returns
/// whether it was; on an error the disk is unchanged.
fn put_basic(disk: &mut Disk, name: &str, prg: &[u8]) -> Result<bool, String> {
    let found = disk.find_entry(name).map_err(|e| e.to_string())?;
    let mut new = disk.clone();
    if let Some(e) = &found {
        let refuse = |why: &str| Err(format!("\"{name}\" is on the disk already and {why}"));
        if e.locked {
            return refuse("it's locked");
        }
        let old = disk.read_entry(e).ok();
        let listing = old.as_deref().and_then(sector64::detokenize_basic);
        match listing {
            Some(l) if e.file_type == FileType::Prg && l.trailing_bytes == 0 => {}
            Some(_) if e.file_type == FileType::Prg => {
                return refuse("has machine code or data after its BASIC");
            }
            _ => return refuse("isn't a BASIC program"),
        }
        new.scratch(name).map_err(|e| e.to_string())?;
    }
    new.write_file(name, FileType::Prg, prg)
        .map_err(|e| e.to_string())?;
    *disk = new;
    Ok(found.is_some())
}

#[derive(Default)]
struct App {
    /// The open disk or archive: in dual-pane view, the focused pane, the one
    /// the menus and keys act on.
    pane: Pane,
    /// The other pane in dual-pane view. Focusing it swaps it with `pane`.
    back: Pane,
    /// Whether two panes show side by side.
    dual: bool,
    /// Whether the focused pane is the right-hand one.
    focus_right: bool,
    /// Whether the Compare window is open, and whether it lists the files
    /// that are the same on both sides as well as the differences.
    show_compare: bool,
    compare_all: bool,
    // new-image controls
    new_fmt: usize, // 0=d64, 1=d71, 2=d81
    new_name: String,
    new_id: String,
    // editing
    add_type: usize, // 0=prg,1=seq,2=usr
    // bottom bar
    out_dir: String,
    status: String,
    /// Last status text turned into a toast, so each change toasts once.
    last_status: String,
    /// Active transient notifications (newest first in the stack).
    toasts: Vec<Toast>,
    /// Recently opened or saved files, newest first (File ▸ Open Recent).
    recent: Vec<String>,
    /// A pending confirmation (unsaved changes before open/quit).
    confirm: Option<Confirm>,
    /// Whether the Disk Map window is open.
    show_map: bool,
    /// Whether the Sector Editor window is open, and the block it shows.
    show_sector: bool,
    sector: sector::SectorEditor,
    /// Whether the Find window is open, and its search.
    show_find: bool,
    find: FindState,
    /// Whether the Copy window is open, and the disk it copies to and from.
    show_copy: bool,
    other: Option<OtherDisk>,
    /// The BASIC editor: whether its window is open, and its program.
    show_basic: bool,
    basic: BasicEditor,
    // preview/validation pane
    /// How the preview pane renders bytes (hex or PETSCII text).
    preview_mode: PreviewMode,
    /// The BASIC dialect the BASIC preview lists in; `None` detects it.
    basic_dialect: Option<BasicDialect>,
    /// What the GFX preview draws the bytes as, and whether in multicolour.
    gfx_kind: GfxKind,
    gfx_multi: bool,
    /// The PIC preview's format (`None` detects it).
    pic_format: Option<PicFormat>,
    validation: String,
    /// Whether the last check found anything wrong, and the fixes it offers.
    validation_bad: bool,
    repair: RepairPlan,
    // whether the validation window is showing
    show_validation: bool,
    // whether the preview pane is visible (toggled from the menu bar)
    show_preview: bool,
    // whether the Help / user-manual window is open
    show_help: bool,
    // button-icon textures, built lazily on the first frame
    icons: Option<Icons>,
    /// Folder holding VICE's emulators, chosen when they weren't on the PATH.
    vice_dir: String,
    /// Emulators started by Run in VICE, with the temporary folder each runs
    /// from (removed at exit once that emulator has closed).
    vice_runs: Vec<(std::process::Child, PathBuf)>,
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
    /// BASIC editor: load its text from a file, or from the selected program.
    BasicOpen,
    BasicFromSelected,
    /// BASIC editor: save its text to a file, or put the program on the disk.
    BasicSaveText,
    BasicAdd,
    Validate,
    /// Validation window: apply the fixes that are ticked.
    Repair,
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
    /// Start the open disk, or the selected program, in VICE.
    RunVice,
    /// Dual-pane view: focus the other pane.
    FocusOther,
    /// Start renaming this row in place.
    StartRename(usize),
    /// Dual-pane view: copy the chosen files into the other pane.
    CopyToPane,
    /// Compare window: select this file in the left and right panes.
    CompareSelect(Option<usize>, Option<usize>),
    /// Compare window: copy the files missing from the right pane into it
    /// (`true`), or the ones missing from the left pane (`false`).
    CopyMissing(bool),
    /// Open the Sector Editor at this track and sector.
    SectorOpen(u8, u8),
    /// Sector Editor: write its staged edits to the disk.
    SectorWrite,
    /// Find window: search the listing's files.
    Find,
    /// Find window: show this file's match at this offset.
    FindShow(usize, usize),
}

impl Pane {
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
        self.pic_cache = None;
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
}

impl App {
    /// `initial` is up to two files from the command line; two open side by
    /// side in dual-pane view.
    fn new(initial: &[String], storage: Option<&dyn eframe::Storage>) -> Self {
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
            if let Some(dual) = eframe::get_value::<bool>(s, "dual_pane") {
                app.dual = dual;
            }
            // The editor's program is kept, so quitting never loses it.
            if let Some((text, name, dialect, load, modified)) =
                eframe::get_value::<(String, String, u8, Option<u16>, bool)>(s, "basic_editor")
            {
                app.basic = BasicEditor {
                    text,
                    name,
                    dialect: BASIC_DIALECTS[usize::from(dialect).min(2)].0,
                    load,
                    modified,
                    ..Default::default()
                };
            }
            if let Some(dir) = eframe::get_value::<String>(s, "vice_dir") {
                app.vice_dir = dir;
            }
            if let Some(list) = eframe::get_value::<Vec<String>>(s, "recent_files") {
                app.recent = list;
                app.recent.truncate(RECENT_MAX);
            }
        }
        if let Some(p) = initial.first() {
            app.open(Path::new(p));
        }
        if let Some(p) = initial.get(1) {
            app.dual = true;
            app.apply(Act::FocusOther);
            app.open(Path::new(p));
            app.apply(Act::FocusOther);
        }
        app
    }

    fn open(&mut self, path: &Path) {
        self.pane.clear_history();
        self.sector.source.clear();
        self.pane.nodes.clear();
        self.pane.selected = None;
        self.pane.marked.clear();
        self.pane.filter.clear();
        self.pane.disk = None;
        self.pane.archive = None;
        self.pane.read_only_disk = false;
        self.pane.ro_disk = None;
        self.pane.map_cache = None;
        self.pane.title.clear();
        self.pane.hex_cache = None;
        self.pane.gfx_cache = None;
        self.pane.pic_cache = None;
        self.validation.clear();
        self.pane.renaming = None;
        match open_path(path) {
            Ok(Opened::Disk(disk)) => {
                self.pane.disk = Some(EditDisk {
                    disk,
                    path: Some(path.to_path_buf()),
                    dirty: false,
                });
                self.pane.refresh();
                self.after_open(path);
            }
            Ok(Opened::List(title, nodes)) => {
                self.pane.title = title;
                self.pane.nodes = nodes;
                self.after_open(path);
            }
            Ok(Opened::ReadOnlyDisk(title, nodes, disk)) => {
                self.pane.title = title;
                self.pane.nodes = nodes;
                self.pane.read_only_disk = true;
                self.pane.ro_disk = Some(disk);
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
        self.pane.opened_path = Some(path.display().to_string());
        remember_recent(&mut self.recent, path);
        // Select the first entry so the preview pane shows something at once.
        self.pane.selected = (!self.pane.nodes.is_empty()).then_some(0);
        self.status = format!("{} item(s) loaded", self.pane.nodes.len());
    }

    fn apply(&mut self, act: Act) {
        match act {
            Act::OpenDialog => {
                let dlg = open_dialog(
                    "Commodore disks & archives",
                    &[
                        "d64", "d71", "d81", "g64", "d2m", "arc", "sda", "lha", "lzh", "sfx",
                        "lbr", "t64", "lnx", "wr3", "p00",
                    ],
                );
                if let Some(p) = dlg.pick_file() {
                    if self.pane.is_dirty() {
                        self.confirm = Some(Confirm::OpenFile(p));
                    } else {
                        self.open(&p);
                    }
                }
            }
            Act::Select(i) => {
                self.pane.selected = Some(i);
                self.pane.marked.clear();
            }
            Act::ToggleMark(i) => {
                if !self.pane.marked.insert(i) {
                    self.pane.marked.remove(&i);
                }
                self.pane.selected = Some(i);
            }
            Act::MarkRange(i) => {
                // Range in display order, so Shift-click follows what's shown.
                let order = self.pane.sorted_order(&self.pane.filter.clone());
                let anchor = self.pane.selected.unwrap_or(i);
                let pa = order.iter().position(|&x| x == anchor);
                let pi = order.iter().position(|&x| x == i);
                if let (Some(pa), Some(pi)) = (pa, pi) {
                    for &node in &order[pa.min(pi)..=pa.max(pi)] {
                        self.pane.marked.insert(node);
                    }
                }
                self.pane.selected = Some(i);
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
                } else if self.pane.is_dirty() {
                    self.confirm = Some(Confirm::OpenFile(p));
                } else {
                    self.open(&p);
                }
            }
            Act::FocusOther => {
                if self.dual {
                    std::mem::swap(&mut self.pane, &mut self.back);
                    self.back.renaming = None;
                    self.focus_right = !self.focus_right;
                    self.show_validation = false;
                }
            }
            Act::StartRename(i) => {
                if let Some(node) = self.pane.nodes.get(i) {
                    self.pane.rename_to = node.name.clone();
                    self.pane.renaming = Some(i);
                    self.pane.rename_focus = true;
                }
            }
            Act::CopyToPane => self.copy_to_pane(),
            Act::CopyMissing(to_right) => self.copy_missing(to_right),
            Act::SectorOpen(t, s) => {
                let src = self.sector_source();
                if let Some(disk) = self.pane.map_disk() {
                    if self.sector.modified() {
                        self.status = "Write or revert the Sector Editor's changes first".into();
                    } else {
                        self.sector.jump(disk, t, s);
                        self.sector.source = src;
                        self.show_sector = true;
                    }
                }
            }
            Act::SectorWrite => self.sector_write(),
            Act::Find => self.run_find(),
            Act::FindShow(node, offset) => self.find_show(node, offset),
            Act::CompareSelect(l, r) => {
                let (left, right) = if self.focus_right {
                    (&mut self.back, &mut self.pane)
                } else {
                    (&mut self.pane, &mut self.back)
                };
                for (pane, i) in [(left, l), (right, r)] {
                    pane.marked.clear();
                    if i.is_some() {
                        pane.selected = i;
                    }
                }
            }
            Act::Undo => self.undo(),
            Act::Redo => self.redo(),
            Act::ClearRecent => {
                self.recent.clear();
                self.status = "Cleared the recent-files list".into();
            }
            Act::Sort(key) => self.pane.cycle_sort(key),
            Act::ExtractSelected => self.extract_selected(),
            Act::ExtractAll => self.extract_all(),
            Act::Create => {
                self.sector.source.clear();
                let disk = match self.new_fmt {
                    1 => Disk::format_d71(&self.new_name, &self.new_id),
                    2 => Disk::format_d81(&self.new_name, &self.new_id),
                    _ => Disk::format(&self.new_name, &self.new_id),
                };
                self.pane.clear_history();
                self.pane.disk = Some(EditDisk {
                    disk,
                    path: None,
                    dirty: true,
                });
                self.pane.selected = None;
                self.pane.refresh();
                self.pane.opened_path = None;
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
            Act::BasicOpen => self.basic_open(),
            Act::BasicFromSelected => self.basic_from_selected(),
            Act::BasicSaveText => self.basic_save_text(),
            Act::BasicAdd => self.basic_add(),
            Act::Validate => self.validate(),
            Act::Repair => self.repair_disk(),
            Act::NewArchive => {
                self.pane.clear_history();
                self.pane.disk = None;
                self.pane.read_only_disk = false;
                self.pane.archive = Some(Vec::new());
                self.pane.selected = None;
                self.pane.hex_cache = None;
                self.pane.gfx_cache = None;
                self.pane.pic_cache = None;
                self.pane.refresh();
                self.pane.opened_path = None;
                self.status = "New archive — add files, then Save archive as…".into();
            }
            Act::SaveArchiveAs => self.save_archive(),
            Act::OtherOpen => {
                let dlg = open_dialog("disk image", &["d64", "d71", "d81", "d2m"]);
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
            Act::RunVice => self.run_vice(),
        }
    }

    /// What Run in VICE would start, or why there's nothing to run.
    fn vice_plan(&self) -> Result<VicePlan, String> {
        let stem = self
            .pane
            .opened_path
            .as_deref()
            .and_then(|p| Path::new(p).file_stem())
            .and_then(|s| s.to_str())
            .map(safe_file_name)
            .unwrap_or_else(|| "disk".into());
        if let Some(disk) = self.pane.map_disk() {
            let entries = disk.directory().unwrap_or_default();
            let is_prg = |e: &sector64::Entry| e.file_type == sector64::FileType::Prg;
            // The selected program, else the first one on the disk.
            let i = self
                .pane
                .selected
                .filter(|&i| entries.get(i).is_some_and(is_prg))
                .or_else(|| entries.iter().position(is_prg))
                .ok_or("there's no program on this disk to run")?;
            let entry = &entries[i];
            let load = self.pane.nodes.get(i).and_then(|n| n.data.as_ref().ok());
            use sector64::geometry::Geometry;
            let ext = match disk.geometry() {
                Geometry::Cbm1541 { .. } => "d64",
                Geometry::Cbm1571 => "d71",
                Geometry::Cbm1581 => "d81",
                Geometry::CmdNative { .. } => "d2m",
            };
            return Ok(VicePlan {
                file_name: format!("{stem}.{ext}"),
                program: Some(vice_load_name(&entry.name_raw)),
                c128: load.is_some_and(|d| is_c128_load(d)),
                what: format!("\"{}\"", entry.name),
            });
        }
        let node = self
            .pane
            .selected
            .and_then(|i| self.pane.nodes.get(i))
            .filter(|n| n.badge == "prg")
            .ok_or("select a program to run")?;
        let data = node
            .data
            .as_ref()
            .map_err(|e| format!("can't run it: {e}"))?;
        if data.len() < 3 {
            return Err("that program is empty".into());
        }
        Ok(VicePlan {
            file_name: format!("{}.prg", safe_file_name(&node.name)),
            program: None,
            c128: is_c128_load(data),
            what: format!("\"{}\"", node.name),
        })
    }

    /// Run in VICE: write the disk (with any unsaved edits) or the program to
    /// a temporary folder and autostart it, so VICE never touches the
    /// user's own file.
    fn run_vice(&mut self) {
        // Reap emulators that have closed.
        self.vice_runs
            .retain_mut(|(child, dir)| match child.try_wait() {
                Ok(None) => true,
                _ => {
                    let _ = std::fs::remove_dir_all(&*dir);
                    false
                }
            });
        let plan = match self.vice_plan() {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("Run in VICE: {e}");
                return;
            }
        };
        let names = vice_names(plan.c128);
        let mut exe = find_vice(names, &self.vice_dir);
        if exe.is_none() {
            // Not on the PATH: ask where VICE is, once.
            if let Some(d) = rfd::FileDialog::new()
                .set_title(format!("Choose the folder that holds VICE's {}", names[0]))
                .pick_folder()
            {
                exe = find_vice(names, &d.display().to_string());
                if exe.is_some() {
                    self.vice_dir = d.display().to_string();
                }
            }
        }
        let Some(exe) = exe else {
            self.status = format!(
                "Run in VICE failed: couldn't find VICE's {} (install VICE, or choose its folder)",
                names[0]
            );
            return;
        };
        static RUN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("sector64-vice-{}-{n}", std::process::id()));
        let file = dir.join(&plan.file_name);
        let bytes = match (
            self.pane.map_disk(),
            self.pane.selected.and_then(|i| self.pane.nodes.get(i)),
        ) {
            (Some(disk), _) => disk.as_bytes(),
            (None, Some(Node { data: Ok(d), .. })) => d.as_slice(),
            _ => return,
        };
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, bytes)) {
            self.status = format!("Run in VICE failed: can't write {}: {e}", file.display());
            return;
        }
        let mut target = file.into_os_string();
        if let Some(p) = &plan.program {
            target.push(":");
            target.push(p);
        }
        match std::process::Command::new(&exe)
            .arg("-autostart")
            .arg(target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                self.vice_runs.push((child, dir));
                let machine = if plan.c128 { "C128" } else { "C64" };
                self.status = format!("Running {} in VICE ({machine})", plan.what);
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                self.status = format!("Run in VICE failed: can't start {}: {e}", exe.display());
            }
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
        let mut rows: Vec<usize> = if self.pane.marked.is_empty() {
            self.pane.selected.into_iter().collect()
        } else {
            self.pane.marked.iter().copied().collect()
        };
        rows.retain(|&i| i < self.pane.nodes.len());
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
            .pane
            .disk
            .as_ref()
            .map(|e| &e.disk)
            .or(self.pane.ro_disk.as_ref());
        let entries = src.map(|d| d.directory().unwrap_or_default());
        let mut done = Vec::new();
        let mut failed = Vec::new();
        for i in rows {
            let node = &self.pane.nodes[i];
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
        let before = self.pane.snapshot();
        let mut done = Vec::new();
        let mut failed = Vec::new();
        for e in picked {
            let r = if let Some(ed) = &mut self.pane.disk {
                ed.disk
                    .copy_file_from(&o.disk, e)
                    .map_err(|x| x.to_string())
            } else if let Some(stage) = &mut self.pane.archive {
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
            if let Some(ed) = &mut self.pane.disk {
                ed.dirty = true;
            }
            self.pane.record(
                before,
                &format!("Copied {} file(s) from the other disk", done.len()),
            );
            self.pane.hex_cache = None;
            self.pane.gfx_cache = None;
            self.pane.pic_cache = None;
            self.pane.refresh();
        }
        self.status = status;
    }

    /// Dual-pane view: copy the focused pane's chosen files into the other
    /// pane's disk or new archive, as one step of that pane's Undo.
    fn copy_to_pane(&mut self) {
        if !self.dual {
            return;
        }
        let rows = self.chosen_rows();
        if rows.is_empty() {
            self.status = "Select files to copy to the other pane".into();
            return;
        }
        if !self.back.editable() {
            self.status =
                "Open a disk or start a new archive in the other pane to copy into".into();
            return;
        }
        self.status = copy_rows(&self.pane, &mut self.back, &rows, "the other pane");
    }

    /// Compare window: copy every file that's only in the other pane into the
    /// right pane (`to_right`) or the left one, as one step of its Undo.
    fn copy_missing(&mut self, to_right: bool) {
        let (dest, src, side) = match (self.focus_right == to_right, to_right) {
            (true, true) => (&mut self.pane, &self.back, "the right pane"),
            (true, false) => (&mut self.pane, &self.back, "the left pane"),
            (false, true) => (&mut self.back, &self.pane, "the right pane"),
            (false, false) => (&mut self.back, &self.pane, "the left pane"),
        };
        if !dest.editable() {
            self.status = format!("Open a disk or start a new archive in {side} to copy into");
            return;
        }
        let rows = sector64::compare::compare(&compare_items(src), &compare_items(dest));
        let missing: Vec<usize> = rows
            .iter()
            .filter(|r| r.status == CompareStatus::OnlyLeft)
            .filter_map(|r| r.left)
            .collect();
        self.status = copy_rows(src, dest, &missing, side);
    }

    fn save_archive(&mut self) {
        let Some(stage) = &self.pane.archive else {
            return;
        };
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
        let Some(ed) = &self.pane.disk else {
            self.validation = "Open a D64/D71/D81 to validate".into();
            return;
        };
        let map = ed.disk.block_map();
        let shared = map.tracks.iter().flatten().filter(|b| b.shared).count();
        let (_, used, system, lost, conflict) = map.counts();
        let plan = ed.disk.repair_plan();
        // Lost blocks, files never closed and off-by-default fixes may all be
        // deliberate; the rest is damage.
        self.validation_bad = conflict > 0
            || !map.problems.is_empty()
            || plan
                .choices
                .iter()
                .any(|c| c.apply && !matches!(c.fix, Fix::FreeLost { .. }));
        let free = ed.disk.blocks_free().unwrap_or(0);
        self.validation = format!(
            "blocks: {used} in files, {system} DOS, {free} free\n\
             shared by two files: {shared}\n\
             in use but listed as free: {}\n\
             lost (nothing uses them): {lost}\n\
             broken chains: {}\n{}",
            conflict - shared,
            map.problems.len(),
            if self.validation_bad {
                "⚠ problems found"
            } else {
                "✓ consistent with the BAM"
            },
        );
        self.repair = plan;
        self.show_validation = true;
    }

    /// Apply the ticked fixes to the open disk, as one step of its Undo, and
    /// check it again.
    fn repair_disk(&mut self) {
        let before = self.pane.snapshot();
        let Some(ed) = &mut self.pane.disk else {
            return;
        };
        // The window stays open while the disk can be edited, so make sure
        // the fixes are still the ones it shows.
        if !ed.disk.repair_plan().same_fixes(&self.repair) {
            self.validate();
            self.status = "The disk has changed, so it was checked again".into();
            return;
        }
        match ed.disk.repair(&self.repair) {
            Ok(n) => {
                ed.dirty = true;
                let done = format!(
                    "Repaired the disk ({n} fix{})",
                    if n == 1 { "" } else { "es" }
                );
                self.pane.record(before, &done);
                self.pane.hex_cache = None;
                self.pane.gfx_cache = None;
                self.pane.pic_cache = None;
                self.pane.refresh();
                self.validate();
                self.status = done;
            }
            Err(e) => self.status = format!("Repair failed, nothing changed: {e}"),
        }
    }

    /// Which disk the Sector Editor should show: the focused pane's file.
    fn sector_source(&self) -> String {
        format!(
            "{}|{}",
            self.pane.opened_path.as_deref().unwrap_or(""),
            self.focus_right
        )
    }

    /// Keep the Sector Editor on the focused pane's disk: when that changes,
    /// show the same block of the new disk (or its header) and drop any
    /// unwritten edits; otherwise re-read the block unless edits are staged.
    fn sector_sync(&mut self) {
        let src = self.sector_source();
        let Some(disk) = self.pane.map_disk() else {
            return;
        };
        if self.sector.source == src {
            self.sector.sync(disk);
            return;
        }
        if self.sector.modified() {
            self.status = "Another disk is showing, so the Sector Editor's unwritten \
                           changes were dropped"
                .into();
        }
        let (t, s) = (self.sector.track, self.sector.sector);
        let (t, s) = if sector::SectorEditor::exists(disk, t, s) {
            (t, s)
        } else {
            let g = disk.geometry();
            (g.directory_track(), g.header_sector())
        };
        self.sector.forget();
        self.sector.load(disk, t, s);
        self.sector.source = src;
    }

    /// Search every file in the focused listing for the Find window's query.
    fn run_find(&mut self) {
        let source = self.sector_source();
        let f = &mut self.find;
        f.hits.clear();
        f.mark = None;
        f.source = source;
        let pattern = if f.hex {
            sector64::search::Pattern::hex(&f.query)
        } else {
            sector64::search::Pattern::text(&f.query)
        };
        let pattern = match pattern {
            Ok(p) => p,
            Err(e) => {
                f.summary = format!("⚠ {e}");
                return;
            }
        };
        f.len = pattern.len();
        let map = self.pane.map_disk().map(Disk::block_map);
        let (mut matches, mut unreadable) = (0, 0);
        for (i, n) in self.pane.nodes.iter().enumerate() {
            let in_name = !f.hex && pattern.in_name(&n.name);
            let (offsets, total, on_disk) = match &n.data {
                Ok(d) => {
                    let offsets = pattern.find(d, FIND_OFFSETS);
                    let chain = map.as_ref().and_then(|m| m.chains.get(i));
                    let on_disk = chain.is_some_and(|c| {
                        sector64::search::block_of_offset(c, d.len(), 0).is_some()
                    });
                    let total = if offsets.len() < FIND_OFFSETS {
                        offsets.len()
                    } else {
                        pattern.count(d)
                    };
                    (offsets, total, on_disk)
                }
                Err(_) => {
                    unreadable += 1;
                    (Vec::new(), 0, false)
                }
            };
            matches += total;
            if in_name || total > 0 {
                f.hits.push(FindHit {
                    node: i,
                    name: n.name.clone(),
                    badge: n.badge.clone(),
                    in_name,
                    offsets,
                    total,
                    on_disk,
                });
            }
        }
        let files = self.pane.nodes.len();
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        f.summary = if f.hits.is_empty() {
            format!("Nothing found in {}", plural(files, "file", "files"))
        } else {
            let in_files = f.hits.iter().filter(|h| h.total > 0).count();
            let names = f.hits.iter().filter(|h| h.in_name).count();
            let mut s = format!(
                "{} in {} of {files}",
                plural(matches, "match", "matches"),
                plural(in_files, "file", "files")
            );
            if names > 0 {
                s += &format!(" · {} by name", plural(names, "file", "files"));
            }
            s
        };
        if unreadable > 0 {
            f.summary += &format!(
                " ({} couldn't be read)",
                plural(unreadable, "file", "files")
            );
        }
    }

    /// Show a Find match: select its file, then either highlight it in the
    /// hex preview or open its block in the Sector Editor.
    fn find_show(&mut self, node: usize, offset: usize) {
        let Some(hit) = self.find.hits.iter().find(|h| h.node == node) else {
            return;
        };
        if self.pane.nodes.get(node).map(|n| &n.name) != Some(&hit.name) {
            self.status = "The listing has changed since the search; search again".into();
            return;
        }
        let name = hit.name.clone();
        self.pane.selected = Some(node);
        self.pane.marked.clear();
        if self.find.in_sector && hit.on_disk {
            let len = self.pane.nodes[node].data.as_ref().map_or(0, Vec::len);
            let block = self
                .pane
                .map_disk()
                .map(Disk::block_map)
                .and_then(|m| m.chains.get(node).cloned())
                .and_then(|c| sector64::search::block_of_offset(&c, len, offset));
            if let Some((t, s, byte)) = block {
                self.apply(Act::SectorOpen(t, s));
                if self.show_sector && (self.sector.track, self.sector.sector) == (t, s) {
                    self.sector.place(byte);
                    self.status = format!(
                        "\"{name}\" offset ${offset:04X}: track {t}, sector {s}, byte ${byte:02X}"
                    );
                }
                return;
            }
        }
        self.preview_mode = PreviewMode::Hex;
        self.show_preview = true;
        self.dual = false;
        self.find.mark = Some((node, offset, self.find.len));
        self.find.scroll = true;
        self.status = if offset < PREVIEW_CAP {
            format!("\"{name}\": match at offset ${offset:04X}")
        } else {
            format!(
                "\"{name}\": match at offset ${offset:04X}, past the {} KB the preview shows",
                PREVIEW_CAP / 1024
            )
        };
    }

    /// Write the Sector Editor's staged edits to the open disk, as one undo
    /// step.
    fn sector_write(&mut self) {
        let before = self.pane.snapshot();
        let (t, s) = (self.sector.track, self.sector.sector);
        let Some(ed) = &mut self.pane.disk else {
            self.status = "This disk is read-only, so its blocks can't be changed".into();
            return;
        };
        match self.sector.write(&mut ed.disk) {
            Ok(()) => {
                ed.dirty = true;
                let done = format!("Wrote track {t}, sector {s}");
                self.pane.record(before, &done);
                self.pane.hex_cache = None;
                self.pane.gfx_cache = None;
                self.pane.pic_cache = None;
                self.pane.refresh();
                self.status = done;
            }
            Err(e) => self.status = format!("Couldn't write track {t}, sector {s}: {e}"),
        }
    }

    fn add(&mut self, name: String, bytes: &[u8]) {
        let ft = match self.add_type {
            1 => FileType::Seq,
            2 => FileType::Usr,
            _ => FileType::Prg,
        };
        let before = self.pane.snapshot();
        let done = format!("Added \"{name}\" ({} bytes)", bytes.len());
        if let Some(ed) = &mut self.pane.disk {
            match ed.disk.write_file(&name, ft, bytes) {
                Ok(()) => {
                    ed.dirty = true;
                    self.pane.record(before, &done);
                    self.pane.refresh();
                    self.status = done;
                }
                Err(e) => self.status = format!("Add failed: {e}"),
            }
        } else if let Some(stage) = &mut self.pane.archive {
            stage.push(ImportFile {
                name: name.clone(),
                file_type: ft,
                data: bytes.to_vec(),
            });
            self.pane.record(before, &done);
            self.pane.refresh();
            self.status = done;
        }
    }

    /// BASIC editor: load text from a file.
    fn basic_open(&mut self) {
        let Some(path) = open_dialog("BASIC text", &["txt", "bas"]).pick_file() else {
            return;
        };
        match std::fs::read(&path) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
                self.basic.set_text(text, None);
                self.basic.name = cbm_name_from(&path);
                self.status = format!("Loaded {}", path.display());
            }
            Err(e) => self.status = format!("Open failed: {e}"),
        }
    }

    /// BASIC editor: list the selected program.
    fn basic_from_selected(&mut self) {
        let node = self.pane.selected.and_then(|i| self.pane.nodes.get(i));
        let Some(node) = node else {
            self.status = "Select a BASIC program first".into();
            return;
        };
        let listing = node
            .data
            .as_deref()
            .ok()
            .and_then(|data| match self.basic_dialect {
                Some(d) => sector64::detokenize_basic_as(data, d),
                None => sector64::detokenize_basic(data),
            });
        let Some(l) = listing else {
            self.status = format!(
                "\"{}\" isn't a BASIC program, so it can't be listed",
                node.name
            );
            return;
        };
        let name = node.name.clone();
        self.status = match l.trailing_bytes {
            0 => format!("Listed \"{name}\" in the BASIC editor"),
            n => {
                format!("Listed \"{name}\" without the {n} bytes of machine code or data after it")
            }
        };
        self.basic.name = name;
        self.basic.dialect = l.dialect;
        let load = (l.load_address != l.dialect.load_address()).then_some(l.load_address);
        self.basic.set_text(l.text, load);
    }

    /// BASIC editor: save the text to a file.
    fn basic_save_text(&mut self) {
        let name = self.basic.disk_name().to_lowercase();
        let name = if name.is_empty() { "program" } else { &name };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("BASIC text", &["txt", "bas"])
            .set_file_name(format!("{}.txt", sanitize(name)))
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, &self.basic.text) {
            Ok(()) => {
                self.basic.modified = false;
                self.status = format!("Saved {}", path.display());
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    /// BASIC editor: put the program on the open disk or new archive.
    fn basic_add(&mut self) {
        let name = self.basic.disk_name();
        if name.is_empty() {
            self.status = "Enter a name for the program".into();
            return;
        }
        let prg = match self.basic.check() {
            Ok(t) => t.prg.clone(),
            Err(problems) => {
                self.status = format!("Error in {}", problems[0]);
                return;
            }
        };
        let before = self.pane.snapshot();
        let replaced = if let Some(ed) = &mut self.pane.disk {
            match put_basic(&mut ed.disk, &name, &prg) {
                Ok(replaced) => {
                    ed.dirty = true;
                    replaced
                }
                Err(e) => {
                    self.status = format!("Add failed: {e}");
                    return;
                }
            }
        } else if let Some(stage) = &mut self.pane.archive {
            let file = ImportFile {
                name: name.clone(),
                file_type: FileType::Prg,
                data: prg,
            };
            match stage.iter_mut().find(|f| f.name == name) {
                Some(f) => {
                    *f = file;
                    true
                }
                None => {
                    stage.push(file);
                    false
                }
            }
        } else {
            self.status = "Open a disk or start a new archive to put the program on".into();
            return;
        };
        let did = if replaced { "Replaced" } else { "Added" };
        let done = format!("{did} \"{name}\" from the BASIC editor");
        self.pane.record(before, &done);
        self.pane.hex_cache = None;
        self.pane.gfx_cache = None;
        self.pane.pic_cache = None;
        self.pane.refresh();
        // Selected, it's ready to preview or run.
        self.pane.selected = self.pane.nodes.iter().position(|n| n.name == name);
        self.basic.modified = false;
        self.status = done;
    }

    fn rename(&mut self) {
        let Some(i) = self.pane.selected else {
            self.status = "Select a file to rename".into();
            return;
        };
        let old = self.pane.nodes[i].name.clone();
        let new = self.pane.rename_to.trim().to_string();
        if new.is_empty() {
            self.status = "Enter a new name".into();
            return;
        }
        let before = self.pane.snapshot();
        let done = format!("Renamed \"{old}\" → \"{new}\"");
        if let Some(ed) = &mut self.pane.disk {
            match ed.disk.rename(&old, &new) {
                Ok(()) => {
                    ed.dirty = true;
                    self.pane.record(before, &done);
                    self.pane.refresh();
                    self.status = done;
                }
                Err(e) => self.status = format!("Rename failed: {e}"),
            }
        } else if let Some(stage) = &mut self.pane.archive {
            stage[i].name = new.clone();
            self.pane.record(before, &done);
            self.pane.refresh();
            self.status = done;
        }
    }

    fn delete(&mut self) {
        let Some(i) = self.pane.selected else {
            self.status = "Select a file to delete".into();
            return;
        };
        let name = self.pane.nodes[i].name.clone();
        let before = self.pane.snapshot();
        if let Some(ed) = &mut self.pane.disk {
            match ed.disk.scratch(&name) {
                Ok(n) => {
                    ed.dirty = true;
                    let done = format!("Deleted \"{name}\" ({n} blocks freed)");
                    self.pane.record(before, &done);
                    self.pane.selected = None;
                    self.pane.refresh();
                    self.status = done;
                }
                Err(e) => self.status = format!("Delete failed: {e}"),
            }
        } else if let Some(stage) = &mut self.pane.archive {
            stage.remove(i);
            let done = format!("Removed \"{name}\"");
            self.pane.record(before, &done);
            self.pane.selected = None;
            self.pane.refresh();
            self.status = done;
        }
    }

    fn undo(&mut self) {
        let (Some(step), Some(now)) = (self.pane.undo.pop(), self.pane.snapshot()) else {
            self.status = "Nothing to undo".into();
            return;
        };
        self.pane.restore(step.snapshot);
        self.status = format!("Undone: {}", step.label);
        self.pane.redo.push(UndoStep {
            snapshot: now,
            label: step.label,
        });
    }

    fn redo(&mut self) {
        let (Some(step), Some(now)) = (self.pane.redo.pop(), self.pane.snapshot()) else {
            self.status = "Nothing to redo".into();
            return;
        };
        self.pane.restore(step.snapshot);
        self.status = format!("Redone: {}", step.label);
        self.pane.undo.push(UndoStep {
            snapshot: now,
            label: step.label,
        });
    }

    fn save(&mut self, save_as: bool) {
        let Some(ed) = &mut self.pane.disk else {
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
                for step in self.pane.undo.iter_mut().chain(self.pane.redo.iter_mut()) {
                    if let Snapshot::Disk(_, dirty) = &mut step.snapshot {
                        *dirty = true;
                    }
                }
                self.pane.opened_path = Some(path.display().to_string());
                remember_recent(&mut self.recent, &path);
                self.status = format!("Saved {}", path.display());
                self.pane.refresh();
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn extract_selected(&mut self) {
        // The target rows: the multi-select set if any, else the active row.
        let mut targets: Vec<usize> = if self.pane.marked.is_empty() {
            self.pane.selected.into_iter().collect()
        } else {
            self.pane.marked.iter().copied().collect()
        };
        targets.retain(|&i| i < self.pane.nodes.len());
        if targets.is_empty() {
            self.status = "Select a file first".into();
            return;
        }
        if targets.len() == 1 {
            let i = targets[0];
            let (name, data) = (
                self.pane.nodes[i].name.clone(),
                self.pane.nodes[i].data.clone(),
            );
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
            match &self.pane.nodes[i].data {
                Ok(d) => {
                    let dest = PathBuf::from(&dir).join(sanitize(&self.pane.nodes[i].name));
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
        for node in &self.pane.nodes {
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
        eframe::set_value(storage, "dual_pane", &self.dual);
        eframe::set_value(storage, "recent_files", &self.recent);
        eframe::set_value(storage, "vice_dir", &self.vice_dir);
        let b = &self.basic;
        let dialect = BASIC_DIALECTS
            .iter()
            .position(|(d, ..)| *d == b.dialect)
            .unwrap_or(0) as u8;
        eframe::set_value(
            storage,
            "basic_editor",
            &(&b.text, &b.name, dialect, b.load, b.modified),
        );
    }

    /// Remove the temporary folders of emulators that have already closed;
    /// a running VICE keeps its disk.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for (child, dir) in &mut self.vice_runs {
            if !matches!(child.try_wait(), Ok(None)) {
                let _ = std::fs::remove_dir_all(&*dir);
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_ui(ctx);
    }
}

impl App {
    /// One frame of the whole window: `update` without eframe's `Frame`, so
    /// tests can run it on a bare context.
    fn frame_ui(&mut self, ctx: &egui::Context) {
        let mut acts: Vec<Act> = Vec::new();
        if self.icons.is_none() {
            self.icons = Some(Icons::load(ctx));
        }
        let ic = self.icons.clone().unwrap();
        // Tab switches panes when no text field has the keyboard. Checked
        // before any widget runs, while focus is still last frame's.
        let tab_switch = self.dual
            && !ctx.wants_keyboard_input()
            && ctx.input(|i| i.modifiers.is_none() && i.key_pressed(egui::Key::Tab));

        // Guard the window close against unsaved disk edits.
        let other_dirty = self.other.as_ref().is_some_and(|o| o.dirty);
        let any_dirty = self.pane.is_dirty() || self.back.is_dirty() || other_dirty;
        #[cfg(target_os = "macos")]
        if mac::quit_refused(any_dirty) {
            if self.confirm.is_none() {
                self.confirm = Some(Confirm::Quit);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if ctx.input(|i| i.viewport().close_requested()) && any_dirty {
            if self.confirm.is_none() {
                self.confirm = Some(Confirm::Quit);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        let editable = self.pane.editable();
        let is_disk = self.pane.disk.is_some();
        let is_archive = self.pane.archive.is_some();

        // Refresh the hex preview cache when the selection changes (done before
        // the panels so the closures only read `self` immutably).
        match self.pane.selected {
            Some(i) if i < self.pane.nodes.len() => {
                let mode = self.preview_mode;
                let dialect = self.basic_dialect;
                let stale = self
                    .pane
                    .hex_cache
                    .as_ref()
                    .map(|(ci, cm, cd, _)| *ci != i || *cm != mode || *cd != dialect)
                    .unwrap_or(true);
                if stale {
                    let text = match &self.pane.nodes[i].data {
                        Ok(d) => match mode {
                            PreviewMode::Hex => hexdump(d, PREVIEW_CAP),
                            PreviewMode::Text => petscii_text(d, 64 * 1024),
                            PreviewMode::Basic => basic_preview(d, dialect),
                            PreviewMode::Dis => dis_preview(d, self.pane.nodes[i].badge == "prg"),
                            // Drawn from the bytes by the pane itself; this
                            // only marks that there is something to show.
                            PreviewMode::Gfx | PreviewMode::Pic => "gfx".into(),
                        },
                        Err(e) => format!("(not extractable: {e})"),
                    };
                    self.pane.hex_cache = Some((i, mode, dialect, text));
                }
            }
            _ => {
                self.pane.hex_cache = None;
                self.pane.gfx_cache = None;
                self.pane.pic_cache = None;
            }
        }
        let preview = self
            .pane
            .hex_cache
            .as_ref()
            .map(|(_, _, _, s)| s.clone())
            .unwrap_or_default();
        let preview_name = self
            .pane
            .selected
            .and_then(|i| self.pane.nodes.get(i))
            .map(|n| n.name.clone());

        // Context flags that drive which buttons are enabled.
        let sel_now = self.pane.selected;
        let has_sel = self.pane.selected.is_some();
        let sel_extractable = self
            .pane
            .selected
            .and_then(|i| self.pane.nodes.get(i))
            .map(|n| n.data.is_ok())
            .unwrap_or(false);
        let any_extractable = self.pane.nodes.iter().any(|n| n.data.is_ok());

        // Local copies of edited strings (write back after the frame).
        let mut out_dir = std::mem::take(&mut self.out_dir);
        let mut new_name = std::mem::take(&mut self.new_name);
        let mut new_id = std::mem::take(&mut self.new_id);
        let mut rename_to = std::mem::take(&mut self.pane.rename_to);
        let mut new_fmt = self.new_fmt;
        let mut add_type = self.add_type;
        let mut renaming = self.pane.renaming.filter(|&i| i < self.pane.nodes.len());
        let mut rename_focus = self.pane.rename_focus;
        let mut filter = std::mem::take(&mut self.pane.filter);
        let mut preview_mode = self.preview_mode;
        let mut basic_dialect = self.basic_dialect;
        let mut gfx_kind = self.gfx_kind;
        let mut gfx_multi = self.gfx_multi;
        let mut gfx_cache = self.pane.gfx_cache.take();
        let mut pic_format = self.pic_format;
        let mut pic_cache = self.pane.pic_cache.take();
        let mut show_preview = self.show_preview;
        let mut show_help = self.show_help;
        let has_map = self.pane.map_disk().is_some();
        let mut show_map = self.show_map && has_map;
        let mut show_sector = self.show_sector && has_map;
        let mut show_find = self.show_find;
        // Find results (and the match the preview highlights) belong to the
        // listing they were searched in.
        let find_live = self.find.source == self.sector_source();
        if show_sector {
            self.sector_sync();
        }
        let mut show_copy = self.show_copy;
        let can_compare = !self.pane.nodes.is_empty() && !self.back.nodes.is_empty();
        let mut show_compare = self.show_compare && can_compare;
        let mut compare_all = self.compare_all;
        let can_run = self.vice_plan().is_ok();
        let mut show_basic = self.show_basic;
        let mut basic = std::mem::take(&mut self.basic);
        let mut dual = self.dual;
        let back_editable = self.back.editable();

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(2.0);
            egui::menu::bar(ui, |ui| {
                // The floppy-disk logo, as tall as the title beside it.
                let h = ui.text_style_height(&egui::TextStyle::Heading) + 4.0;
                ui.add(egui::Image::new((ic.logo.id(), egui::vec2(h, h))));
                ui.label(
                    egui::RichText::new("SECTOR64")
                        .heading()
                        .color(c64::ACCENT)
                        .strong(),
                );
                ui.separator();

                ui.menu_button("File", |ui| {
                    ui.set_min_width(180.0);
                    // Grow the menu to fit rather than wrap an item's name.
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
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
                    if ui
                        .add(icon_button(&ic.folder, "Open…").shortcut_text(key_hint("Ctrl+O")))
                        .clicked()
                    {
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
                        if ui
                            .add(icon_button(&ic.floppy, "Save").shortcut_text(key_hint("Ctrl+S")))
                            .clicked()
                        {
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
                    // Extract ▸ (the selected file(s), or everything).
                    ui.add_enabled_ui(any_extractable, |ui| {
                        egui::menu::menu_custom_button(
                            ui,
                            icon_button(&ic.down, "Extract ▸"),
                            |ui| {
                                ui.set_min_width(170.0);
                                if ui
                                    .add_enabled(
                                        sel_extractable,
                                        icon_button(&ic.down, "Extract Selected")
                                            .shortcut_text(key_hint("Enter")),
                                    )
                                    .clicked()
                                {
                                    acts.push(Act::ExtractSelected);
                                    ui.close_menu();
                                }
                                if ui.add(icon_button(&ic.down, "Extract All")).clicked() {
                                    acts.push(Act::ExtractAll);
                                    ui.close_menu();
                                }
                            },
                        );
                    });
                    if ui
                        .add(
                            icon_button(&ic.blank, "Copy…")
                                .selected(show_copy)
                                .shortcut_text(key_hint("Ctrl+K")),
                        )
                        .on_hover_text("copy files to and from a second disk")
                        .clicked()
                    {
                        show_copy = !show_copy;
                        ui.close_menu();
                    }
                    if ui
                        .add(
                            icon_button(&ic.blank, "BASIC Editor…")
                                .selected(show_basic)
                                .shortcut_text(key_hint("Ctrl+B")),
                        )
                        .on_hover_text("write a BASIC program, or edit the selected one")
                        .clicked()
                    {
                        show_basic = !show_basic;
                        ui.close_menu();
                    }
                    if ui
                        .add(
                            icon_button(&ic.blank, "Find in Files…")
                                .selected(show_find)
                                .shortcut_text(key_hint("Ctrl+F")),
                        )
                        .on_hover_text("look for text or hex bytes inside every file listed")
                        .clicked()
                    {
                        show_find = !show_find;
                        ui.close_menu();
                    }
                    if dual
                        && ui
                            .add_enabled(
                                has_sel && back_editable,
                                icon_button(&ic.blank, "Copy to Other Pane")
                                    .shortcut_text(key_hint("F5")),
                            )
                            .on_hover_text("copy the selected files into the other pane")
                            .on_disabled_hover_text(
                                "select files, with a disk or new archive open in the other pane",
                            )
                            .clicked()
                    {
                        acts.push(Act::CopyToPane);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            can_run,
                            icon_button(&ic.blank, "Run in VICE").shortcut_text(key_hint("Ctrl+R")),
                        )
                        .on_hover_text(
                            "start this disk in the VICE emulator, loading the selected \
                             program or else the first one",
                        )
                        .on_disabled_hover_text("open a disk or select a program to run it in VICE")
                        .clicked()
                    {
                        acts.push(Act::RunVice);
                        ui.close_menu();
                    }
                    ui.separator();
                    // Undo / Redo, with what they would undo or redo on hover.
                    let undo_next = self.pane.undo.last().map(|s| s.label.clone());
                    let redo_next = self.pane.redo.last().map(|s| s.label.clone());
                    let undo = ui
                        .add_enabled(
                            undo_next.is_some(),
                            icon_button(&ic.blank, "Undo").shortcut_text(key_hint("Ctrl+Z")),
                        )
                        .on_hover_text(undo_next.as_deref().unwrap_or("nothing to undo"));
                    if undo.clicked() {
                        acts.push(Act::Undo);
                        ui.close_menu();
                    }
                    let redo = ui
                        .add_enabled(
                            redo_next.is_some(),
                            icon_button(&ic.blank, "Redo").shortcut_text(key_hint("Ctrl+Y")),
                        )
                        .on_hover_text(redo_next.as_deref().unwrap_or("nothing to redo"));
                    if redo.clicked() {
                        acts.push(Act::Redo);
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.add_enabled_ui(has_sel, |ui| {
                        // Rename the selected row in place.
                        if ui
                            .add(icon_button(&ic.pencil, "Rename").shortcut_text(key_hint("F2")))
                            .clicked()
                        {
                            if let Some(s) = sel_now {
                                renaming = Some(s);
                                rename_to = self.pane.nodes[s].name.clone();
                                rename_focus = true;
                            }
                            ui.close_menu();
                        }
                        let del = if is_archive { "Remove" } else { "Delete" };
                        if ui
                            .add(icon_button(&ic.trash, del).shortcut_text(key_hint("Del")))
                            .clicked()
                        {
                            acts.push(Act::Delete);
                            ui.close_menu();
                        }
                    });
                });

                ui.separator();

                ui.menu_button("Disk", |ui| {
                    ui.set_min_width(170.0);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    let disk_tip = "open a D64, D71, D81 or CMD-native D2M to validate it";
                    if ui
                        .add_enabled(
                            is_disk,
                            egui::Button::new("Validate / Repair")
                                .shortcut_text(key_hint("Ctrl+L")),
                        )
                        .on_hover_text("check every file's blocks against the BAM, and repair them")
                        .on_disabled_hover_text(disk_tip)
                        .clicked()
                    {
                        acts.push(Act::Validate);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            has_map,
                            egui::Button::new("Disk Map")
                                .selected(show_map)
                                .shortcut_text(key_hint("Ctrl+M")),
                        )
                        .on_hover_text("show which file owns each block")
                        .on_disabled_hover_text("open a disk image to see its map")
                        .clicked()
                    {
                        show_map = !show_map;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            has_map,
                            egui::Button::new("Sector Editor")
                                .selected(show_sector)
                                .shortcut_text(key_hint("Ctrl+E")),
                        )
                        .on_hover_text("view and edit any track and sector in hex")
                        .on_disabled_hover_text("open a disk image to edit its sectors")
                        .clicked()
                    {
                        show_sector = !show_sector;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            can_compare,
                            egui::Button::new("Compare Panes")
                                .selected(show_compare)
                                .shortcut_text(key_hint("Ctrl+D")),
                        )
                        .on_hover_text("list which files match, differ, or are on only one side")
                        .on_disabled_hover_text(
                            "open a disk or archive in each pane (Dual) to compare them",
                        )
                        .clicked()
                    {
                        show_compare = !show_compare;
                        ui.close_menu();
                    }
                });
                ui.separator();
                if ui
                    .selectable_label(show_preview, "Preview")
                    .on_hover_text("show or hide the preview pane")
                    .clicked()
                {
                    show_preview = !show_preview;
                    // The preview takes the other pane's place; Dual brings
                    // that pane back.
                    if show_preview {
                        dual = false;
                    }
                }
                ui.separator();
                if ui
                    .selectable_label(dual, "Dual")
                    .on_hover_text(
                        "show two disks side by side; Tab switches between them \
                         (hides the preview pane)",
                    )
                    .clicked()
                {
                    dual = !dual;
                    if dual {
                        show_preview = false;
                    }
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
                let mut opened = self.pane.opened_path.clone().unwrap_or_default();
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

        // Preview pane — shown only when toggled on from the menu bar. All of
        // its functionality (HEX/TEXT toggle, scrolling) is unchanged.
        // Column widths for the listing, shared by the preview sizing below and
        // the list itself.
        let cols = Cols::of(&self.pane.nodes);

        if show_preview {
            let pane_w = preview_width(ctx, preview_mode, &preview, preview_name.as_deref(), cols);
            egui::SidePanel::right("preview")
                .resizable(false)
                .exact_width(pane_w)
                .show(ctx, |ui| {
                    let header = match &preview_name {
                        Some(n) => format!("PREVIEW — {n}"),
                        None => "PREVIEW".to_string(),
                    };
                    let title = |ui: &mut egui::Ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&header).color(c64::ACCENT).strong(),
                            )
                            .truncate(),
                        );
                    };
                    // The mode toggles sit right of the title when there's
                    // room for them and a few letters of it; otherwise the
                    // title gets its own line and the toggles wrap, so the
                    // pane is never wider than the space it was given (that
                    // left a strip of the window unpainted).
                    let toggles = preview_toggles_width(ui);
                    let title_room = 10.0
                        * ui.fonts(|f| {
                            f.glyph_width(&egui::TextStyle::Body.resolve(ui.style()), '0')
                        });
                    if ui.available_width() >= toggles + title_room {
                        ui.horizontal(|ui| {
                            // Laid out first, right-aligned, so the title is
                            // shortened to the space left beside them.
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    for (mode, label, tip) in PREVIEW_MODES.iter().rev() {
                                        ui.selectable_value(&mut preview_mode, *mode, *label)
                                            .on_hover_text(*tip);
                                    }
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        title,
                                    );
                                },
                            );
                        });
                    } else {
                        title(ui);
                        ui.horizontal_wrapped(|ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            for (mode, label, tip) in PREVIEW_MODES {
                                ui.selectable_value(&mut preview_mode, mode, label)
                                    .on_hover_text(tip);
                            }
                        });
                    }
                    if preview_mode == PreviewMode::Basic {
                        ui.horizontal_wrapped(|ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            ui.selectable_value(&mut basic_dialect, None, "AUTO")
                                .on_hover_text(
                                    "guess: BASIC 7.0 for C128 programs, Simons' BASIC when its \
                                 keywords are used, otherwise V2",
                                );
                            for (d, label, tip) in BASIC_DIALECTS {
                                ui.selectable_value(&mut basic_dialect, Some(d), label)
                                    .on_hover_text(tip);
                            }
                        });
                    }
                    if preview_mode == PreviewMode::Pic {
                        ui.horizontal_wrapped(|ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            ui.label("format");
                            let name = pic_format.map_or("AUTO", |f| f.name());
                            egui::ComboBox::from_id_salt("pic_format")
                                .selected_text(name)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut pic_format, None, "AUTO")
                                        .on_hover_text("go by the file's load address and size");
                                    for f in PicFormat::ALL {
                                        ui.selectable_value(&mut pic_format, Some(f), f.name());
                                    }
                                });
                        });
                    }
                    if preview_mode == PreviewMode::Gfx {
                        ui.horizontal_wrapped(|ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
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
                    // A Find match to highlight in this file, and scroll to.
                    let mark = self
                        .find
                        .mark
                        .filter(|&(n, ..)| {
                            hex && find_live
                                && Some(n) == self.pane.selected
                                // Still the same file (the list can change).
                                && self.find.hits.iter().any(|h| {
                                    h.node == n
                                        && self.pane.nodes.get(n).is_some_and(|x| x.name == h.name)
                                })
                        })
                        .map(|(_, from, len)| (from, len));
                    // Hex dumps and disassembly don't wrap: they scroll sideways
                    // in a narrow pane rather than run past its edge.
                    let unwrapped = hex || preview_mode == PreviewMode::Dis;
                    let mut area =
                        egui::ScrollArea::new([unwrapped, true]).id_salt("preview_scroll");
                    if let Some((from, _)) = mark.filter(|_| self.find.scroll) {
                        let font = egui::TextStyle::Monospace.resolve(ui.style());
                        let row = ui.fonts(|f| f.row_height(&font));
                        let line = (from.min(PREVIEW_CAP) / 16) as f32;
                        area = area.vertical_scroll_offset(((line - 3.0) * row).max(0.0));
                        self.find.scroll = false;
                    }
                    area.show(ui, |ui| {
                        if preview.is_empty() {
                            ui.label(
                                egui::RichText::new("Select a file to preview its bytes.")
                                    .color(c64::TEXT_DIM),
                            );
                        } else if let (PreviewMode::Gfx, Some((i, Ok(data), badge))) = (
                            preview_mode,
                            self.pane.selected.and_then(|i| {
                                self.pane
                                    .nodes
                                    .get(i)
                                    .map(|n| (i, n.data.as_ref(), &n.badge))
                            }),
                        ) {
                            gfx_ui(ui, &mut gfx_cache, i, data, badge, gfx_kind, gfx_multi);
                        } else if let (PreviewMode::Pic, Some((i, Ok(data)))) = (
                            preview_mode,
                            self.pane
                                .selected
                                .and_then(|i| self.pane.nodes.get(i).map(|n| (i, n.data.as_ref()))),
                        ) {
                            pic_ui(ui, &mut pic_cache, i, data, pic_format);
                        } else if preview_mode == PreviewMode::Dis {
                            let job = dis_layout(
                                &preview,
                                egui::TextStyle::Monospace.resolve(ui.style()),
                                ui.visuals().text_color(),
                            );
                            ui.add(egui::Label::new(job).extend());
                        } else if preview_mode == PreviewMode::Basic {
                            let job = basic_layout(
                                &preview,
                                egui::TextStyle::Monospace.resolve(ui.style()),
                                ui.visuals().text_color(),
                                ui.available_width(),
                            );
                            ui.add(egui::Label::new(job));
                        } else if let Some((from, len)) = mark {
                            let job = hex_marked_layout(
                                &preview,
                                from,
                                len,
                                egui::TextStyle::Monospace.resolve(ui.style()),
                                ui.visuals().text_color(),
                            );
                            ui.add(egui::Label::new(job).extend());
                        } else {
                            let label = egui::Label::new(egui::RichText::new(&preview).monospace());
                            ui.add(if hex { label.extend() } else { label.wrap() });
                        }
                    });
                });
        }

        let mut back_filter = std::mem::take(&mut self.back.filter);
        let mut back_rect = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut focused = |ui: &mut egui::Ui, acts: &mut Vec<Act>| {
                pane_ui(
                    ui,
                    &ic,
                    &self.pane,
                    &mut filter,
                    Some((&mut renaming, &mut rename_to, &mut rename_focus)),
                    if self.focus_right { "right" } else { "left" },
                    &self.recent,
                    self.dual && back_editable,
                    acts,
                )
            };
            if !self.dual {
                focused(ui, &mut acts);
                return;
            }
            // Two panes side by side, each framed; the focused one's
            // frame is bright.
            let mut focused = Some(focused);
            ui.columns(2, |cols| {
                for (k, col) in cols.iter_mut().enumerate() {
                    let is_focused = (k == 1) == self.focus_right;
                    let stroke = if is_focused { c64::ACCENT } else { c64::WELL };
                    let r = egui::Frame::none()
                        .stroke(egui::Stroke::new(2.0, stroke))
                        .inner_margin(egui::Margin::same(4.0))
                        .show(col, |ui| {
                            ui.set_min_size(ui.available_size());
                            if is_focused {
                                if let Some(mut f) = focused.take() {
                                    f(ui, &mut acts);
                                }
                            } else {
                                let side = if k == 1 { "right" } else { "left" };
                                pane_ui(
                                    ui,
                                    &ic,
                                    &self.back,
                                    &mut back_filter,
                                    None,
                                    side,
                                    &self.recent,
                                    false,
                                    &mut acts,
                                );
                            }
                        });
                    if !is_focused {
                        back_rect = Some((r.response.rect, r.response.layer_id));
                    }
                }
            });
        });
        // A press anywhere in the other pane focuses it first, so the click,
        // menu or typing that follows acts on it.
        if let Some((rect, layer)) = back_rect {
            let pressed_in = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| rect.contains(p))
            }) && ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| ctx.layer_id_at(p) == Some(layer));
            if pressed_in {
                acts.insert(0, Act::FocusOther);
            }
        }

        // Validation window: what the check found, and the fixes it offers.
        if self.show_validation {
            if !show_help
                && !show_map
                && !show_sector
                && !show_copy
                && !show_compare
                && ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                self.show_validation = false;
            }
            let screen = ctx.screen_rect();
            let mut close = false;
            egui::Window::new(
                egui::RichText::new("DISK VALIDATION")
                    .color(c64::ACCENT)
                    .strong(),
            )
            .collapsible(false)
            .resizable(false)
            .max_height(screen.height() - 60.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(640.0_f32.min(screen.width() - 60.0));
                validation_ui(
                    ui,
                    &self.validation,
                    self.validation_bad,
                    &mut self.repair,
                    &mut close,
                    &mut acts,
                );
            });
            if close {
                self.show_validation = false;
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
            if self.pane.map_cache.is_none() {
                self.pane.map_cache = self.pane.map_disk().map(Disk::block_map);
            }
            if !show_help && !show_sector && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                show_map = false;
            }
            if let Some(map) = &self.pane.map_cache {
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
                        let (clicked, opened) = disk_map_ui(ui, map, sel_now);
                        if let Some(i) = clicked {
                            acts.push(Act::Select(i));
                        }
                        if let Some((t, s)) = opened {
                            acts.push(Act::SectorOpen(t, s));
                        }
                    });
            }
        }

        // Sector Editor window: one block in hex, edits staged until written.
        if show_sector {
            if self.pane.map_cache.is_none() {
                self.pane.map_cache = self.pane.map_disk().map(Disk::block_map);
            }
            if !show_help && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                show_sector = false;
            }
            let writable = self.pane.disk.is_some();
            if let (Some(disk), Some(map)) = (self.pane.map_disk(), &self.pane.map_cache) {
                let screen = ctx.screen_rect();
                egui::Window::new(
                    egui::RichText::new("SECTOR EDITOR")
                        .color(c64::ACCENT)
                        .strong(),
                )
                .collapsible(false)
                .resizable(false)
                .max_height(screen.height() - 60.0)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(screen.center())
                .show(ctx, |ui| {
                    let view = SectorView {
                        disk,
                        map,
                        selected: sel_now,
                        writable,
                    };
                    sector_ui(ui, &mut self.sector, &view, &mut show_sector, &mut acts);
                });
            }
        }

        // Find window: search the listing's files for text or bytes.
        if show_find {
            if !show_help
                && !show_map
                && !show_sector
                && !show_copy
                && !show_compare
                && !show_basic
                && !self.show_validation
                && ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                show_find = false;
            }
            let source = self.sector_source();
            let stale = !self.find.source.is_empty() && self.find.source != source;
            let on_disk = self.pane.map_disk().is_some();
            let screen = ctx.screen_rect();
            egui::Window::new(egui::RichText::new("FIND").color(c64::ACCENT).strong())
                .collapsible(false)
                .resizable(false)
                .max_height(screen.height() - 80.0)
                .pivot(egui::Align2::RIGHT_TOP)
                .default_pos(screen.right_top() + egui::vec2(-24.0, 70.0))
                .show(ctx, |ui| {
                    find_ui(
                        ui,
                        &mut self.find,
                        stale,
                        on_disk,
                        &mut show_find,
                        &mut acts,
                    );
                });
        }

        // Copy window: a second disk to copy files to and from.
        if show_copy {
            if !show_help
                && !show_map
                && !show_sector
                && ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                show_copy = false;
            }
            let screen = ctx.screen_rect();
            let chosen = self.chosen_rows().len();
            let can_receive = self.pane.editable();
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

        // Compare window: the two panes' files side by side, by name. It shows
        // both panes, so it brings Dual back if Preview had hidden one.
        if show_compare {
            if !dual {
                dual = true;
                show_preview = false;
            }
            if !show_help
                && !show_map
                && !show_sector
                && !show_copy
                && ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                show_compare = false;
            }
            let (left, right) = if self.focus_right {
                (&self.back, &self.pane)
            } else {
                (&self.pane, &self.back)
            };
            let rows = sector64::compare::compare(&compare_items(left), &compare_items(right));
            let screen = ctx.screen_rect();
            egui::Window::new(egui::RichText::new("COMPARE").color(c64::ACCENT).strong())
                .collapsible(false)
                .resizable(true)
                .default_width(680.0_f32.min(screen.width() - 40.0))
                .max_height(screen.height() - 60.0)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(screen.center())
                .show(ctx, |ui| {
                    compare_ui(
                        ui,
                        [left, right],
                        &rows,
                        &mut compare_all,
                        &mut show_compare,
                        &mut acts,
                    );
                });
        }

        // BASIC editor window. Esc leaves the text first, then closes it.
        if show_basic {
            if !basic.typing
                && !show_help
                && !show_map
                && !show_sector
                && !show_copy
                && !show_compare
                && !self.show_validation
                && ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                show_basic = false;
            }
            let screen = ctx.screen_rect();
            let exists = match (&self.pane.disk, &self.pane.archive) {
                (Some(ed), _) => ed
                    .disk
                    .find_entry(&basic.disk_name())
                    .is_ok_and(|e| e.is_some()),
                (None, Some(stage)) => stage.iter().any(|f| f.name == basic.disk_name()),
                (None, None) => false,
            };
            egui::Window::new(
                egui::RichText::new("BASIC EDITOR")
                    .color(c64::ACCENT)
                    .strong(),
            )
            .collapsible(false)
            .resizable(true)
            .default_size([
                680.0_f32.min(screen.width() - 40.0),
                460.0_f32.min(screen.height() - 80.0),
            ])
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(screen.center())
            .show(ctx, |ui| {
                basic_editor_ui(
                    ui,
                    &mut basic,
                    BasicTarget {
                        editable,
                        is_archive,
                        exists,
                        selected: sel_now.is_some(),
                    },
                    &mut show_basic,
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
                let back_dirty = quitting && self.back.is_dirty();
                let mut which = Vec::new();
                if self.pane.is_dirty() {
                    which.push(if self.dual {
                        "this pane's disk"
                    } else {
                        "the open disk"
                    });
                }
                if back_dirty {
                    which.push("the other pane's disk");
                }
                if quitting && other_dirty {
                    which.push("the Copy window's disk");
                }
                ui.label(unsaved_message(&which));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if (is_disk || back_dirty || quitting && other_dirty)
                        && ui.button("💾 Save first").clicked()
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
                        for pane in [&mut self.pane, &mut self.back] {
                            if let Some(ed) = &mut pane.disk {
                                ed.dirty = false;
                            }
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
                    if self.pane.is_dirty() {
                        self.save(false);
                    }
                    let quitting = matches!(self.confirm, Some(Confirm::Quit));
                    if quitting && self.back.is_dirty() {
                        // Save it as the focused pane, then swap back.
                        std::mem::swap(&mut self.pane, &mut self.back);
                        self.save(false);
                        std::mem::swap(&mut self.pane, &mut self.back);
                    }
                    if quitting && self.other.as_ref().is_some_and(|o| o.dirty) {
                        self.save_other(false);
                    }
                    let other_dirty = quitting
                        && (self.back.is_dirty() || self.other.as_ref().is_some_and(|o| o.dirty));
                    if !self.pane.is_dirty() && !other_dirty {
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
        // In dual-pane view they go to the pane they were dropped on.
        let mut drop_editable = editable;
        if let (false, Some((rect, _))) = (dropped.is_empty(), back_rect) {
            if ctx
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|p| rect.contains(p))
            {
                acts.push(Act::FocusOther);
                drop_editable = back_editable;
            }
        }
        if drop_editable {
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
        let n = self.pane.nodes.len();
        let sel_name = sel_now
            .and_then(|s| self.pane.nodes.get(s))
            .map(|x| x.name.clone());
        let order = self.pane.sorted_order(&filter);
        if tab_switch {
            acts.push(Act::FocusOther);
            // Tab also moves egui's keyboard focus to the next widget; undo
            // that so the keys keep working on the list.
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
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
            if i.modifiers.command && i.key_pressed(egui::Key::E) && has_map {
                show_sector = !show_sector;
            }
            if i.modifiers.command && i.key_pressed(egui::Key::F) {
                show_find = !show_find;
            }
            if i.modifiers.command && i.key_pressed(egui::Key::D) && can_compare {
                show_compare = !show_compare;
            }
            if i.modifiers.command && i.key_pressed(egui::Key::B) {
                show_basic = !show_basic;
            }
            if i.modifiers.command && i.key_pressed(egui::Key::K) {
                show_copy = !show_copy;
            }
            if i.modifiers.command && i.key_pressed(egui::Key::L) && is_disk {
                acts.push(Act::Validate);
            }
            if i.modifiers.command && i.key_pressed(egui::Key::R) && can_run {
                acts.push(Act::RunVice);
            }
            if self.dual && i.key_pressed(egui::Key::F5) {
                acts.push(Act::CopyToPane);
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
                // A Mac's "delete" key is Backspace.
                let delete = i.key_pressed(egui::Key::Delete)
                    || cfg!(target_os = "macos") && i.key_pressed(egui::Key::Backspace);
                if delete && editable && has_sel {
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
        self.pane.filter = filter;
        self.back.filter = back_filter;
        self.out_dir = out_dir;
        self.new_name = new_name;
        self.new_id = new_id;
        self.pane.rename_to = rename_to;
        self.new_fmt = new_fmt;
        self.add_type = add_type;
        self.pane.renaming = renaming;
        self.pane.rename_focus = rename_focus;
        self.preview_mode = preview_mode;
        self.basic_dialect = basic_dialect;
        self.gfx_kind = gfx_kind;
        self.gfx_multi = gfx_multi;
        self.pane.gfx_cache = gfx_cache;
        self.pic_format = pic_format;
        self.pane.pic_cache = pic_cache;
        self.show_preview = show_preview;
        self.show_help = show_help;
        self.show_map = show_map;
        self.show_sector = show_sector;
        if show_find && !self.show_find {
            self.find.focus = true;
        }
        self.show_find = show_find;
        self.show_copy = show_copy;
        self.show_compare = show_compare;
        self.compare_all = compare_all;
        self.show_basic = show_basic;
        self.basic = basic;
        self.dual = dual;
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

/// The inline-rename state a pane's list edits: the row being renamed, the
/// text typed so far, and whether the field should take focus this frame.
type RenameState<'a> = (&'a mut Option<usize>, &'a mut String, &'a mut bool);

/// Draw one pane: the welcome screen when it's empty, otherwise the header,
/// the filter box and the file list. `side` keeps each pane's widget ids
/// apart; `rename` is given only to the focused pane, the only one that
/// renames in place. Clicks and menu picks become `acts`.
#[allow(clippy::too_many_arguments)]
fn pane_ui(
    ui: &mut egui::Ui,
    ic: &Icons,
    pane: &Pane,
    filter: &mut String,
    mut rename: Option<RenameState>,
    side: &str,
    recent: &[String],
    can_copy_across: bool,
    acts: &mut Vec<Act>,
) {
    let editable = pane.editable();
    let is_disk = pane.disk.is_some();
    let is_archive = pane.archive.is_some();
    let cols = Cols::of(&pane.nodes);
    // Only the focused pane can rename in place.
    let active = rename.is_some();
    if pane.nodes.is_empty() {
        // The logo, the name, and what to do first.
        let logo = ic.logo.id();
        let intro = |ui: &mut egui::Ui| {
            ui.add(egui::Image::new((logo, egui::vec2(48.0, 48.0))));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("SECTOR64")
                    .heading()
                    .color(c64::ACCENT)
                    .strong(),
            );
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(
                    "Open a disk or archive, or make a new image,\n\
                     then drag files in to add them.",
                )
                .color(c64::TEXT_DIM),
            );
        };
        if recent.is_empty() || pane.editable() || pane.opened_path.is_some() {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.5 - 70.0).max(8.0));
                intro(ui);
            });
        } else {
            // Start screen: the intro, then the recent files as links.
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.25).max(8.0));
                intro(ui);
                ui.add_space(16.0);
                ui.label(egui::RichText::new("RECENT FILES").color(c64::ACCENT));
                ui.add_space(4.0);
                for (label, path) in recent_labels(recent) {
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
    if !pane.title.is_empty() {
        egui::Frame::none()
            .fill(c64::BG_PANEL)
            .stroke(egui::Stroke::new(1.0, c64::ACCENT_DIM))
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(&pane.title).color(c64::BRIGHT).strong())
                        .truncate(),
                );
            });
        ui.add_space(2.0);
    }
    // Filter box: live, case-insensitive name filter.
    let order = pane.sorted_order(filter);
    ui.horizontal(|ui| {
        ui.label("Filter:");
        if !filter.trim().is_empty() {
            ui.label(
                egui::RichText::new(format!("({}/{})", order.len(), pane.nodes.len()))
                    .color(c64::TEXT_DIM),
            );
            if ui.button("X").on_hover_text("clear filter").clicked() {
                filter.clear();
            }
        }
        ui.add(
            egui::TextEdit::singleline(filter)
                // Stable id so focus survives the count/clear widgets
                // appearing before the field once it has text.
                .id(egui::Id::new(("filter_box", side)))
                .hint_text("type to filter by name")
                .desired_width(f32::INFINITY),
        );
    });
    ui.add_space(2.0);
    // Auto-pad columns to the widest row so they line up.
    // Column header (reserves the scrollbar width so it lines up with
    // the rows below), clickable to sort.
    let bar = ui.spacing().scroll.bar_width + 2.0;
    let disk_columns = is_disk || pane.read_only_disk;
    if let Some(key) = header_row(ui, cols, disk_columns, bar, pane.sort) {
        acts.push(Act::Sort(key));
    }
    egui::ScrollArea::vertical()
        .id_salt(("list", side))
        .show(ui, |ui| {
            for (pos, &i) in order.iter().enumerate() {
                let node = &pane.nodes[i];
                let sel = pane.selected == Some(i) || pane.marked.contains(&i);
                let extractable = node.data.is_ok();
                let editing = rename.as_ref().is_some_and(|r| *r.0 == Some(i));
                let (edit, focus) = match (&mut rename, editing) {
                    (Some((_, text, focus)), true) => (Some(&mut **text), **focus),
                    _ => (None, false),
                };
                let row_sel = match (sel, active) {
                    (false, _) => RowSel::No,
                    (true, true) => RowSel::Yes,
                    (true, false) => RowSel::Unfocused,
                };
                let (resp, result) = file_row(ui, node, row_sel, pos, cols, edit, focus);
                if let (Some((renaming, _, focus)), true) = (&mut rename, editing) {
                    **focus = false;
                    match result {
                        RenameEdit::Commit => {
                            acts.push(Act::Select(i));
                            acts.push(Act::Rename);
                            **renaming = None;
                        }
                        RenameEdit::Cancel => **renaming = None,
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
                    if ui
                        .add_enabled(extractable, icon_button(&ic.down, "Extract"))
                        .clicked()
                    {
                        acts.push(Act::ExtractSelected);
                        ui.close_menu();
                    }
                    if can_copy_across
                        && ui
                            .add(
                                icon_button(&ic.blank, "Copy to Other Pane")
                                    .shortcut_text(key_hint("F5")),
                            )
                            .clicked()
                    {
                        acts.push(Act::CopyToPane);
                        ui.close_menu();
                    }
                    if editable {
                        ui.separator();
                        if ui
                            .add(icon_button(&ic.pencil, "Rename").shortcut_text(key_hint("F2")))
                            .clicked()
                        {
                            acts.push(Act::Select(i));
                            acts.push(Act::StartRename(i));
                            ui.close_menu();
                        }
                        let del = if is_archive { "Remove" } else { "Delete" };
                        if ui
                            .add(icon_button(&ic.trash, del).shortcut_text(key_hint("Del")))
                            .clicked()
                        {
                            acts.push(Act::Select(i)); // delete this row specifically
                            acts.push(Act::Delete);
                            ui.close_menu();
                        }
                    }
                });
            }
        });
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

/// How a row shows selection: in the focused pane, a selected row is filled
/// bright; in the other pane, it's shaded softly.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowSel {
    No,
    Yes,
    Unfocused,
}

/// Draw one file as a full-width, painted, selectable row: a colour-coded type
/// badge, the name, its detail, and a right-aligned size (or an error). When
/// `edit` is `Some`, the name is shown as an editable field instead.
fn file_row(
    ui: &mut egui::Ui,
    node: &Node,
    sel: RowSel,
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
    let selected = sel == RowSel::Yes;
    if selected {
        painter.rect_filled(rect, Rounding::ZERO, c64::ACCENT);
    } else if sel == RowSel::Unfocused || resp.hovered() {
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
/// per track and a row per sector. Returns the file whose block was clicked,
/// and the block that was double-clicked (to open in the Sector Editor).
fn disk_map_ui(
    ui: &mut egui::Ui,
    map: &BlockMap,
    selected: Option<usize>,
) -> (Option<usize>, Option<(u8, u8)>) {
    let tracks = map.tracks.len();
    let rows = map.tracks.iter().map(Vec::len).max().unwrap_or(0);
    if tracks == 0 || rows == 0 {
        ui.label("(this disk has no blocks to map)");
        return (None, None);
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
    let mut opened = None;
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
            if resp.double_clicked() {
                opened = hovered;
            }
            if let Some((t, s)) = hovered {
                let tip =
                    block_tooltip(map, t, s) + "\n(double-click to open it in the Sector Editor)";
                resp.on_hover_text_at_pointer(tip);
            }
        });
    (clicked, opened)
}

/// Draw the Find window: the query and its kind, then each matching file
/// with its match offsets as buttons.
fn find_ui(
    ui: &mut egui::Ui,
    f: &mut FindState,
    stale: bool,
    on_disk: bool,
    open: &mut bool,
    acts: &mut Vec<Act>,
) {
    let w = 440.0_f32.min(ui.ctx().screen_rect().width() - 60.0);
    ui.set_width(w);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut f.hex, false, "TEXT")
            .on_hover_text("letters match in either case, as the C64 shows them");
        ui.selectable_value(&mut f.hex, true, "HEX")
            .on_hover_text("bytes such as A9 00 8D; ?? matches any byte");
        let hint = if f.hex {
            "e.g. A9 00 8D ?? D0"
        } else {
            "text to look for"
        };
        let edit = ui.add(
            egui::TextEdit::singleline(&mut f.query)
                .hint_text(hint)
                .desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut f.focus) {
            edit.request_focus();
        }
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            acts.push(Act::Find);
            // Keep the keyboard here, so Enter doesn't also extract a file.
            edit.request_focus();
        }
    });
    ui.horizontal_wrapped(|ui| {
        // Wrap whole choices onto the next line, never inside one.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        if ui.button("Find").clicked() {
            acts.push(Act::Find);
        }
        ui.label(egui::RichText::new("show in").color(c64::TEXT_DIM));
        ui.selectable_value(&mut f.in_sector, false, "PREVIEW")
            .on_hover_text("highlight the match in the hex preview");
        ui.add_enabled_ui(on_disk, |ui| {
            ui.selectable_value(&mut f.in_sector, true, "SECTOR EDITOR")
                .on_hover_text("open the block holding the match, with the cursor on it")
                .on_disabled_hover_text("only disk images have sectors");
        });
    });
    find_results_ui(ui, f, stale, acts);
    ui.separator();
    if ui.button("Close").clicked() {
        *open = false;
    }
}

/// The Find window's summary line and its list of matching files.
fn find_results_ui(ui: &mut egui::Ui, f: &FindState, stale: bool, acts: &mut Vec<Act>) {
    let note = if stale {
        "Another listing is showing; Find searches it."
    } else if f.summary.is_empty() {
        "Searches every file in the listing (and, for text, their names)."
    } else {
        ""
    };
    if !note.is_empty() {
        ui.add(egui::Label::new(egui::RichText::new(note).color(c64::TEXT_DIM)).wrap());
        return;
    }
    let color = if f.summary.starts_with('⚠') {
        c64::WARN
    } else {
        c64::TEXT_DIM
    };
    ui.add(egui::Label::new(egui::RichText::new(&f.summary).color(color)).wrap());
    if f.hits.is_empty() {
        return;
    }
    ui.separator();
    let mark = f.mark;
    egui::ScrollArea::vertical()
        .id_salt("find_results")
        .max_height(320.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for h in &f.hits {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(h.badge.to_uppercase()).color(badge_color(&h.badge)),
                    );
                    ui.label(egui::RichText::new(&h.name).color(c64::TEXT));
                    let mut what = Vec::new();
                    if h.in_name {
                        what.push("name".to_string());
                    }
                    if h.total > 0 {
                        what.push(format!(
                            "{} match{}",
                            h.total,
                            if h.total == 1 { "" } else { "es" }
                        ));
                    }
                    ui.label(egui::RichText::new(what.join(" · ")).color(c64::TEXT_DIM));
                });
                if h.offsets.is_empty() {
                    continue;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(12.0);
                    for &off in &h.offsets {
                        let on = mark.is_some_and(|(n, o, _)| n == h.node && o == off);
                        let tip = if f.in_sector && h.on_disk {
                            "open the block holding it in the Sector Editor"
                        } else {
                            "show it in the hex preview"
                        };
                        if ui
                            .selectable_label(on, format!("${off:04X}"))
                            .on_hover_text(tip)
                            .clicked()
                        {
                            acts.push(Act::FindShow(h.node, off));
                        }
                    }
                    if h.total > h.offsets.len() {
                        ui.label(
                            egui::RichText::new(format!("+{} more", h.total - h.offsets.len()))
                                .color(c64::TEXT_DIM),
                        );
                    }
                });
            }
        });
}

/// What the Sector Editor shows a block of, and what it may do to it.
struct SectorView<'a> {
    disk: &'a Disk,
    map: &'a BlockMap,
    /// The listing's selected file (its index in the map's files).
    selected: Option<usize>,
    /// Whether the disk can be changed (not a G64 or other read-only one).
    writable: bool,
}

/// Columns of the Sector Editor's grid, in characters: a two-digit offset,
/// sixteen bytes in hex (an extra space after the eighth), then the text.
const SEC_HEX0: usize = 4;
const SEC_TEXT0: usize = SEC_HEX0 + 16 * 3 + 1 + 2;
const SEC_COLS: usize = SEC_TEXT0 + 16;

/// The grid column where byte `i` of a row starts in hex.
fn sec_hex_col(i: usize) -> usize {
    SEC_HEX0 + i * 3 + usize::from(i >= 8)
}

/// The byte (0..16) under grid column `col` of a row, in either the hex or
/// the text part.
fn sec_byte_at(col: usize) -> Option<usize> {
    if (SEC_TEXT0..SEC_COLS).contains(&col) {
        return Some(col - SEC_TEXT0);
    }
    (0..16).find(|&i| (sec_hex_col(i)..sec_hex_col(i) + 2).contains(&col))
}

/// Draw the Sector Editor: where it is and how to move, who owns the block,
/// its link, the hex grid, and Write / Revert.
fn sector_ui(
    ui: &mut egui::Ui,
    ed: &mut sector::SectorEditor,
    v: &SectorView,
    open: &mut bool,
    acts: &mut Vec<Act>,
) {
    use sector::{SectorEditor, SIZE};
    let disk = v.disk;
    let idle = !ed.modified();
    let busy = "write or revert your changes first";
    let g = disk.geometry();

    // The grid's font: the usual monospace, smaller if the grid wouldn't fit
    // the screen (the C64 font is wide). The window is as wide as the grid.
    let mut font = egui::TextStyle::Monospace.resolve(ui.style());
    let cw = ui.fonts(|f| f.glyph_width(&font, '0'));
    let fit = (ui.ctx().screen_rect().width() - 80.0) / (SEC_COLS as f32 * cw);
    if fit < 1.0 {
        font.size = (font.size * fit).floor().max(7.0);
    }
    let cw = ui.fonts(|f| f.glyph_width(&font, '0'));
    let rh = ui.fonts(|f| f.row_height(&font)) + 3.0;
    let size = egui::vec2(SEC_COLS as f32 * cw, 16.0 * rh);
    ui.set_max_width(size.x);

    ui.horizontal_wrapped(|ui| {
        let (mut t, mut s) = (ed.track, ed.sector);
        ui.label("Track");
        ui.add_enabled(idle, egui::DragValue::new(&mut t).range(1..=g.tracks()))
            .on_disabled_hover_text(busy);
        let last = g
            .sector_count(t)
            .map_or(0, |n| n.saturating_sub(1))
            .min(255) as u8;
        ui.label("Sector");
        ui.add_enabled(idle, egui::DragValue::new(&mut s).range(0..=last))
            .on_disabled_hover_text(busy);
        if (t, s) != (ed.track, ed.sector) {
            ed.load(disk, t, s.min(last));
        }
        let nav = |ui: &mut egui::Ui, ok: bool, text: &str, tip: &str| {
            ui.add_enabled(idle && ok, egui::Button::new(text))
                .on_hover_text(tip)
                .on_disabled_hover_text(if idle { tip } else { busy })
                .clicked()
        };
        let (tr, se) = (ed.track, ed.sector);
        let prev = SectorEditor::neighbour(disk, tr, se, -1).is_some();
        if nav(ui, prev, "◀ Prev", "the block before this one (PgUp)") {
            ed.step(disk, -1);
        }
        let next = SectorEditor::neighbour(disk, tr, se, 1).is_some();
        if nav(ui, next, "Next ▶", "the block after this one (PgDn)") {
            ed.step(disk, 1);
        }
    });
    ui.horizontal_wrapped(|ui| {
        let nav = |ui: &mut egui::Ui, ok: bool, text: &str, tip: &str| {
            ui.add_enabled(idle && ok, egui::Button::new(text))
                .on_hover_text(tip)
                .on_disabled_hover_text(if idle { tip } else { busy })
                .clicked()
        };
        let linked = matches!(ed.link(disk), Some(sector::Link::Next(..)));
        if nav(
            ui,
            linked,
            "Follow link",
            "go to the block bytes 0–1 point to",
        ) {
            ed.follow(disk);
        }
        if nav(
            ui,
            ed.can_go_back(),
            "Back",
            "return to the block you followed a link from",
        ) {
            ed.back(disk);
        }
        let first = v
            .selected
            .and_then(|i| v.map.chains.get(i))
            .and_then(|c| c.first().copied());
        if nav(
            ui,
            first.is_some(),
            "Selected file",
            "go to the first block of the file selected in the list",
        ) {
            if let Some((t, s)) = first {
                ed.jump(disk, t, s);
            }
        }
    });

    // Who owns the block, and where its link goes.
    let owner = block_tooltip(v.map, ed.track, ed.sector);
    let owner: Vec<&str> = owner.lines().skip(1).collect();
    if !owner.is_empty() {
        ui.label(egui::RichText::new(owner.join(" · ")).color(c64::TEXT_DIM));
    }
    if let Some(err) = &ed.error {
        ui.label(egui::RichText::new(format!("⚠ can't read this block: {err}")).color(c64::WARN));
        if ui.button("Close").clicked() {
            *open = false;
        }
        return;
    }
    if let Some(link) = ed.link(disk) {
        let color = if matches!(link, sector::Link::Invalid(..)) {
            c64::WARN
        } else {
            c64::TEXT_DIM
        };
        ui.label(
            egui::RichText::new(format!("Bytes 0–1: {}", sector::describe(link))).color(color),
        );
    }
    ui.add_space(4.0);

    // The grid.
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        resp.request_focus();
        if let Some(p) = resp.interact_pointer_pos() {
            let col = ((p.x - rect.min.x) / cw) as usize;
            let row = ((p.y - rect.min.y) / rh) as usize;
            if let Some(i) = sec_byte_at(col).filter(|_| row < 16) {
                ed.place(row * 16 + i);
            }
        }
    }
    let focused = resp.has_focus();
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    tab: false,
                    escape: false,
                },
            )
        });
        let events = ui.input(|i| i.events.clone());
        for e in events {
            match e {
                egui::Event::Text(text) => {
                    for d in text.chars().filter_map(sector::hex_digit) {
                        if v.writable {
                            ed.type_digit(d);
                        }
                    }
                }
                egui::Event::Key {
                    key, pressed: true, ..
                } => match key {
                    egui::Key::ArrowLeft => ed.move_cursor(-1),
                    egui::Key::ArrowRight => ed.move_cursor(1),
                    egui::Key::ArrowUp => ed.move_cursor(-16),
                    egui::Key::ArrowDown => ed.move_cursor(16),
                    egui::Key::Home => ed.place(ed.cursor / 16 * 16),
                    egui::Key::End => ed.place(ed.cursor / 16 * 16 + 15),
                    egui::Key::PageUp if !ed.modified() => {
                        ed.step(disk, -1);
                    }
                    egui::Key::PageDown if !ed.modified() => {
                        ed.step(disk, 1);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect.expand(2.0), 2.0, c64::WELL);
    let at = |col: usize, row: usize| rect.min + egui::vec2(col as f32 * cw, row as f32 * rh + 1.5);
    let cursor_fill = if focused {
        c64::ACCENT_DIM
    } else {
        c64::STRIPE
    };
    for row in 0..SIZE / 16 {
        painter.text(
            at(0, row),
            egui::Align2::LEFT_TOP,
            format!("{:02X}", row * 16),
            font.clone(),
            c64::TEXT_DIM,
        );
        for i in 0..16 {
            let n = row * 16 + i;
            let Some(&b) = ed.buf.get(n) else { continue };
            let color = if ed.changed(n) {
                c64::ORPHAN
            } else if n < 2 {
                c64::ACCENT
            } else {
                c64::TEXT
            };
            let (hx, tx) = (at(sec_hex_col(i), row), at(SEC_TEXT0 + i, row));
            if n == ed.cursor {
                let cell = |p: egui::Pos2, w: f32| {
                    egui::Rect::from_min_size(p - egui::vec2(0.0, 1.5), egui::vec2(w, rh))
                };
                painter.rect_filled(cell(hx, 2.0 * cw), 1.0, cursor_fill);
                painter.rect_filled(cell(tx, cw), 1.0, cursor_fill);
                if ed.low_nibble {
                    let y = hx.y + rh - 2.5;
                    painter.line_segment(
                        [egui::pos2(hx.x + cw, y), egui::pos2(hx.x + 2.0 * cw, y)],
                        egui::Stroke::new(2.0, c64::BRIGHT),
                    );
                }
            }
            painter.text(
                hx,
                egui::Align2::LEFT_TOP,
                format!("{b:02X}"),
                font.clone(),
                color,
            );
            painter.text(
                tx,
                egui::Align2::LEFT_TOP,
                sector::text_char(b).to_string(),
                font.clone(),
                color,
            );
        }
    }
    ui.add_space(4.0);

    // Where the cursor is, and Write / Revert.
    let changed = (0..SIZE).filter(|&i| ed.changed(i)).count();
    let b = ed.buf.get(ed.cursor).copied().unwrap_or(0);
    let mut info = format!("Offset ${0:02X} ({0}) = ${1:02X} ({1})", ed.cursor, b);
    if changed > 0 {
        info += &format!(
            " · {changed} byte{} changed",
            if changed == 1 { "" } else { "s" }
        );
    }
    ui.label(info);
    ui.horizontal(|ui| {
        if ui.button("Close").clicked() {
            *open = false;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let write_tip = "write the changed bytes to the disk (Undo takes them back; \
                             Save writes the disk file)";
            if ui
                .add_enabled(v.writable && changed > 0, egui::Button::new("Write"))
                .on_hover_text(write_tip)
                .on_disabled_hover_text(if v.writable {
                    "nothing has changed yet"
                } else {
                    "this disk is read-only"
                })
                .clicked()
            {
                acts.push(Act::SectorWrite);
            }
            if ui
                .add_enabled(changed > 0, egui::Button::new("Revert"))
                .on_hover_text("forget the changes typed into this block")
                .clicked()
            {
                ed.revert();
            }
        });
    });
    let hint = if v.writable {
        "Click a byte, then type hex digits. Arrows move; PgUp/PgDn step through blocks."
    } else {
        "This disk is read-only (view only). Arrows move; PgUp/PgDn step through blocks."
    };
    ui.label(egui::RichText::new(hint).color(c64::TEXT_DIM));
}

/// The preview pane's modes: what each toggle is called and does.
const PREVIEW_MODES: [(PreviewMode, &str, &str); 6] = [
    (PreviewMode::Hex, "HEX", "show an offset + hex + ASCII dump"),
    (PreviewMode::Text, "TEXT", "show the bytes as PETSCII text"),
    (
        PreviewMode::Basic,
        "BASIC",
        "list a BASIC program, like LIST on a C64",
    ),
    (
        PreviewMode::Dis,
        "DIS",
        "disassemble the bytes as 6502 machine code",
    ),
    (
        PreviewMode::Gfx,
        "GFX",
        "draw the bytes as sprites or characters",
    ),
    (
        PreviewMode::Pic,
        "PIC",
        "show a Koala Painter, Art Studio or Doodle! picture",
    ),
];

/// How wide the row of preview mode toggles is.
fn preview_toggles_width(ui: &egui::Ui) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let pad = 2.0 * ui.spacing().button_padding.x + ui.spacing().item_spacing.x;
    PREVIEW_MODES
        .iter()
        .map(|(_, label, _)| {
            let w = ui.fonts(|f| {
                f.layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE)
                    .size()
                    .x
            });
            w + pad
        })
        .sum()
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
        PreviewMode::Text
        | PreviewMode::Basic
        | PreviewMode::Gfx
        | PreviewMode::Dis
        | PreviewMode::Pic => TEXT_PREVIEW_COLS,
    };
    // "PREVIEW — NAME" plus the HEX / TEXT / BASIC / DIS / GFX / PIC toggle.
    let header_chars = "PREVIEW — ".chars().count() + name.map_or(0, |n| n.chars().count()) + 30;
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

/// How many bytes the hex preview shows.
const PREVIEW_CAP: usize = 16 * 1024;

/// The hex preview with bytes `from..from + len` highlighted, in both the
/// hex and the ASCII part (`text` is [`hexdump`]'s output).
fn hex_marked_layout(
    text: &str,
    from: usize,
    len: usize,
    font: egui::FontId,
    color: Color32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat::simple(font.clone(), color);
    let marked = egui::TextFormat {
        background: c64::ACCENT_DIM,
        color: c64::BRIGHT,
        ..egui::TextFormat::simple(font, color)
    };
    let in_mark = |b: usize| (from..from + len).contains(&b);
    for (row, line) in text.split_inclusive('\n').enumerate() {
        // Which byte of the row each character shows, if any: "OOOO: " then
        // sixteen "XX " (an extra space after the eighth), a space, the text.
        let byte_at = |c: usize| -> Option<usize> {
            if (6..6 + 49).contains(&c) {
                let k = c - 6;
                let k = if k >= 24 { k - 1 } else { k };
                (k % 3 < 2).then_some(k / 3)
            } else if (56..72).contains(&c) {
                Some(c - 56)
            } else {
                None
            }
        };
        let mut run = String::new();
        let mut run_marked = false;
        for (c, ch) in line.chars().enumerate() {
            let m = !line.starts_with('…') && byte_at(c).is_some_and(|i| in_mark(row * 16 + i));
            if m != run_marked && !run.is_empty() {
                let fmt = if run_marked { &marked } else { &plain };
                job.append(&std::mem::take(&mut run), 0.0, fmt.clone());
            }
            run_marked = m;
            run.push(ch);
        }
        if !run.is_empty() {
            let fmt = if run_marked { &marked } else { &plain };
            job.append(&run, 0.0, fmt.clone());
        }
    }
    job
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

/// The C64's 16 colours (Pepto's measured PAL values), by colour number.
const C64_PALETTE: [(u8, u8, u8, &str); 16] = [
    (0x00, 0x00, 0x00, "black"),
    (0xFF, 0xFF, 0xFF, "white"),
    (0x68, 0x37, 0x2B, "red"),
    (0x70, 0xA4, 0xB2, "cyan"),
    (0x6F, 0x3D, 0x86, "purple"),
    (0x58, 0x8D, 0x43, "green"),
    (0x35, 0x28, 0x79, "blue"),
    (0xB8, 0xC7, 0x6F, "yellow"),
    (0x6F, 0x4F, 0x25, "orange"),
    (0x43, 0x39, 0x00, "brown"),
    (0x9A, 0x67, 0x59, "light red"),
    (0x44, 0x44, 0x44, "dark grey"),
    (0x6C, 0x6C, 0x6C, "grey"),
    (0x9A, 0xD2, 0x84, "light green"),
    (0x6C, 0x5E, 0xB5, "light blue"),
    (0x95, 0x95, 0x95, "light grey"),
];

/// C64 colour number `c` (0–15) as an egui colour.
fn c64_color(c: u8) -> Color32 {
    let (r, g, b, _) = C64_PALETTE[(c & 15) as usize];
    Color32::from_rgb(r, g, b)
}

/// Why a file didn't decode as a picture, for the PIC preview.
fn pic_failure(data: &[u8], format: Option<PicFormat>) -> String {
    match format {
        Some(f) => format!("(too short for a {} picture)", f.name()),
        None => {
            let load = match data {
                [lo, hi, ..] => format!(
                    ", and this file loads at ${:04X}",
                    u16::from_le_bytes([*lo, *hi])
                ),
                _ => String::new(),
            };
            format!(
                "(not a picture this preview knows: Koala Painter pictures load at $6000, \
                 Art Studio and Advanced Art Studio at $2000, Doodle! at $5C00{load}. \
                 Choose a format above to show it as one anyway, or try GFX.)"
            )
        }
    }
}

/// The PIC preview: the selected file decoded as a paint program's picture,
/// framed in its border colour where the format saves one, scaled to fit the
/// pane. Hovering shows the pixel's position and colour.
fn pic_ui(
    ui: &mut egui::Ui,
    cache: &mut Option<PicView>,
    node: usize,
    data: &[u8],
    format: Option<PicFormat>,
) {
    use sector64::picture::{self, HEIGHT, WIDTH};
    let key = (node, format);
    if cache.as_ref().map(|c| c.key) != Some(key) {
        let decoded = match format {
            Some(f) => picture::decode_as(data, f),
            None => picture::decode(data),
        };
        let pic = decoded
            .map(|p| {
                let img = egui::ColorImage {
                    size: [WIDTH, HEIGHT],
                    pixels: p.pixels.iter().map(|&c| c64_color(c)).collect(),
                };
                let tex = ui
                    .ctx()
                    .load_texture("pic-preview", img, egui::TextureOptions::NEAREST);
                (p, tex)
            })
            .ok_or_else(|| pic_failure(data, format));
        *cache = Some(PicView { key, pic });
    }
    let view = cache.as_ref().unwrap();
    let (pic, tex) = match &view.pic {
        Ok(p) => p,
        Err(why) => {
            ui.add(egui::Label::new(egui::RichText::new(why).color(c64::TEXT_DIM)).wrap());
            return;
        }
    };
    let mode = if pic.format.multicolor() {
        "multicolour"
    } else {
        "hires"
    };
    ui.label(
        egui::RichText::new(format!("{} picture, {mode}, 320×200", pic.format.name()))
            .color(c64::TEXT_DIM),
    );
    // Whole-number scales stay crisp; a pane narrower than the picture
    // shrinks it to fit.
    let border = if pic.border.is_some() { 8.0 } else { 0.0 };
    let avail = (ui.available_width() - 2.0 * border).max(32.0);
    let fit = avail / WIDTH as f32;
    let scale = if fit >= 1.0 {
        fit.floor().min(4.0)
    } else {
        fit
    };
    let size = egui::vec2(WIDTH as f32, HEIGHT as f32) * scale;
    let frame = match pic.border {
        Some(b) => egui::Frame::none()
            .fill(c64_color(b))
            .inner_margin(egui::Margin::same(border)),
        None => egui::Frame::none(),
    };
    let resp = frame
        .show(ui, |ui| {
            ui.add(egui::Image::new((tex.id(), size)).sense(egui::Sense::hover()))
        })
        .inner;
    if let Some(pos) = resp.hover_pos() {
        let p = (pos - resp.rect.min) / scale;
        let (x, y) = (p.x.max(0.0) as usize, p.y.max(0.0) as usize);
        if x < WIDTH && y < HEIGHT {
            let c = pic.pixels[y * WIDTH + x];
            resp.on_hover_text_at_pointer(format!(
                "x {x}, y {y} (cell {}, {})\ncolour {c}: {}",
                x / 8,
                y / 8,
                C64_PALETTE[c as usize].3
            ));
        }
    }
    if let Some(b) = pic.border {
        ui.label(
            egui::RichText::new(format!("border: {}", C64_PALETTE[b as usize].3))
                .color(c64::TEXT_DIM),
        );
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

/// Most lines the DIS preview lists.
const DIS_MAX_LINES: usize = 8192;

/// The DIS preview: a note on where the listing starts, then one 6502
/// instruction per line. A PRG starts at the address a BASIC `SYS` line jumps
/// to, or else at its load address; any other file has no load address and
/// is listed from $0000.
fn dis_preview(data: &[u8], is_prg: bool) -> String {
    use sector64::disasm;
    let (addr, offset, note) = if is_prg {
        let Some(s) = disasm::code_start(data) else {
            return "(too short to hold any code)".into();
        };
        let note = if s.from_sys {
            format!(
                "; SYS {} → ${:04X} (loads at ${:04X})",
                s.addr, s.addr, s.load
            )
        } else if sector64::detokenize_basic(data).is_some() {
            format!(
                "; a BASIC program with no SYS into it (try BASIC);\n; listed from its load address, ${:04X}",
                s.load
            )
        } else {
            format!("; loads at ${:04X}", s.load)
        };
        (s.addr, s.offset, note)
    } else {
        (0, 0, "; no load address: listed from $0000".to_string())
    };
    let code = &data[offset.min(data.len())..];
    let lines = disasm::disassemble(code, addr, DIS_MAX_LINES);
    let mut out = note;
    out.push('\n');
    for l in &lines {
        out.push_str(&l.to_string());
        out.push('\n');
    }
    let shown: usize = lines.iter().map(|l| l.bytes.len()).sum();
    if shown < code.len() {
        out.push_str(&format!(
            "; … stopped after {DIS_MAX_LINES} instructions ({} more bytes)\n",
            code.len() - shown
        ));
    }
    out
}

/// Lay out a DIS listing: addresses in the accent colour, and notes and
/// undocumented opcodes (often a sign the bytes are data) dimmed.
fn dis_layout(text: &str, font: egui::FontId, color: egui::Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let fmt = |c| egui::TextFormat::simple(font.clone(), c);
    for line in text.lines() {
        let dim = line.starts_with(';') || line.get(15..16) == Some("*");
        match line.split_at_checked(4) {
            Some((addr, rest)) if !dim => {
                job.append(addr, 0.0, fmt(c64::ACCENT));
                job.append(rest, 0.0, fmt(color));
            }
            _ => job.append(line, 0.0, fmt(c64::TEXT_DIM)),
        }
        job.append("\n", 0.0, fmt(color));
    }
    job
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

/// What Run in VICE starts.
struct VicePlan {
    /// The file written to the temporary folder ("games.d64", "name.prg"):
    /// the disk, or else the selected program.
    file_name: String,
    /// The program to load from a disk, as VICE's `-autostart image:name`
    /// takes it.
    program: Option<String>,
    /// Run it on a C128 (it loads where C128 BASIC starts).
    c128: bool,
    /// The program, for the status line.
    what: String,
}

/// Whether a program loads where a C128 keeps BASIC ($1C01, or $4001 with a
/// graphics screen), so it should run on a C128.
fn is_c128_load(data: &[u8]) -> bool {
    matches!(data, [0x01, 0x1C, ..] | [0x01, 0x40, ..])
}

/// A directory name as VICE's autostart takes it. VICE types
/// `LOAD"name",8,1` with lowercase ASCII becoming PETSCII capitals, so
/// letters go in lowercase and plain characters as they are. Anything that
/// can't be typed, or that would end the name early (`"`, `,`, `:`), becomes
/// the `?` wildcard, which matches any one character.
fn vice_load_name(raw: &[u8; 16]) -> String {
    let name: String = raw
        .iter()
        .take_while(|&&b| b != 0xA0)
        .map(|&b| match b {
            b'A'..=b'Z' => b.to_ascii_lowercase() as char,
            b'"' | b',' | b':' => '?',
            0x20..=0x40 => b as char,
            _ => '?',
        })
        .collect();
    if name.is_empty() {
        "*".into()
    } else {
        name
    }
}

/// A listed name made safe as a file name (the emulator shows it).
fn safe_file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || " -_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() {
        "program".into()
    } else {
        s
    }
}

/// VICE's emulators to try, best first.
fn vice_names(c128: bool) -> &'static [&'static str] {
    if c128 {
        &["x128"]
    } else {
        &["x64sc", "x64"]
    }
}

/// Find a VICE emulator: in the chosen folder (or its `bin`, as VICE's
/// Windows and macOS downloads have it), then on the PATH, then (macOS) in
/// the usual places.
fn find_vice(names: &[&str], dir: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if !dir.is_empty() {
        dirs.push(PathBuf::from(dir));
        dirs.push(Path::new(dir).join("bin"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    #[cfg(target_os = "macos")]
    dirs.extend(mac::vice_dirs());
    let ext = std::env::consts::EXE_SUFFIX;
    names.iter().find_map(|n| {
        dirs.iter()
            .map(|d| d.join(format!("{n}{ext}")))
            .find(|p| p.is_file())
    })
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

/// The BASIC dialects, with a button label and a description each.
const BASIC_DIALECTS: [(BasicDialect, &str, &str); 3] = [
    (BasicDialect::V2, "V2", "Commodore BASIC V2 (C64, VIC-20)"),
    (BasicDialect::V7, "7.0", "the C128's BASIC 7.0"),
    (BasicDialect::Simons, "SIMONS", "Simons' BASIC (C64)"),
];

/// Where the BASIC editor's program would go.
struct BasicTarget {
    editable: bool,
    is_archive: bool,
    /// There's a file of that name already.
    exists: bool,
    /// A file is selected in the list.
    selected: bool,
}

/// The BASIC editor's contents: name, dialect, the text, what's wrong with
/// it, and the buttons.
fn basic_editor_ui(
    ui: &mut egui::Ui,
    ed: &mut BasicEditor,
    target: BasicTarget,
    open: &mut bool,
    acts: &mut Vec<Act>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Name");
        let name = ui.add(
            egui::TextEdit::singleline(&mut ed.name)
                .char_limit(16)
                .desired_width(150.0),
        );
        ed.typing = name.has_focus();
        ui.separator();
        for (d, label, tip) in BASIC_DIALECTS {
            if ui
                .selectable_label(ed.dialect == d, label)
                .on_hover_text(tip)
                .clicked()
                && ed.dialect != d
            {
                ed.dialect = d;
                ed.load = None;
                ed.checked = None;
            }
        }
        ui.separator();
        let sources = [
            (
                BasicSource::File,
                "Open Text…",
                "load a program's text from a file",
            ),
            (
                BasicSource::Selected,
                "List Selected",
                "load the program selected in the list",
            ),
        ];
        for (source, label, tip) in sources {
            let sure = ed.confirm == Some(source);
            let label = if sure { "Lose changes?" } else { label };
            let colour = if sure { c64::WARN } else { c64::TEXT };
            let usable = source == BasicSource::File || target.selected;
            if ui
                .add_enabled(
                    usable,
                    egui::Button::new(egui::RichText::new(label).color(colour)),
                )
                .on_hover_text(tip)
                .on_disabled_hover_text("select a BASIC program in the list first")
                .clicked()
            {
                if ed.modified && !ed.text.trim().is_empty() && !sure {
                    ed.confirm = Some(source);
                } else {
                    ed.confirm = None;
                    acts.push(match source {
                        BasicSource::File => Act::BasicOpen,
                        BasicSource::Selected => Act::BasicFromSelected,
                    });
                }
            }
        }
    });
    ui.add_space(2.0);

    // The buttons and messages sit at the bottom; the text gets the rest.
    egui::TopBottomPanel::bottom("basic_bottom")
        .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(0.0, 4.0)))
        .show_inside(ui, |ui| {
            basic_check_ui(ui, ed);
            ui.add_space(4.0);
            let ok = ed.check().is_ok();
            let named = !ed.disk_name().is_empty();
            ui.horizontal(|ui| {
                let to = if target.is_archive { "Archive" } else { "Disk" };
                let label = if target.exists && named {
                    format!("Replace on {to}")
                } else {
                    format!("Add to {to}")
                };
                let tip = if !target.editable {
                    "open an editable disk or start a new archive first"
                } else if !named {
                    "give the program a name first"
                } else {
                    "the text has problems, listed above"
                };
                if ui
                    .add_enabled(ok && named && target.editable, egui::Button::new(label))
                    .on_hover_text("make it a PRG; Undo takes it back")
                    .on_disabled_hover_text(tip)
                    .clicked()
                {
                    acts.push(Act::BasicAdd);
                }
                if ui
                    .add_enabled(!ed.text.is_empty(), egui::Button::new("Save Text…"))
                    .on_hover_text("save the text to a file")
                    .clicked()
                {
                    acts.push(Act::BasicSaveText);
                }
                if ui.button("Close").clicked() {
                    *open = false;
                }
            });
        });
    egui::ScrollArea::both()
        .id_salt("basic_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let edit = egui::TextEdit::multiline(&mut ed.text)
                .id_salt("basic_text")
                .font(egui::TextStyle::Monospace)
                .hint_text("10 PRINT \"{CLR}HELLO\"\n20 GOTO 10")
                .desired_width(f32::INFINITY)
                .min_size(ui.available_size())
                .lock_focus(true);
            let edit = ui.add(edit);
            ed.typing |= edit.has_focus();
            if edit.changed() {
                ed.modified = true;
                ed.confirm = None;
                ed.checked = None;
            }
        });
}

/// The line under the BASIC editor's text: what it makes, or what's wrong.
fn basic_check_ui(ui: &mut egui::Ui, ed: &mut BasicEditor) {
    if ed.text.trim().is_empty() {
        ui.label(
            egui::RichText::new("Type or paste a program, one numbered line per line.")
                .color(c64::TEXT_DIM),
        );
        return;
    }
    const SHOWN: usize = 4;
    let (problems, colour) = match ed.check() {
        Ok(t) => {
            let s = if t.lines == 1 { "" } else { "s" };
            let made = format!(
                "{} line{s}, {} bytes, loads at ${:04X}",
                t.lines,
                t.prg.len(),
                t.load_address
            );
            ui.label(egui::RichText::new(made).color(c64::OK));
            (&t.warnings, c64::ORPHAN)
        }
        Err(p) => (p, c64::WARN),
    };
    for p in problems.iter().take(SHOWN) {
        ui.add(egui::Label::new(egui::RichText::new(p.to_string()).color(colour)).wrap());
    }
    if problems.len() > SHOWN {
        let more = format!("and {} more", problems.len() - SHOWN);
        ui.label(egui::RichText::new(more).color(colour));
    }
}

/// The validation window's contents: the check's results, a checkbox per
/// fix, and the buttons.
fn validation_ui(
    ui: &mut egui::Ui,
    text: &str,
    bad: bool,
    plan: &mut RepairPlan,
    close: &mut bool,
    acts: &mut Vec<Act>,
) {
    let col = if bad { c64::WARN } else { c64::OK };
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).monospace().color(col));
    if !plan.choices.is_empty() {
        ui.separator();
        ui.label(egui::RichText::new("FIXES").color(c64::ACCENT).strong());
        egui::ScrollArea::vertical()
            .id_salt("repair_scroll")
            .max_height(260.0)
            .show(ui, |ui| {
                for c in &mut plan.choices {
                    let tip = fix_tip(&c.fix);
                    ui.checkbox(&mut c.apply, c.fix.to_string())
                        .on_hover_text(tip);
                }
            });
    }
    for note in &plan.notes {
        ui.label(egui::RichText::new(format!("Note: {note}.")).color(c64::TEXT_DIM));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if !plan.choices.is_empty() {
            let n = plan.choices.iter().filter(|c| c.apply).count();
            if ui
                .add_enabled(n > 0, egui::Button::new(format!("Repair ({n})")))
                .on_hover_text("make the ticked fixes; Undo takes them all back")
                .on_disabled_hover_text("tick the fixes to make")
                .clicked()
            {
                acts.push(Act::Repair);
            }
        }
        if ui.button("Close").clicked() {
            *close = true;
        }
    });
}

/// What a fix does, and what to think about before making it.
fn fix_tip(fix: &Fix) -> &'static str {
    match fix {
        Fix::EndDirectory { .. } => {
            "entries after the break are lost; their files' blocks stay allocated"
        }
        Fix::EndFile { .. } => "the rest of the file is already cut off and can't be found",
        Fix::RemoveFile { .. } => {
            "blocks that only this file uses are freed; untick it if the entry is there on purpose"
        }
        Fix::SplitFile { .. } => {
            "each file gets its own copy, so deleting one can't damage the other"
        }
        Fix::MarkUsed { .. } => "otherwise the next file written could overwrite them",
        Fix::FreeLost { .. } => {
            "they may be lost parts of broken files, or data a protected game hides: \
             untick it if you want to keep them"
        }
        Fix::Recount { .. } => "the number of free blocks is worked out again",
    }
}

/// A pane's files as the comparison sees them.
fn compare_items(p: &Pane) -> Vec<CompareItem<'_>> {
    p.nodes
        .iter()
        .map(|n| CompareItem {
            name: &n.name,
            kind: &n.badge,
            data: n.data.as_deref().ok(),
        })
        .collect()
}

/// The colour of a Compare row.
fn compare_colour(status: CompareStatus) -> Color32 {
    match status {
        CompareStatus::Same | CompareStatus::Unreadable => c64::TEXT_DIM,
        CompareStatus::Differs(_) => c64::WARN,
        CompareStatus::OnlyLeft | CompareStatus::OnlyRight => c64::ORPHAN,
    }
}

/// The Compare window's contents: what each pane holds, a summary, and a
/// row per file saying how it compares. Clicking a row selects that file
/// in both panes.
fn compare_ui(
    ui: &mut egui::Ui,
    panes: [&Pane; 2],
    rows: &[CompareRow],
    all: &mut bool,
    open: &mut bool,
    acts: &mut Vec<Act>,
) {
    for (side, p) in ["LEFT ", "RIGHT"].into_iter().zip(panes) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(side).color(c64::TEXT_DIM));
            ui.add(egui::Label::new(egui::RichText::new(&p.title).color(c64::BRIGHT)).truncate());
        });
    }
    ui.separator();
    let summary = sector64::compare::summarize(rows);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                *open = false;
            }
            ui.checkbox(all, "Show same")
                .on_hover_text("also list the files that match on both sides");
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let colour = if summary.identical() {
                    c64::OK
                } else {
                    c64::TEXT
                };
                ui.add(
                    egui::Label::new(egui::RichText::new(summary.to_string()).color(colour)).wrap(),
                );
            });
        });
    });
    ui.horizontal(|ui| {
        let buttons = [
            (true, "Copy Missing to Right", summary.only_left),
            (false, "Copy Missing to Left", summary.only_right),
        ];
        for (to_right, label, n) in buttons {
            let dest = panes[usize::from(to_right)];
            let label = match n {
                0 => label.to_string(),
                n => format!("{label} ({n})"),
            };
            let (from, to) = if to_right {
                ("left", "right")
            } else {
                ("right", "left")
            };
            let tip = if dest.editable() {
                format!("the {to} pane isn't missing any files")
            } else {
                format!("the {to} pane needs an editable disk or a new archive")
            };
            if ui
                .add_enabled(n > 0 && dest.editable(), egui::Button::new(label))
                .on_hover_text(format!(
                    "copy the files that are only on the {from} into the {to} pane"
                ))
                .on_disabled_hover_text(tip)
                .clicked()
            {
                acts.push(Act::CopyMissing(to_right));
            }
        }
    });
    ui.separator();
    let node = |side: usize, i: Option<usize>| i.and_then(|i| panes[side].nodes.get(i));
    let shown: Vec<&CompareRow> = rows
        .iter()
        .filter(|r| *all || r.status != CompareStatus::Same)
        .collect();
    if shown.is_empty() {
        ui.label(egui::RichText::new("No differences.").color(c64::TEXT_DIM));
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("compare_scroll")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new("compare_grid")
                .striped(true)
                .num_columns(5)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    for head in ["", "NAME", "LEFT", "RIGHT", ""] {
                        ui.label(egui::RichText::new(head).color(c64::TEXT_DIM));
                    }
                    ui.end_row();
                    for row in shown {
                        let (l, r) = (node(0, row.left), node(1, row.right));
                        let kinds = [l.map(|n| n.badge.as_str()), r.map(|n| n.badge.as_str())];
                        let colour = compare_colour(row.status);
                        let note = row.status.describe(kinds);
                        ui.label(egui::RichText::new(row.status.mark()).color(colour));
                        if ui
                            .selectable_label(false, egui::RichText::new(&row.name).color(colour))
                            .on_hover_text("select it in both panes")
                            .clicked()
                        {
                            acts.push(Act::CompareSelect(row.left, row.right));
                        }
                        for n in [l, r] {
                            let size = match n.map(|n| n.data.as_ref()) {
                                Some(Ok(d)) => format!("{} B", d.len()),
                                Some(Err(_)) => "?".into(),
                                None => "—".into(),
                            };
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(size);
                                },
                            );
                        }
                        ui.label(egui::RichText::new(note).color(colour));
                        ui.end_row();
                    }
                });
        });
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
            "open an editable disk (or start a new archive) in the main window first".to_string()
        } else {
            key_name("select files above first (Ctrl-click for several)")
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

/// Copy the files at `rows` of `src` into `dest`'s disk or new archive, as
/// one step of its Undo. Returns the status line, which calls it `dest_name`.
fn copy_rows(src: &Pane, dest: &mut Pane, rows: &[usize], dest_name: &str) -> String {
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
        dest.hex_cache = None;
        dest.gfx_cache = None;
        dest.pic_cache = None;
        dest.refresh();
    }
    copy_report(&done, &failed, dest_name)
}

/// Write an archive member (or any listed file without a directory entry)
/// onto `disk` under its listed name, refusing names already there and REL
/// files.
fn copy_bytes(disk: &mut Disk, node: &Node) -> Result<(), String> {
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
fn node_file(node: &Node) -> Result<ImportFile, String> {
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

/// What the unsaved-changes prompt says, given which disks have unsaved
/// changes (e.g. "the open disk").
fn unsaved_message(which: &[&str]) -> String {
    let list = match which {
        [] => "Nothing".to_string(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    let verb = if which.len() > 1 { "have" } else { "has" };
    let mut s = format!("{list} {verb} unsaved changes.");
    s[..1].make_ascii_uppercase();
    s
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
    fill(16, 7, 32, 19, shutter); // metal shutter, centred on the top edge
    fill(23, 9, 27, 17, body); // its window, off-centre as on a real disk
    fill(10, 24, 38, 40, label); // write-on label
    let lines = [0x9a, 0x92, 0xc0, 0xff];
    fill(12, 29, 36, 31, lines); // ruled lines on the label, as in the
    fill(12, 32, 36, 34, lines); // launcher icon (packaging/sector64.svg)
    fill(12, 36, 28, 38, lines);
    // Bevel both top corners: 4 pixels cut at 45°.
    for y in 5..9 {
        for k in 0..9 - y {
            for x in [5 + k, 42 - k] {
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
    let initial: Vec<String> = std::env::args().skip(1).take(2).collect();
    // On a Mac the Dock shows the app bundle's icon, which a window icon
    // would replace; an empty one leaves it alone.
    let icon = if cfg!(target_os = "macos") {
        egui::IconData::default()
    } else {
        make_icon()
    };
    // Finder starts apps in "/", which nobody can write to: files extracted
    // without choosing a folder go to the home folder instead.
    #[cfg(target_os = "macos")]
    mac::leave_root_dir();
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("sector64-gui")
            .with_inner_size([960.0, 640.0])
            .with_icon(std::sync::Arc::new(icon)),
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
            #[cfg(target_os = "macos")]
            mac::guard_quit(&cc.egui_ctx);
            load_c64_font(&cc.egui_ctx);
            apply_c64_theme(&cc.egui_ctx);
            Ok(Box::new(App::new(&initial, cc.storage)))
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
    fn pic_failures_say_what_was_expected() {
        let why = pic_failure(&[0x01, 0x08, 0x00], None);
        assert!(
            why.contains("Koala Painter pictures load at $6000"),
            "{why}"
        );
        assert!(why.contains("this file loads at $0801"), "{why}");
        assert_eq!(
            pic_failure(&[0x00, 0x60], Some(PicFormat::Doodle)),
            "(too short for a Doodle! picture)"
        );
        assert_eq!(c64_color(6), c64::BG);
    }

    #[test]
    fn dis_preview_starts_at_the_sys_target() {
        // 10 SYS2061, then INC $D020 / RTS at $080D.
        let prg = [
            0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x9E, 0x32, 0x30, 0x36, 0x31, 0x00, 0x00, 0x00,
            0xEE, 0x20, 0xD0, 0x60,
        ];
        assert_eq!(
            dis_preview(&prg, true),
            "; SYS 2061 → $080D (loads at $0801)\n\
             080D  EE 20 D0  INC $D020\n\
             0810  60        RTS\n"
        );
        // Without a load address, everything from $0000.
        assert_eq!(
            dis_preview(&[0xA7, 0x10], false),
            "; no load address: listed from $0000\n0000  A7 10    *LAX $10\n"
        );
        assert!(
            dis_preview(&[0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x99, 0, 0, 0], true)
                .starts_with("; a BASIC program with no SYS")
        );
        let big = vec![0xEA; DIS_MAX_LINES + 5];
        assert!(dis_preview(&big, false).ends_with("(5 more bytes)\n"));
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
            pane: Pane {
                disk: Some(EditDisk {
                    disk: Disk::format("UNDO TEST", "UT"),
                    path: None,
                    dirty: false,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        app.pane.refresh();
        app
    }

    fn names(app: &App) -> Vec<String> {
        app.pane.nodes.iter().map(|n| n.name.clone()).collect()
    }

    #[test]
    fn tab_focus_swaps_the_panes_only_in_dual_view() {
        let mut app = app_with_disk();
        app.apply(Act::AddBytes("ONE".into(), vec![1, 8, 0]));
        app.apply(Act::FocusOther);
        assert_eq!(names(&app), ["ONE"], "single view: nothing to focus");
        app.dual = true;
        app.apply(Act::FocusOther);
        assert!(app.focus_right && app.pane.nodes.is_empty());
        assert_eq!(app.back.nodes[0].name, "ONE");
        // Each pane keeps its own undo history.
        app.apply(Act::Undo);
        assert_eq!(app.status, "Nothing to undo");
        app.apply(Act::FocusOther);
        assert!(!app.focus_right);
        app.apply(Act::Undo);
        assert!(names(&app).is_empty());
    }

    #[test]
    fn f5_copies_into_the_other_panes_disk_or_archive() {
        let mut app = app_with_disk();
        app.dual = true;
        app.apply(Act::AddBytes("ONE".into(), vec![1, 8, 0]));
        app.apply(Act::AddBytes("TWO".into(), vec![1, 8, 1]));
        app.pane.marked.extend([0, 1]);
        // Nothing open in the other pane yet.
        app.apply(Act::CopyToPane);
        assert!(app.status.contains("in the other pane to copy into"));

        // Into a blank disk: one undo step there, nothing changed here.
        app.apply(Act::FocusOther);
        app.apply(Act::Create);
        app.apply(Act::FocusOther);
        app.pane.marked.extend([0, 1]);
        app.apply(Act::CopyToPane);
        assert_eq!(app.status, "Copied 2 files to the other pane");
        let back: Vec<_> = app.back.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(back, ["ONE", "TWO"]);
        assert!(app.back.is_dirty());
        assert_eq!(app.back.undo.len(), 1);
        assert_eq!(names(&app), ["ONE", "TWO"]);
        // Copying again clashes on the names.
        app.pane.marked.extend([0]);
        app.apply(Act::CopyToPane);
        assert!(app.status.starts_with("Nothing copied; skipped \"ONE\""));

        // Into a new archive, with the bytes and type.
        app.apply(Act::FocusOther);
        app.apply(Act::NewArchive);
        app.apply(Act::FocusOther);
        app.apply(Act::Select(1));
        app.apply(Act::CopyToPane);
        let stage = app.back.archive.as_ref().unwrap();
        assert_eq!(
            (stage[0].name.as_str(), &stage[0].data[..]),
            ("TWO", &[1, 8, 1][..])
        );
    }

    #[test]
    fn compare_rows_select_the_file_in_both_panes() {
        let mut app = app_with_disk();
        app.dual = true;
        app.apply(Act::AddBytes("A".into(), vec![1, 8, 0]));
        app.apply(Act::AddBytes("B".into(), vec![1, 8, 1]));
        app.apply(Act::FocusOther);
        app.apply(Act::Create);
        app.apply(Act::AddBytes("B".into(), vec![1, 8, 2]));
        // The focused pane is now the right one.
        app.pane.marked.insert(0);
        app.apply(Act::CompareSelect(Some(1), Some(0)));
        assert_eq!((app.back.selected, app.pane.selected), (Some(1), Some(0)));
        assert!(app.pane.marked.is_empty());
        // A file on one side only leaves the other pane's selection alone.
        app.apply(Act::CompareSelect(Some(0), None));
        assert_eq!((app.back.selected, app.pane.selected), (Some(0), Some(0)));

        let rows = sector64::compare::compare(&compare_items(&app.back), &compare_items(&app.pane));
        let s = sector64::compare::summarize(&rows);
        assert_eq!(s.to_string(), "1 differ, 1 only on the left");
        let note = rows[1].status.describe([Some("prg"), Some("prg")]);
        assert_eq!(
            (rows[1].status.mark(), note.as_str()),
            ("*", "differs from byte 2")
        );
    }

    #[test]
    fn compare_copies_the_missing_files_either_way() {
        let names = |p: &Pane| p.nodes.iter().map(|n| n.name.clone()).collect::<Vec<_>>();
        let mut app = app_with_disk();
        app.dual = true;
        for name in ["A", "BOTH", "C"] {
            app.apply(Act::AddBytes(name.into(), vec![1, 8, 0]));
        }
        app.apply(Act::FocusOther);
        app.apply(Act::Create);
        app.apply(Act::AddBytes("BOTH".into(), vec![1, 8, 9]));
        app.apply(Act::AddBytes("D".into(), vec![1, 8, 0]));

        // The right pane has the focus, so this copies into it.
        let undo = app.pane.undo.len();
        app.apply(Act::CopyMissing(true));
        assert_eq!(app.status, "Copied 2 files to the right pane");
        assert_eq!(names(&app.pane), ["BOTH", "D", "A", "C"]);
        assert_eq!(names(&app.back), ["A", "BOTH", "C"]);
        assert_eq!(app.pane.nodes[0].data.as_deref(), Ok(&[1, 8, 9][..]));
        // One Undo takes all of them back.
        assert_eq!(app.pane.undo.len(), undo + 1);
        app.apply(Act::Undo);
        assert_eq!(names(&app.pane), ["BOTH", "D"]);

        // Into the unfocused left pane, then the same with the focus there.
        app.apply(Act::CopyMissing(false));
        assert_eq!(app.status, "Copied \"D\" to the left pane");
        assert_eq!(names(&app.back), ["A", "BOTH", "C", "D"]);
        app.apply(Act::FocusOther);
        app.apply(Act::CopyMissing(true));
        assert_eq!(names(&app.back), ["BOTH", "D", "A", "C"]);
        let rows = sector64::compare::compare(&compare_items(&app.pane), &compare_items(&app.back));
        let s = sector64::compare::summarize(&rows);
        assert_eq!((s.same, s.differ, s.only_left + s.only_right), (3, 1, 0));

        // A read-only pane is left alone.
        app.back.disk = None;
        app.back.nodes.clear();
        app.apply(Act::CopyMissing(true));
        assert!(app.status.starts_with("Open a disk"), "{}", app.status);
    }

    #[test]
    fn copying_a_long_archive_name_twice_is_refused() {
        let node = Node {
            name: "A NAME LONGER THAN SIXTEEN".into(),
            badge: "prg".into(),
            detail: String::new(),
            blocks: 0,
            data: Ok(vec![1, 8, 0]),
        };
        let mut disk = Disk::format("D", "D1");
        copy_bytes(&mut disk, &node).unwrap();
        assert!(copy_bytes(&mut disk, &node).is_err());
        assert_eq!(disk.directory().unwrap().len(), 1);
    }

    #[test]
    fn validating_offers_repairs_as_one_undo_step() {
        let mut app = app_with_disk();
        app.add("ONE".into(), &[1; 600]);
        app.apply(Act::Validate);
        assert!(app.show_validation && !app.validation_bad);
        assert!(app.repair.choices.is_empty());

        // Break ONE's chain after its first block.
        let disk = &mut app.pane.disk.as_mut().unwrap().disk;
        let e = disk.find_entry("ONE").unwrap().unwrap();
        disk.block_mut(e.first_track, e.first_sector).unwrap()[0] = 99;
        let broken = disk.as_bytes().to_vec();
        // The window's list is out of date, so it's only checked again.
        app.apply(Act::Repair);
        assert_eq!(app.pane.disk.as_ref().unwrap().disk.as_bytes(), broken);
        assert!(app.validation_bad, "{}", app.validation);
        let undo = app.pane.undo.len();
        app.apply(Act::Repair);
        assert_eq!(app.status, "Repaired the disk (1 fix)");
        assert_eq!(app.pane.nodes[0].data.as_ref().unwrap().len(), 254);
        assert!(app.pane.is_dirty());
        // What's left is the cut-off blocks, which aren't damage.
        assert!(!app.validation_bad, "{}", app.validation);
        assert_eq!(app.repair.choices.len(), 1);
        assert_eq!(app.pane.undo.len(), undo + 1);
        app.apply(Act::Undo);
        assert_eq!(app.pane.disk.as_ref().unwrap().disk.as_bytes(), broken);
    }

    #[test]
    fn basic_editor_adds_lists_and_replaces() {
        let mut app = app_with_disk();
        app.basic.text = "10 PRINT \"HI\"\n20 GOTO 10\n".into();
        app.apply(Act::BasicAdd);
        assert_eq!(app.status, "Enter a name for the program");
        app.basic.name = "hello".into();
        app.basic.modified = true;
        app.apply(Act::BasicAdd);
        assert_eq!(app.status, "Added \"HELLO\" from the BASIC editor");
        assert_eq!(app.pane.selected, Some(0));
        assert!(!app.basic.modified && app.pane.is_dirty());

        // Listed back, changed and put back: one file still, one Undo each.
        app.basic = BasicEditor::default();
        app.apply(Act::BasicFromSelected);
        assert_eq!(app.basic.text, "10 PRINT \"HI\"\n20 GOTO 10\n");
        assert_eq!((app.basic.name.as_str(), app.basic.load), ("HELLO", None));
        app.basic.text.push_str("30 END\n");
        app.basic.checked = None;
        app.apply(Act::BasicAdd);
        assert_eq!(app.status, "Replaced \"HELLO\" from the BASIC editor");
        assert_eq!(names(&app), ["HELLO"]);
        let data = app.pane.nodes[0].data.as_ref().unwrap();
        assert_eq!(sector64::detokenize_basic(data).unwrap().lines, 3);
        app.apply(Act::Undo);
        let data = app.pane.nodes[0].data.as_ref().unwrap();
        assert_eq!(sector64::detokenize_basic(data).unwrap().lines, 2);

        // Nothing but a plain BASIC program is replaced.
        app.apply(Act::AddBytes("CODE".into(), vec![0, 0xC0, 0x60]));
        app.basic.name = "CODE".into();
        app.apply(Act::BasicAdd);
        assert!(
            app.status.contains("isn't a BASIC program"),
            "{}",
            app.status
        );
        let mut stub = app.basic.check().as_ref().unwrap().prg.clone();
        stub.push(0x60);
        app.apply(Act::AddBytes("STUB".into(), stub));
        app.basic.name = "STUB".into();
        app.apply(Act::BasicAdd);
        assert!(app.status.contains("machine code"), "{}", app.status);

        // Bad text is refused, saying where.
        app.basic.text = "10 {NOPE}".into();
        app.basic.checked = None;
        app.apply(Act::BasicAdd);
        assert!(app.status.starts_with("Error in text line 1"), "{}", app.status);
        assert_eq!(names(&app), ["HELLO", "CODE", "STUB"]);

        // A new archive works too.
        app.apply(Act::NewArchive);
        app.basic.set_text("10 END".into(), Some(0x1001));
        app.basic.name = "vic".into();
        app.apply(Act::BasicAdd);
        app.apply(Act::BasicAdd);
        assert_eq!(names(&app), ["VIC"]);
        let stage = app.pane.archive.as_ref().unwrap();
        assert_eq!(stage[0].data[..2], [0x01, 0x10]);
    }

    #[test]
    fn unsaved_prompt_names_each_disk() {
        assert_eq!(
            unsaved_message(&["the open disk"]),
            "The open disk has unsaved changes."
        );
        assert_eq!(
            unsaved_message(&[
                "this pane's disk",
                "the other pane's disk",
                "the Copy window's disk"
            ]),
            "This pane's disk, the other pane's disk and the Copy window's disk have unsaved \
             changes."
        );
    }

    #[test]
    fn vice_load_names_are_typeable_with_wildcards_for_the_rest() {
        let raw = |b: &[u8]| {
            let mut r = [0xA0u8; 16];
            r[..b.len()].copy_from_slice(b);
            r
        };
        assert_eq!(vice_load_name(&raw(b"GAME V1.2")), "game v1.2");
        assert_eq!(vice_load_name(&raw(b"A\"B,C:D")), "a?b?c?d");
        // Shifted letters, graphics and control codes can't be typed.
        assert_eq!(vice_load_name(&raw(&[b'X', 0xC1, 0x5C, 0x0D])), "x???");
        assert_eq!(vice_load_name(&raw(b"")), "*");
        assert_eq!(vice_load_name(&[b'Z'; 16]), "z".repeat(16));
    }

    #[test]
    fn c128_programs_and_safe_file_names() {
        assert!(is_c128_load(&[0x01, 0x1C, 0]));
        assert!(is_c128_load(&[0x01, 0x40, 0]));
        assert!(!is_c128_load(&[0x01, 0x08, 0]));
        assert!(!is_c128_load(&[0x01]));
        assert_eq!(safe_file_name("my game/v2*"), "my game_v2_");
        assert_eq!(safe_file_name(".."), "program");
    }

    #[test]
    fn vice_runs_the_selected_program_or_else_the_first() {
        let mut app = app_with_disk();
        assert_eq!(
            app.vice_plan().err().as_deref(),
            Some("there's no program on this disk to run")
        );
        app.add_type = 1; // SEQ
        app.add("NOTES".into(), b"hello");
        app.add_type = 0; // PRG
        app.add("FIRST".into(), b"\x01\x08\x00\x00");
        app.add("SECOND".into(), b"\x01\x1c\x00\x00");
        let i = |app: &App, n: &str| app.pane.nodes.iter().position(|x| x.name == n);
        app.pane.selected = i(&app, "NOTES");
        let p = app.vice_plan().unwrap();
        assert_eq!(p.program.as_deref(), Some("first"));
        assert_eq!((p.file_name.as_str(), p.c128), ("disk.d64", false));
        app.pane.selected = i(&app, "SECOND");
        let p = app.vice_plan().unwrap();
        assert_eq!((p.program.as_deref(), p.c128), (Some("second"), true));

        // From an archive listing, the selected program runs on its own.
        let mut list = App {
            pane: Pane {
                nodes: vec![Node {
                    name: "DEMO/1".into(),
                    badge: "prg".into(),
                    detail: String::new(),
                    blocks: 0,
                    data: Ok(vec![0x01, 0x08, 0x00]),
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(list.vice_plan().is_err());
        list.pane.selected = Some(0);
        let p = list.vice_plan().unwrap();
        assert_eq!((p.file_name.as_str(), p.program), ("DEMO_1.prg", None));
    }

    #[test]
    fn vice_is_found_in_the_chosen_folder_or_its_bin() {
        let d = std::env::temp_dir().join(format!("s64-vice-find-{}", std::process::id()));
        let bin = d.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(format!("x128{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(find_vice(&["x128"], &d.display().to_string()), Some(exe));
        assert_eq!(
            find_vice(&["no-such-emulator"], &d.display().to_string()),
            None
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn undo_and_redo_step_through_disk_edits() {
        let mut app = app_with_disk();
        app.add("ONE".into(), b"\x01\x08");
        app.add("TWO".into(), b"\x01\x08");
        app.pane.selected = Some(0);
        app.pane.rename_to = "FIRST".into();
        app.rename();
        app.pane.selected = Some(1);
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
        assert!(!app.pane.is_dirty(), "back to the state that was loaded");
        app.undo();
        assert_eq!(app.status, "Nothing to undo");

        app.redo();
        app.redo();
        assert_eq!(names(&app), ["ONE", "TWO"]);
        assert!(app.pane.is_dirty());
        // A new edit drops the redo history.
        app.add("NEW".into(), b"\x01\x08");
        app.redo();
        assert_eq!(app.status, "Nothing to redo");
        assert_eq!(names(&app), ["ONE", "TWO", "NEW"]);
    }

    #[test]
    fn sector_editor_writes_one_undoable_block() {
        let mut app = app_with_disk();
        app.add("PROG".into(), &[0x11; 300]);
        let first = app.pane.map_disk().unwrap().block_map().chains[0][0];
        // Opened from the disk map at the file's first block.
        app.apply(Act::SectorOpen(first.0, first.1));
        assert!(app.show_sector);
        assert_eq!((app.sector.track, app.sector.sector), first);
        app.sector_sync();
        app.sector.place(2);
        app.sector.type_digit(0xA);
        app.sector.type_digit(0x9);
        // Not on the disk until Write.
        let disk = |app: &App| app.pane.disk.as_ref().unwrap().disk.clone();
        assert_eq!(disk(&app).block(first.0, first.1).unwrap()[2], 0x11);
        // Jumping elsewhere is refused while edits are staged.
        app.apply(Act::SectorOpen(18, 0));
        assert_eq!((app.sector.track, app.sector.sector), first);
        app.apply(Act::SectorWrite);
        assert_eq!(
            app.status,
            format!("Wrote track {}, sector {}", first.0, first.1)
        );
        assert_eq!(disk(&app).block(first.0, first.1).unwrap()[2], 0xA9);
        assert!(app.pane.is_dirty());
        let data = app.pane.nodes[0].data.as_ref().unwrap();
        assert_eq!(data[0], 0xA9, "the listing shows the new bytes");

        app.undo();
        assert_eq!(disk(&app).block(first.0, first.1).unwrap()[2], 0x11);
        app.sector_sync();
        assert_eq!(app.sector.buf[2], 0x11, "the editor follows the undo");
    }

    #[test]
    fn sector_editor_drops_edits_for_another_disk_and_is_view_only_on_read_only_ones() {
        let mut app = app_with_disk();
        app.show_sector = true;
        app.sector_sync();
        // A fresh editor opens on the header block.
        assert_eq!((app.sector.track, app.sector.sector), (18, 0));
        app.sector.type_digit(0);
        assert!(app.sector.modified());

        // A Zip-Code set opens read-only: the staged edits go, Write refuses.
        let dir = scratch("sector");
        for (i, part) in sector64::write_zipcode(&sample_d64())
            .unwrap()
            .iter()
            .enumerate()
        {
            std::fs::write(dir.join(format!("{}!GAME", i + 1)), part).unwrap();
        }
        app.open(&dir.join("1!GAME"));
        app.sector_sync();
        assert!(!app.sector.modified());
        assert_eq!((app.sector.track, app.sector.sector), (18, 0));
        app.sector.type_digit(1);
        app.apply(Act::SectorWrite);
        assert_eq!(
            app.status,
            "This disk is read-only, so its blocks can't be changed"
        );
    }

    #[test]
    fn sector_grid_columns_map_back_to_bytes() {
        for i in 0..16 {
            assert_eq!(sec_byte_at(sec_hex_col(i)), Some(i));
            assert_eq!(sec_byte_at(sec_hex_col(i) + 1), Some(i));
            assert_eq!(sec_byte_at(SEC_TEXT0 + i), Some(i));
        }
        assert_eq!(sec_byte_at(0), None);
        assert_eq!(sec_byte_at(sec_hex_col(7) + 2), None);
        assert_eq!(sec_hex_col(8), sec_hex_col(7) + 4, "gap after the 8th byte");
        assert_eq!(sec_byte_at(SEC_COLS), None);
    }

    #[test]
    fn find_searches_names_and_contents_and_shows_matches() {
        let mut app = app_with_disk();
        app.add("HELLO".into(), b"\x01\x08hello world");
        let mut big = vec![0u8; 600];
        big[300..303].copy_from_slice(&[0xA9, 0x00, 0x8D]);
        big[500..503].copy_from_slice(&[0xA9, 0x07, 0x8D]);
        app.add("CODE".into(), &big);
        app.add("OTHER".into(), b"nothing");

        app.find.query = "HELLO".into();
        app.apply(Act::Find);
        assert_eq!(app.find.hits.len(), 1);
        let h = &app.find.hits[0];
        assert!(h.in_name && h.total == 1 && h.offsets == [2]);
        assert_eq!(app.find.summary, "1 match in 1 file of 3 · 1 file by name");

        app.find.hex = true;
        app.find.query = "A9 ?? 8D".into();
        app.apply(Act::Find);
        assert_eq!(app.find.hits.len(), 1);
        assert_eq!(app.find.hits[0].offsets, [300, 500]);
        assert!(app.find.hits[0].on_disk);

        // Shown in the hex preview...
        app.apply(Act::FindShow(1, 500));
        assert_eq!(app.pane.selected, Some(1));
        assert!(app.preview_mode == PreviewMode::Hex && app.show_preview);
        assert_eq!(app.find.mark, Some((1, 500, 3)));
        assert!(app.find.scroll);

        // ...or in the Sector Editor: offset 500 is block 2, byte 2 + 246.
        app.find.in_sector = true;
        app.apply(Act::FindShow(1, 500));
        let chain = app.pane.map_disk().unwrap().block_map().chains[1].clone();
        assert!(app.show_sector);
        assert_eq!((app.sector.track, app.sector.sector), chain[1]);
        app.sector_sync();
        assert_eq!(app.sector.cursor, 248);
        assert_eq!(&app.sector.buf[248..251], &[0xA9, 0x07, 0x8D]);

        // Bad patterns explain themselves; a changed list is noticed.
        app.find.query = "A9 0".into();
        app.apply(Act::Find);
        assert!(app.find.summary.starts_with('⚠') && app.find.hits.is_empty());
        app.find.query = "8D".into();
        app.apply(Act::Find);
        app.pane.selected = Some(0);
        app.delete();
        app.find.in_sector = false;
        app.apply(Act::FindShow(1, 302));
        assert_eq!(
            app.status,
            "The listing has changed since the search; search again"
        );
    }

    #[test]
    fn hex_preview_highlights_the_match_in_both_columns() {
        let data: Vec<u8> = (0u8..40).collect();
        let dump = hexdump(&data, 64);
        let job = hex_marked_layout(&dump, 7, 3, egui::FontId::monospace(10.0), Color32::WHITE);
        let marked: String = job
            .sections
            .iter()
            .filter(|s| s.format.background == c64::ACCENT_DIM)
            .map(|s| &job.text[s.byte_range.clone()])
            .collect::<Vec<_>>()
            .join("|");
        // Bytes 7, 8 and 9 (across the gap after the eighth), then their text.
        assert_eq!(marked, "07|08|09|...");
        assert_eq!(job.text, dump);
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
        app.pane.marked.extend([0, 2]);
        app.apply(Act::CopyToOther);
        assert!(app.other.is_none());

        app.apply(Act::OtherNew(2));
        assert!(app.other.as_ref().unwrap().dirty);
        app.apply(Act::CopyToOther);
        assert_eq!(other_names(&app), ["ONE", "THREE"]);
        assert_eq!(app.status, "Copied 2 files to the other disk");
        // Copying one again is refused, and nothing changes.
        app.pane.marked.clear();
        app.pane.selected = Some(0);
        app.apply(Act::CopyToOther);
        assert_eq!(
            app.status,
            "Nothing copied; skipped \"ONE\": a file with that name already exists"
        );
        assert_eq!(app.other.as_ref().unwrap().undo.len(), 1);

        // Back the other way: delete ONE here, copy it back from the other
        // disk; the main Undo takes it away again.
        app.pane.selected = Some(0);
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
        assert_eq!(app.pane.nodes[0].data.as_deref().unwrap(), b"\x01\x08\x60");
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
            pane: Pane {
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
            },
            ..Default::default()
        };
        app.apply(Act::OtherNew(0));
        app.pane.marked.extend([0, 1]);
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
        assert_eq!(
            app.pane.archive.as_ref().unwrap()[0].file_type,
            FileType::Seq
        );
    }

    #[test]
    fn failed_edits_are_not_recorded_and_history_is_capped() {
        let mut app = app_with_disk();
        app.pane.selected = None;
        app.delete(); // nothing selected: no change
        assert!(app.pane.undo.is_empty());
        for i in 0..UNDO_MAX + 5 {
            app.add(format!("F{i}"), b"\x01\x08");
        }
        assert_eq!(app.pane.undo.len(), UNDO_MAX);
    }

    #[test]
    fn undo_after_save_marks_the_disk_unsaved() {
        let dir = std::env::temp_dir().join(format!("s64-undo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("u.d64");
        let mut app = app_with_disk();
        app.pane.disk.as_mut().unwrap().path = Some(path.clone());
        app.add("ONE".into(), b"\x01\x08");
        app.save(false);
        assert!(!app.pane.is_dirty());
        app.undo();
        assert!(
            app.pane.is_dirty(),
            "the disk no longer matches the saved file"
        );
        app.redo();
        assert!(!app.pane.is_dirty(), "back to exactly what was saved");
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
        assert!(app.pane.undo.is_empty() && app.pane.redo.is_empty());
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
    /// Run `frames` frames of the whole window at `width` × 760 and return
    /// the x positions (sampled every pixel) along a line through the
    /// middle that no opaque fill covers: places where whatever is behind
    /// the window would show through.
    fn unpainted_columns(app: &mut App, ctx: &egui::Context, width: f32) -> Vec<f32> {
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 760.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run(input(), |ctx| app.frame_ui(ctx));
        for _ in 0..3 {
            out = ctx.run(input(), |ctx| app.frame_ui(ctx));
        }
        fn fills(shape: &egui::Shape, clip: egui::Rect, acc: &mut Vec<egui::Rect>) {
            match shape {
                egui::Shape::Rect(r) if r.fill.a() == 255 => {
                    acc.push(r.rect.intersect(clip));
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| fills(s, clip, acc)),
                _ => {}
            }
        }
        let mut rects = Vec::new();
        for cs in &out.shapes {
            fills(&cs.shape, cs.clip_rect, &mut rects);
        }
        let y = 400.0;
        (0..width as usize)
            .map(|x| x as f32 + 0.5)
            .filter(|&x| !rects.iter().any(|r| r.contains(egui::pos2(x, y))))
            .collect()
    }

    #[test]
    fn preview_pane_never_leaves_part_of_the_window_unpainted() {
        let ctx = egui::Context::default();
        load_c64_font(&ctx);
        apply_c64_theme(&ctx);
        for width in [520.0, 640.0, 760.0, 900.0, 1100.0, 1400.0] {
            for (mode, ..) in PREVIEW_MODES {
                let mut app = app_with_disk();
                app.add("A LONG FILE NAME".into(), &[0x41; 9000]);
                app.add("B".into(), &[1, 2, 3]);
                app.pane.selected = Some(0);
                app.show_preview = true;
                app.preview_mode = mode;
                let gaps = unpainted_columns(&mut app, &ctx, width);
                let label = PREVIEW_MODES.iter().find(|m| m.0 == mode).unwrap().1;
                assert!(
                    gaps.is_empty(),
                    "{width} px wide, {label}: unpainted from x={} to {} ({} px)",
                    gaps[0],
                    gaps[gaps.len() - 1],
                    gaps.len()
                );
                // Nothing in the pane runs off the window's edge either.
                let panel = egui::containers::panel::PanelState::load(&ctx, "preview".into())
                    .expect("the preview pane was drawn");
                assert!(
                    panel.rect.max.x <= width + 0.5,
                    "{width} px wide, {label}: the pane reaches x={}",
                    panel.rect.max.x
                );
            }
        }
    }

    fn sample_d64() -> Vec<u8> {
        let mut disk = Disk::format("ZIP TEST", "ZT");
        let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        disk.write_file("HELLO", FileType::Prg, &data).unwrap();
        disk.as_bytes().to_vec()
    }

    #[test]
    fn disk_map_is_rebuilt_after_edits_and_covers_read_only_disks() {
        let mut app = app_with_disk();
        app.pane.map_cache = app.pane.map_disk().map(Disk::block_map);
        assert_eq!(app.pane.map_cache.as_ref().unwrap().files.len(), 0);
        app.add("NEW".into(), &[1u8; 600]);
        assert!(app.pane.map_cache.is_none(), "an edit drops the stale map");
        let map = app.pane.map_disk().unwrap().block_map();
        assert_eq!(map.blocks_of(0).len(), 3);
        app.pane.map_cache = Some(map);
        app.undo();
        assert!(app.pane.map_cache.is_none(), "undo drops it too");

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
        assert!(app.pane.disk.is_none() && app.pane.read_only_disk);
        let map = app
            .pane
            .map_disk()
            .expect("read-only disk is mapped")
            .block_map();
        assert_eq!(map.files[0].name, "HELLO");
        // Opening a plain archive leaves nothing to map.
        app.pane.archive = Some(Vec::new());
        app.open(&dir.join("missing.d64"));
        assert!(app.pane.map_disk().is_none());
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
