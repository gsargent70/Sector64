//! `App::apply` and the operations it runs (open, save, add, rename, undo, extract…).

use crate::*;

impl App {
    /// `initial` is up to two files from the command line; two open side by
    /// side in dual-pane view.
    pub(crate) fn new(initial: &[String], storage: Option<&dyn eframe::Storage>) -> Self {
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

    pub(crate) fn open(&mut self, path: &Path) {
        self.pane.clear_history();
        self.sector.source.clear();
        self.pane.nodes.clear();
        self.pane.selected = None;
        self.pane.filter.clear();
        self.pane.disk = None;
        self.pane.archive = None;
        self.pane.read_only_disk = false;
        self.pane.ro_disk = None;
        self.pane.title.clear();
        self.pane.forget_derived();
        self.validation.clear();
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

    pub(crate) fn after_open(&mut self, path: &Path) {
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

    pub(crate) fn apply(&mut self, act: Act) {
        match act {
            Act::OpenDialog => {
                let dlg = open_dialog(
                    "Commodore disks & archives",
                    &[
                        "d64", "d71", "d81", "g64", "d2m", "arc", "sda", "lha", "lzh", "sfx",
                        "lbr", "t64", "lnx", "wr3", "p00", "sid",
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
            Act::SelectAll => {
                let order = self.pane.sorted_order(&self.pane.filter);
                self.pane.marked = order.iter().copied().collect();
                if !self
                    .pane
                    .selected
                    .is_some_and(|s| self.pane.marked.contains(&s))
                {
                    self.pane.selected = order.first().copied();
                }
                self.status = format!("Selected {} file(s)", order.len());
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
            // (Only drops on a pane with nothing editable open get here, but
            // check anyway.)
            Act::OpenDropped(p, _) if self.pane.is_dirty() => {
                self.confirm = Some(Confirm::OpenFile(p))
            }
            Act::OpenDropped(p, ignored) => {
                self.open(&p);
                if ignored > 0 && !self.status.starts_with("Error") {
                    self.status = format!(
                        "{} (opened the first dropped file; {ignored} other(s) ignored)",
                        self.status
                    );
                }
            }
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
            Act::Create if self.pane.is_dirty() => self.confirm = Some(Confirm::NewDisk),
            Act::Create => self.create_disk(),
            Act::AddFile => {
                if let Some(paths) = rfd::FileDialog::new().pick_files() {
                    self.add_paths(&paths);
                }
            }
            Act::AddPaths(paths) => self.add_paths(&paths),
            Act::MoveEntry(down) => self.move_entry(down),
            Act::ToggleLock => self.toggle_lock(),
            Act::AddSeparator => self.add_separator(),
            Act::OpenDiskName => {
                if let Some(h) = self.pane.disk.as_ref().and_then(|e| e.disk.header().ok()) {
                    self.disk_name = h.name;
                    self.disk_id = h.id;
                    self.show_disk_name = true;
                }
            }
            Act::SetHeader => self.set_header(),
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
            Act::NewArchive if self.pane.is_dirty() => self.confirm = Some(Confirm::NewArchive),
            Act::NewArchive => self.new_archive(),
            Act::SaveArchiveAs => self.save_archive(),
            Act::ConvertTo(to) => self.convert_to(to),
            Act::RunVice => self.run_vice(),
        }
    }

    /// Replace this pane's contents with a blank disk, set up in the New
    /// Disk dialog (`new_name`, `new_id`, `new_fmt`).
    pub(crate) fn create_disk(&mut self) {
        self.sector.source.clear();
        let (disk, kind) = match self.new_fmt {
            1 => (Disk::format_d71(&self.new_name, &self.new_id), "D71"),
            2 => (Disk::format_d81(&self.new_name, &self.new_id), "D81"),
            _ => (Disk::format(&self.new_name, &self.new_id), "D64"),
        };
        self.pane.clear_history();
        self.pane.read_only_disk = false;
        self.pane.ro_disk = None;
        self.pane.archive = None;
        self.pane.disk = Some(EditDisk {
            disk,
            path: None,
            dirty: true,
        });
        self.pane.selected = None;
        self.pane.refresh();
        self.pane.opened_path = None;
        self.status = format!("Created a blank {kind} image (unsaved); Save As… to keep it");
    }

    /// Replace this pane's contents with a new, empty archive.
    pub(crate) fn new_archive(&mut self) {
        self.pane.clear_history();
        self.pane.disk = None;
        self.pane.read_only_disk = false;
        self.pane.ro_disk = None;
        self.pane.archive = Some(Vec::new());
        self.pane.archive_saved = None;
        self.pane.selected = None;
        self.pane.refresh();
        self.pane.opened_path = None;
        self.status = "New archive — add files, then Save As…".into();
    }

    /// What Run in VICE would start, or why there's nothing to run.
    pub(crate) fn vice_plan(&self) -> Result<VicePlan, String> {
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
    pub(crate) fn run_vice(&mut self) {
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

    /// The main listing's chosen rows: the multi-select set if any, else the
    /// active row.
    pub(crate) fn chosen_rows(&self) -> Vec<usize> {
        let mut rows: Vec<usize> = if self.pane.marked.is_empty() {
            self.pane.selected.into_iter().collect()
        } else {
            self.pane.marked.iter().copied().collect()
        };
        rows.retain(|&i| i < self.pane.nodes.len());
        rows
    }

    /// Dual-pane view: copy the focused pane's chosen files into the other
    /// pane's disk or new archive, as one step of that pane's Undo.
    pub(crate) fn copy_to_pane(&mut self) {
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
    pub(crate) fn copy_missing(&mut self, to_right: bool) {
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

    /// Save this pane: the disk where it came from (or Save As if it's
    /// new), or the new archive through Save As.
    pub(crate) fn save_pane(&mut self) {
        if self.pane.archive.is_some() {
            self.save_archive();
        } else {
            self.save(false);
        }
    }

    pub(crate) fn save_archive(&mut self) {
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
            Ok(()) => {
                self.pane.archive_saved = self.pane.archive.clone();
                self.pane.refresh();
                self.status = format!("Saved {} ({} bytes)", path.display(), bytes.len());
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    pub(crate) fn validate(&mut self) {
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
    pub(crate) fn repair_disk(&mut self) {
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
                self.pane.refresh();
                self.validate();
                self.status = done;
            }
            Err(e) => self.status = format!("Repair failed, nothing changed: {e}"),
        }
    }

    /// Which disk the Sector Editor should show: the focused pane's file.
    pub(crate) fn sector_source(&self) -> String {
        format!(
            "{}|{}",
            self.pane.opened_path.as_deref().unwrap_or(""),
            self.focus_right
        )
    }

    /// Keep the Sector Editor on the focused pane's disk: when that changes,
    /// show the same block of the new disk (or its header) and drop any
    /// unwritten edits; otherwise re-read the block unless edits are staged.
    pub(crate) fn sector_sync(&mut self) {
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
    pub(crate) fn run_find(&mut self) {
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
    pub(crate) fn find_show(&mut self, node: usize, offset: usize) {
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
    pub(crate) fn sector_write(&mut self) {
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
                self.pane.refresh();
                self.status = done;
            }
            Err(e) => self.status = format!("Couldn't write track {t}, sector {s}: {e}"),
        }
    }

    /// The type File > Add Files gives a file whose extension doesn't say.
    pub(crate) fn fallback_type(&self) -> FileType {
        match self.add_type {
            1 => FileType::Seq,
            2 => FileType::Usr,
            _ => FileType::Prg,
        }
    }

    /// Add one file of the fallback type (the tests' shortcut).
    #[cfg(test)]
    pub(crate) fn add(&mut self, name: String, bytes: &[u8]) {
        let file = ImportFile {
            name,
            file_type: self.fallback_type(),
            data: bytes.to_vec(),
        };
        self.add_imports(vec![Ok(file)]);
    }

    /// Add host files: each one's name and type come from its file name
    /// (`game.prg`, `notes.seq`) or its PC64 header (`.P00`, `.S00`).
    pub(crate) fn add_paths(&mut self, paths: &[PathBuf]) {
        let fallback = self.fallback_type();
        let files = paths
            .iter()
            .map(|p| {
                let shown = p.file_name().map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                std::fs::read(p)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| host_import(p, bytes, fallback))
                    .map_err(|e| format!("{shown} ({e})"))
            })
            .collect();
        self.add_imports(files);
    }

    /// Put files on the open disk or new archive as one undo step, and say
    /// what was added and what couldn't be.
    pub(crate) fn add_imports(&mut self, files: Vec<std::result::Result<ImportFile, String>>) {
        let before = self.pane.snapshot();
        let mut added: Vec<(String, usize)> = Vec::new();
        let mut failed: Vec<String> = Vec::new();
        for file in files {
            let file = match file {
                Ok(f) => f,
                Err(e) => {
                    failed.push(e);
                    continue;
                }
            };
            let put = if let Some(ed) = &mut self.pane.disk {
                ed.disk
                    .write_file(&file.name, file.file_type, &file.data)
                    .map(|()| ed.dirty = true)
                    .map_err(|e| e.to_string())
            } else if let Some(stage) = &mut self.pane.archive {
                stage.push(file.clone());
                Ok(())
            } else {
                Err("open a disk or start a new archive first".into())
            };
            match put {
                Ok(()) => added.push((file.name, file.data.len())),
                Err(e) => failed.push(format!("\"{}\" ({e})", file.name)),
            }
        }
        let done = match added.as_slice() {
            [] => String::new(),
            [(name, len)] => format!("Added \"{name}\" ({len} bytes)"),
            many => format!("Added {} files", many.len()),
        };
        if !added.is_empty() {
            self.pane.record(before, &done);
            self.pane.refresh();
        }
        self.status = match (done.is_empty(), failed.as_slice()) {
            (_, []) => done,
            (true, [one]) => format!("Cannot add {one}"),
            (true, many) => format!("Cannot add {} files: {}", many.len(), many.join(", ")),
            (false, some) => format!("{done}; cannot add {}", some.join(", ")),
        };
    }

    /// BASIC editor: load text from a file.
    pub(crate) fn basic_open(&mut self) {
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
    pub(crate) fn basic_from_selected(&mut self) {
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
    pub(crate) fn basic_save_text(&mut self) {
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
    pub(crate) fn basic_add(&mut self) {
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
        self.pane.refresh();
        // Selected, it's ready to preview or run.
        self.pane.selected = self.pane.nodes.iter().position(|n| n.name == name);
        self.basic.modified = false;
        self.status = done;
    }

    /// File > Convert To: write this pane's files (unsaved edits included)
    /// to a new disk or archive of another format, chosen in a Save dialog.
    /// The pane stays as it is.
    pub(crate) fn convert_to(&mut self, to: sector64::convert::Target) {
        let stem = self
            .pane
            .opened_path
            .as_deref()
            .and_then(|p| Path::new(p).file_stem()?.to_str().map(str::to_string))
            .unwrap_or_else(|| "converted".into());
        let converted = match self.pane.map_disk() {
            Some(disk) => match sector64::convert::convert_disk(disk, to) {
                Ok(c) => c,
                Err(e) => {
                    self.status = format!("Cannot convert this disk: {e}");
                    return;
                }
            },
            None if !self.pane.nodes.is_empty() => {
                let (files, unreadable) = listed_files(&self.pane.nodes);
                let mut c = sector64::convert::convert_files(&stem.to_uppercase(), &files, to);
                c.skipped.splice(0..0, unreadable);
                c
            }
            None => {
                self.status = "Open a disk or archive to convert it".into();
                return;
            }
        };
        let ext = to.ext();
        let Some(path) = rfd::FileDialog::new()
            .add_filter(ext.to_uppercase(), &[ext])
            .set_file_name(format!("{stem}.{ext}"))
            .save_file()
        else {
            return;
        };
        let path = path.with_extension(ext);
        match std::fs::write(&path, &converted.bytes) {
            Ok(()) => {
                let shown = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                self.status = convert_report(&shown, &converted);
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    /// Change the open disk as one undo step: `f` makes the change and says
    /// what it did. Returns what `f` returned, or `None` (with the reason
    /// on the status line) if it failed.
    pub(crate) fn edit_disk<T>(
        &mut self,
        f: impl FnOnce(&mut Disk) -> sector64::Result<(T, String)>,
    ) -> Option<T> {
        let before = self.pane.snapshot();
        let ed = self.pane.disk.as_mut()?;
        match f(&mut ed.disk) {
            Ok((out, done)) => {
                ed.dirty = true;
                self.pane.record(before, &done);
                self.pane.refresh();
                self.status = done;
                Some(out)
            }
            Err(e) => {
                self.status = format!("Couldn't change the directory: {e}");
                None
            }
        }
    }

    /// Move the selected entry one place up or down the directory (or the
    /// new archive's list), keeping it selected.
    pub(crate) fn move_entry(&mut self, down: bool) {
        let Some(i) = self.pane.selected else {
            self.status = "Select a file to move".into();
            return;
        };
        if self.pane.sort.is_some() {
            self.status = "Click the sorted column until the list is back in directory order, \
                           then move files"
                .into();
            return;
        }
        let to = if down {
            i.checked_add(1)
        } else {
            i.checked_sub(1)
        };
        let Some(to) = to.filter(|&t| t < self.pane.nodes.len()) else {
            return; // already at the top or bottom
        };
        let name = self.pane.nodes[i].name.clone();
        let done = format!("Moved \"{name}\" {}", if down { "down" } else { "up" });
        if self.pane.disk.is_some() {
            if self
                .edit_disk(|d| d.move_entry(i, to).map(|()| ((), done)))
                .is_none()
            {
                return;
            }
        } else if let Some(stage) = &mut self.pane.archive {
            let before = Some(Snapshot::Archive(stage.clone()));
            stage.swap(i, to);
            self.pane.record(before, &done);
            self.pane.refresh();
            self.status = done;
        }
        self.pane.selected = Some(to);
    }

    /// Lock the selected files (the marked ones, or else the selected row),
    /// or unlock them if they're all locked already.
    pub(crate) fn toggle_lock(&mut self) {
        let mut rows: Vec<usize> = if self.pane.marked.is_empty() {
            self.pane.selected.into_iter().collect()
        } else {
            self.pane.marked.iter().copied().collect()
        };
        rows.sort_unstable();
        let Some(dir) = self
            .pane
            .disk
            .as_ref()
            .and_then(|e| e.disk.directory().ok())
        else {
            return;
        };
        rows.retain(|&r| r < dir.len());
        if rows.is_empty() {
            self.status = "Select a file to lock or unlock".into();
            return;
        }
        let lock = rows.iter().any(|&r| !dir[r].locked);
        let what = match rows.as_slice() {
            [r] => format!("\"{}\"", dir[*r].name),
            many => format!("{} files", many.len()),
        };
        let done = format!("{} {what}", if lock { "Locked" } else { "Unlocked" });
        let (sel, marked) = (self.pane.selected, self.pane.marked.clone());
        let ok = self.edit_disk(|d| {
            for &r in &rows {
                d.set_locked(r, lock)?;
            }
            Ok(((), done))
        });
        if ok.is_some() {
            // The rows didn't move, so the selection still holds.
            self.pane.selected = sel;
            self.pane.marked = marked;
        }
    }

    /// Add a separator line after the selected entry (or first).
    pub(crate) fn add_separator(&mut self) {
        let after = self.pane.selected;
        if self.pane.sort.is_some() {
            self.status = "Click the sorted column until the list is back in directory order, \
                           then add a separator"
                .into();
            return;
        }
        let at = self.edit_disk(|d| {
            d.add_separator(sector64::SEPARATOR, after)
                .map(|at| (at, "Added a separator line".to_string()))
        });
        if at.is_some() {
            self.pane.selected = at;
        }
    }

    /// Give the disk the name and ID typed in the Disk Name dialog.
    pub(crate) fn set_header(&mut self) {
        let (name, id) = (
            self.disk_name.trim().to_string(),
            self.disk_id.trim().to_string(),
        );
        let done = format!("Named the disk \"{name}\", ID {id}");
        self.edit_disk(|d| d.set_header(&name, &id).map(|()| ((), done)));
    }

    pub(crate) fn rename(&mut self) {
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
            match ed.disk.rename_entry(i, &new) {
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

    pub(crate) fn delete(&mut self) {
        let Some(i) = self.pane.selected else {
            self.status = "Select a file to delete".into();
            return;
        };
        let name = self.pane.nodes[i].name.clone();
        let before = self.pane.snapshot();
        if let Some(ed) = &mut self.pane.disk {
            match ed.disk.scratch_entry(i) {
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

    pub(crate) fn undo(&mut self) {
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

    pub(crate) fn redo(&mut self) {
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

    pub(crate) fn save(&mut self, save_as: bool) {
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
                dlg.set_file_name(default_disk_name(&ed.disk))
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

    pub(crate) fn extract_selected(&mut self) {
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

    pub(crate) fn extract_all(&mut self) {
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
    pub(crate) fn pump_toasts(&mut self, now: f64) {
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
    pub(crate) fn show_toasts(&mut self, ctx: &egui::Context) {
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
