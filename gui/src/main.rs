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

mod actions;
mod basic_ui;
mod dialogs;
mod files;
mod frame;
mod listing;
#[cfg(target_os = "macos")]
mod mac;
mod preview;
mod sector;
#[cfg(test)]
mod tests;
mod tools;
mod vice;
mod widgets;

use basic_ui::*;
use dialogs::*;
use files::*;
use listing::*;
use preview::*;
use tools::*;
use vice::*;
use widgets::*;

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
    /// Start the new disk set up in the New Disk dialog.
    NewDisk,
    /// Start a new, empty archive.
    NewArchive,
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
    /// What the new archive held when it was last saved, to tell whether it
    /// has unsaved changes (undo can step back to exactly what was saved).
    archive_saved: Option<Vec<ImportFile>>,
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
    /// The BASIC editor: whether its window is open, and its program.
    show_basic: bool,
    /// The New Disk dialog is open.
    show_new_disk: bool,
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
    // which page the Help window shows
    help_page: HelpPage,
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
    /// Add these host files, each typed by its extension or P00 header.
    AddPaths(Vec<PathBuf>),
    /// A file was dropped on the window with nothing editable open: open it.
    /// The count is how many other dropped files were ignored.
    OpenDropped(PathBuf, usize),
    /// Show a status message.
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
    /// Mark every file the listing shows (the filter decides which).
    SelectAll,
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

    /// Whether the open disk has unsaved edits, or the new archive has files
    /// that aren't saved as they are now.
    fn is_dirty(&self) -> bool {
        self.disk.as_ref().is_some_and(|e| e.dirty) || self.archive_unsaved()
    }

    /// The new archive holds files, and they differ from what was saved.
    fn archive_unsaved(&self) -> bool {
        self.archive
            .as_ref()
            .is_some_and(|a| !a.is_empty() && self.archive_saved.as_ref() != Some(a))
    }

    /// How the unsaved-changes prompt names what this pane would lose:
    /// the focused pane (`this`) or the other one, in single or dual view.
    fn unsaved_name(&self, this: bool, dual: bool) -> &'static str {
        match (this, dual, self.archive.is_some()) {
            (true, false, false) => "the open disk",
            (true, false, true) => "the new archive",
            (true, true, false) => "this pane's disk",
            (true, true, true) => "this pane's new archive",
            (false, _, false) => "the other pane's disk",
            (false, _, true) => "the other pane's new archive",
        }
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
            let saved = self.archive_saved.as_ref() == Some(stage);
            let state = if saved { "saved" } else { "unsaved" };
            self.title = format!("New archive — {} file(s) ({state})", stage.len());
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
        // The list was rebuilt: everything worked out from the old one goes.
        self.forget_derived();
    }

    /// Forget what was worked out from the listing as it was: the preview
    /// pane's views, the disk map, the marks and any rename in progress
    /// (indices may have shifted, and the disk may have changed).
    fn forget_derived(&mut self) {
        self.drop_previews();
        self.map_cache = None;
        self.renaming = None;
        self.marked.clear();
    }

    /// Drop the preview pane's views; they're made again for the file
    /// selected next.
    fn drop_previews(&mut self) {
        self.hex_cache = None;
        self.gfx_cache = None;
        self.pic_cache = None;
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

/// Whether `SECTOR64_X11` asks for the X11 back-end: set to anything except
/// empty or "0".
#[cfg(any(target_os = "linux", test))]
fn force_x11(v: Option<&std::ffi::OsStr>) -> bool {
    v.is_some_and(|v| !v.is_empty() && v != "0")
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
