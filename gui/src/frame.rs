//! `App::frame_ui`: one frame of the whole window.

use crate::*;

impl App {
    /// One frame of the whole window: `update` without eframe's `Frame`, so
    /// tests can run it on a bare context.
    pub(crate) fn frame_ui(&mut self, ctx: &egui::Context) {
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
        let any_dirty = self.pane.is_dirty() || self.back.is_dirty();
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
                self.pane.drop_previews();
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
        let mut help_page = self.help_page;
        let mut quit = false;
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
        let can_compare = !self.pane.nodes.is_empty() && !self.back.nodes.is_empty();
        let mut show_compare = self.show_compare && can_compare;
        let mut compare_all = self.compare_all;
        let can_run = self.vice_plan().is_ok();
        let mut show_basic = self.show_basic;
        let mut show_new_disk = self.show_new_disk;
        let mut show_disk_name = self.show_disk_name;
        let mut disk_name = std::mem::take(&mut self.disk_name);
        let mut disk_id = std::mem::take(&mut self.disk_id);
        // Reordering works on directory order, so not while sorted.
        let in_dir_order = self.pane.sort.is_none();
        let mut basic = std::mem::take(&mut self.basic);
        let mut dual = self.dual;
        let back_editable = self.back.editable();

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(2.0);
            egui::menu::bar(ui, |ui| {
                // The floppy-disk logo, as tall as the title beside it.
                let h = ui.text_style_height(&egui::TextStyle::Heading) + 4.0;
                ui.add(egui::Image::new((ic.logo.id(), egui::vec2(h, h))));
                // The title goes when the window is too narrow for it and
                // every menu; the menus' width is measured as they're drawn.
                let menus_id = egui::Id::new("menu_bar_menus_width");
                let menus_w: f32 = ctx.data(|d| d.get_temp(menus_id)).unwrap_or(0.0);
                let title = egui::RichText::new("SECTOR64")
                    .heading()
                    .color(c64::ACCENT)
                    .strong();
                let title_w = egui::WidgetText::from(title.clone())
                    .into_galley(
                        ui,
                        Some(egui::TextWrapMode::Extend),
                        f32::INFINITY,
                        egui::TextStyle::Heading,
                    )
                    .size()
                    .x;
                let gap = ui.spacing().item_spacing.x;
                if title_w + 4.0 * gap + menus_w <= ui.available_width() {
                    ui.label(title);
                }
                ui.separator();
                let menus_x = ui.cursor().min.x;

                ui.menu_button("File", |ui| {
                    ui.set_min_width(180.0);
                    // Grow the menu to fit rather than wrap an item's name.
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    // New ▸ (a disk image, set up in a dialog, or an archive)
                    egui::menu::menu_custom_button(ui, icon_button(&ic.doc, "New ▸"), |ui| {
                        ui.set_min_width(200.0);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        if menu_entry(
                            ui,
                            true,
                            entry(&ic.floppy, "Disk Image…", "Ctrl+N"),
                            "a blank D64, D71 or D81: pick its name, ID and format",
                            "",
                        ) {
                            show_new_disk = true;
                        }
                        if menu_entry(
                            ui,
                            true,
                            entry(&ic.blank, "Empty Archive", ""),
                            "add files to it, then Save As… a T64, LNX, LBR or P00",
                            "",
                        ) {
                            acts.push(Act::NewArchive);
                        }
                    });
                    if menu_entry(ui, true, entry(&ic.folder, "Open…", "Ctrl+O"), "", "") {
                        acts.push(Act::OpenDialog);
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
                    ui.separator();
                    if menu_entry(
                        ui,
                        is_disk,
                        entry(&ic.floppy, "Save", "Ctrl+S"),
                        "",
                        "open a disk image to save it",
                    ) {
                        acts.push(Act::Save);
                    }
                    if menu_entry(
                        ui,
                        is_disk || is_archive,
                        entry(&ic.floppy, "Save As…", ""),
                        "",
                        "open a disk or start a new archive to save it",
                    ) {
                        acts.push(if is_archive {
                            Act::SaveArchiveAs
                        } else {
                            Act::SaveAs
                        });
                    }
                    // Convert To ▸ (a disk or archive of another format)
                    ui.add_enabled_ui(!self.pane.nodes.is_empty(), |ui| {
                        egui::menu::menu_custom_button(
                            ui,
                            icon_button(&ic.floppy, "Convert To ▸"),
                            |ui| {
                                ui.set_min_width(180.0);
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                                for to in sector64::convert::Target::ALL {
                                    let kind = match to {
                                        sector64::convert::Target::T64 => "tape",
                                        t if t.is_disk() => "disk",
                                        _ => "archive",
                                    };
                                    let label = format!("{} {kind}…", to.ext().to_uppercase());
                                    if menu_entry(
                                        ui,
                                        true,
                                        entry(&ic.blank, &label, ""),
                                        "write this pane's files to a new file of this \
                                         format; the pane stays as it is",
                                        "",
                                    ) {
                                        acts.push(Act::ConvertTo(to));
                                    }
                                }
                            },
                        );
                    })
                    .response
                    .on_disabled_hover_text("open a disk or archive to convert it");
                    ui.separator();
                    // Add Files ▸ (pick several; the type for files whose
                    // name doesn't give one) — only with a disk/archive open.
                    ui.add_enabled_ui(editable, |ui| {
                        egui::menu::menu_custom_button(
                            ui,
                            icon_button(&ic.plus, "Add Files ▸"),
                            |ui| {
                                ui.set_min_width(200.0);
                                ui.label(
                                    egui::RichText::new("other files as").color(c64::TEXT_DIM),
                                )
                                .on_hover_text(
                                    "a file named .prg, .seq, .usr or .del, or a PC64 \
                                         .P00/.S00/.U00 file, gets the type it names; \
                                         any other file gets this one",
                                );
                                ui.horizontal(|ui| {
                                    for (i, t) in ["prg", "seq", "usr"].iter().enumerate() {
                                        if ui.selectable_label(add_type == i, *t).clicked() {
                                            add_type = i;
                                        }
                                    }
                                });
                                if ui.add(icon_button(&ic.plus, "Add Files…")).clicked() {
                                    acts.push(Act::AddFile);
                                    ui.close_menu();
                                }
                            },
                        );
                    });
                    if menu_entry(
                        ui,
                        sel_extractable,
                        entry(&ic.down, "Extract Selected", "Enter"),
                        "save the selected file(s) into the Output folder",
                        "select a file to extract it",
                    ) {
                        acts.push(Act::ExtractSelected);
                    }
                    if menu_entry(
                        ui,
                        any_extractable,
                        entry(&ic.down, "Extract All", ""),
                        "save every file into the Output folder",
                        "open a disk or archive first",
                    ) {
                        acts.push(Act::ExtractAll);
                    }
                    // A Mac has Quit in its application menu already.
                    if !cfg!(target_os = "macos") {
                        ui.separator();
                        if menu_entry(ui, true, entry(&ic.blank, "Quit", "Ctrl+Q"), "", "") {
                            quit = true;
                        }
                    }
                });

                ui.separator();
                ui.menu_button("Edit", |ui| {
                    ui.set_min_width(180.0);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    // Undo / Redo, with what they would undo or redo on hover.
                    let undo_next = self.pane.undo.last().map(|s| s.label.clone());
                    let redo_next = self.pane.redo.last().map(|s| s.label.clone());
                    if menu_entry(
                        ui,
                        undo_next.is_some(),
                        entry(&ic.blank, "Undo", "Ctrl+Z"),
                        undo_next.as_deref().unwrap_or(""),
                        "nothing to undo",
                    ) {
                        acts.push(Act::Undo);
                    }
                    if menu_entry(
                        ui,
                        redo_next.is_some(),
                        entry(&ic.blank, "Redo", "Ctrl+Y"),
                        redo_next.as_deref().unwrap_or(""),
                        "nothing to redo",
                    ) {
                        acts.push(Act::Redo);
                    }
                    ui.separator();
                    let pick_tip = "select a file on a disk or new archive";
                    if menu_entry(
                        ui,
                        editable && has_sel,
                        entry(&ic.pencil, "Rename", "F2"),
                        "",
                        pick_tip,
                    ) {
                        if let Some(s) = sel_now {
                            renaming = Some(s);
                            rename_to = self.pane.nodes[s].name.clone();
                            rename_focus = true;
                        }
                    }
                    let del = if is_archive { "Remove" } else { "Delete" };
                    if menu_entry(
                        ui,
                        editable && has_sel,
                        entry(&ic.trash, del, "Del"),
                        "",
                        pick_tip,
                    ) {
                        acts.push(Act::Delete);
                    }
                    if menu_entry(
                        ui,
                        !self.pane.nodes.is_empty(),
                        entry(&ic.blank, "Select All", "Ctrl+A"),
                        "select every file listed (the filter decides which)",
                        "open a disk or archive first",
                    ) {
                        acts.push(Act::SelectAll);
                    }
                    ui.separator();
                    let order_tip = if in_dir_order {
                        "select a file on a disk or new archive"
                    } else {
                        "click the sorted column until the list is in directory order"
                    };
                    if menu_entry(
                        ui,
                        editable && has_sel && in_dir_order,
                        entry(&ic.blank, "Move Up", "Alt+Up"),
                        "move the selected file one place up the directory",
                        order_tip,
                    ) {
                        acts.push(Act::MoveEntry(false));
                    }
                    if menu_entry(
                        ui,
                        editable && has_sel && in_dir_order,
                        entry(&ic.blank, "Move Down", "Alt+Down"),
                        "move the selected file one place down the directory",
                        order_tip,
                    ) {
                        acts.push(Act::MoveEntry(true));
                    }
                    let disk_tip = "open a D64, D71, D81 or D2M disk";
                    if menu_entry(
                        ui,
                        is_disk && in_dir_order,
                        entry(&ic.blank, "Add Separator", ""),
                        "add a ---------------- line (a 0-block DEL entry) after the \
                         selected file",
                        if is_disk { order_tip } else { disk_tip },
                    ) {
                        acts.push(Act::AddSeparator);
                    }
                    if menu_entry(
                        ui,
                        is_disk && has_sel,
                        entry(&ic.blank, "Lock / Unlock", "Ctrl+Shift+L"),
                        "lock the selected files (shown <), or unlock them if they're \
                         all locked",
                        if is_disk {
                            "select a file on the disk"
                        } else {
                            disk_tip
                        },
                    ) {
                        acts.push(Act::ToggleLock);
                    }
                    if menu_entry(
                        ui,
                        is_disk,
                        entry(&ic.blank, "Disk Name and ID…", ""),
                        "change the name and ID in the directory's header line",
                        disk_tip,
                    ) {
                        acts.push(Act::OpenDiskName);
                    }
                    ui.separator();
                    if menu_entry(
                        ui,
                        dual && has_sel && back_editable,
                        entry(&ic.blank, "Copy to Other Pane", "F5"),
                        "copy the selected files into the other pane",
                        if dual {
                            "select files, with a disk or new archive open in the other pane"
                        } else {
                            "turn on View > Dual Panes first"
                        },
                    ) {
                        acts.push(Act::CopyToPane);
                    }
                });

                ui.separator();
                ui.menu_button("View", |ui| {
                    ui.set_min_width(180.0);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    if menu_entry(
                        ui,
                        true,
                        entry(&ic.blank, "Preview", "Ctrl+P").selected(show_preview),
                        "show or hide the preview pane",
                        "",
                    ) {
                        show_preview = !show_preview;
                        // The preview takes the other pane's place; Dual
                        // Panes brings that pane back.
                        if show_preview {
                            dual = false;
                        }
                    }
                    if menu_entry(
                        ui,
                        true,
                        entry(&ic.blank, "Dual Panes", "Ctrl+K").selected(dual),
                        "show two disks side by side; Tab switches between them \
                         (hides the preview pane)",
                        "",
                    ) {
                        dual = !dual;
                        if dual {
                            show_preview = false;
                        }
                    }
                    ui.separator();
                    if menu_entry(
                        ui,
                        has_map,
                        entry(&ic.blank, "Disk Map", "Ctrl+M").selected(show_map),
                        "show which file owns each block",
                        "open a disk image to see its map",
                    ) {
                        show_map = !show_map;
                    }
                });

                ui.separator();
                ui.menu_button("Tools", |ui| {
                    ui.set_min_width(180.0);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    if menu_entry(
                        ui,
                        is_disk,
                        entry(&ic.blank, "Validate / Repair", "Ctrl+L"),
                        "check every file's blocks against the BAM, and repair them",
                        "open a D64, D71, D81 or CMD-native D2M to validate it",
                    ) {
                        acts.push(Act::Validate);
                    }
                    if menu_entry(
                        ui,
                        has_map,
                        entry(&ic.blank, "Sector Editor", "Ctrl+E").selected(show_sector),
                        "view and edit any track and sector in hex",
                        "open a disk image to edit its sectors",
                    ) {
                        show_sector = !show_sector;
                    }
                    ui.separator();
                    if menu_entry(
                        ui,
                        can_compare,
                        entry(&ic.blank, "Compare Panes", "Ctrl+D").selected(show_compare),
                        "list which files match, differ, or are on only one side",
                        "open a disk or archive in each pane (Dual Panes) to compare them",
                    ) {
                        show_compare = !show_compare;
                    }
                    if menu_entry(
                        ui,
                        true,
                        entry(&ic.blank, "Find in Files…", "Ctrl+F").selected(show_find),
                        "look for text or hex bytes inside every file listed",
                        "",
                    ) {
                        show_find = !show_find;
                    }
                    ui.separator();
                    if menu_entry(
                        ui,
                        true,
                        entry(&ic.blank, "BASIC Editor…", "Ctrl+B").selected(show_basic),
                        "write a BASIC program, or edit the selected one",
                        "",
                    ) {
                        show_basic = !show_basic;
                    }
                    if menu_entry(
                        ui,
                        can_run,
                        entry(&ic.blank, "Run in VICE", "Ctrl+R"),
                        "start this disk in the VICE emulator, loading the selected \
                         program or else the first one",
                        "open a disk or select a program to run it in VICE",
                    ) {
                        acts.push(Act::RunVice);
                    }
                });

                ui.separator();
                let help = ui.menu_button("Help", |ui| {
                    ui.set_min_width(180.0);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    for (page, label, keys) in [
                        (HelpPage::Manual, "Manual", "F1"),
                        (HelpPage::Shortcuts, "Keyboard Shortcuts", ""),
                        (HelpPage::About, "About Sector64", ""),
                    ] {
                        if menu_entry(ui, true, entry(&ic.blank, label, keys), "", "") {
                            help_page = page;
                            show_help = true;
                        }
                    }
                });
                let menus_w = help.response.rect.max.x - menus_x;
                ui.ctx().data_mut(|d| d.insert_temp(menus_id, menus_w));
            });
            ui.add_space(2.0);
        });
        if quit {
            // Goes through the same unsaved-changes check as the window's
            // close button.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

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

        // New Disk dialog. It takes Esc and Enter for itself, so nothing
        // behind it closes or extracts while it's open.
        if show_new_disk {
            let (cancel, enter) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                )
            });
            let mut create = enter;
            let mut close = cancel;
            // Put the cursor in the name field when the dialog opens.
            let was_open = egui::Id::new("new_disk_was_open");
            let first = !ctx.data_mut(|d| d.get_temp(was_open).unwrap_or(false));
            ctx.data_mut(|d| d.insert_temp(was_open, true));
            egui::Window::new(
                egui::RichText::new("NEW DISK IMAGE")
                    .color(c64::ACCENT)
                    .strong(),
            )
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let form = (&mut new_name, &mut new_id, &mut new_fmt);
                new_disk_ui(ui, form, first, &mut create, &mut close)
            });
            if create {
                acts.push(Act::Create);
                close = true;
            }
            if close {
                show_new_disk = false;
                ctx.data_mut(|d| d.remove::<bool>(was_open));
            }
        }

        // Disk Name dialog: like New Disk, it takes Esc and Enter for itself.
        if show_disk_name {
            let (cancel, enter) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                )
            });
            let mut ok = enter;
            let mut close = cancel;
            let was_open = egui::Id::new("disk_name_was_open");
            let first = !ctx.data_mut(|d| d.get_temp(was_open).unwrap_or(false));
            ctx.data_mut(|d| d.insert_temp(was_open, true));
            egui::Window::new(egui::RichText::new("DISK NAME").color(c64::ACCENT).strong())
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    disk_name_ui(
                        ui,
                        (&mut disk_name, &mut disk_id),
                        first,
                        &mut ok,
                        &mut close,
                    )
                });
            if ok {
                acts.push(Act::SetHeader);
                close = true;
            }
            if close || !is_disk {
                show_disk_name = false;
                ctx.data_mut(|d| d.remove::<bool>(was_open));
            }
        }

        // Esc closes the window in front, one per press, in this order. The
        // New Disk and Disk Name dialogs above have already taken Esc if open; with the
        // cursor in the BASIC editor's text, Esc leaves the text and closes
        // nothing.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if show_help {
                show_help = false;
            } else if show_sector {
                show_sector = false;
            } else if show_map {
                show_map = false;
            } else if show_compare {
                show_compare = false;
            } else if self.show_validation {
                self.show_validation = false;
            } else if show_basic {
                show_basic = basic.typing;
            } else {
                show_find = false;
            }
        }

        // Validation window: what the check found, and the fixes it offers.
        if self.show_validation {
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

        // Help window: the user manual, its keyboard shortcuts, or what this
        // program is. Esc or the Close button dismisses it.
        if show_help {
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
                    for (page, label) in [
                        (HelpPage::Manual, "MANUAL"),
                        (HelpPage::Shortcuts, "SHORTCUTS"),
                        (HelpPage::About, "ABOUT"),
                    ] {
                        if ui.selectable_label(help_page == page, label).clicked() {
                            help_page = page;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            show_help = false;
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(("help_scroll", help_page))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match help_page {
                        HelpPage::Manual => {
                            ui.add(
                                egui::Label::new(egui::RichText::new(MANUAL).monospace())
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                        HelpPage::Shortcuts => {
                            let text = manual_section("KEYBOARD SHORTCUTS");
                            ui.add(
                                egui::Label::new(egui::RichText::new(text).monospace())
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                        HelpPage::About => about_ui(ui),
                    });
            });
        }

        // Disk map window: every block, coloured by owner. Clicking a file's
        // block selects it in the listing.
        if show_map {
            if self.pane.map_cache.is_none() {
                self.pane.map_cache = self.pane.map_disk().map(Disk::block_map);
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

        // Compare window: the two panes' files side by side, by name. It shows
        // both panes, so it brings Dual back if Preview had hidden one.
        if show_compare {
            if !dual {
                dual = true;
                show_preview = false;
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
                    which.push(self.pane.unsaved_name(true, self.dual));
                }
                if back_dirty {
                    which.push(self.back.unsaved_name(false, self.dual));
                }
                ui.label(unsaved_message(&which));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("💾 Save first").clicked() {
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
                    Some(Confirm::NewDisk) => self.create_disk(),
                    Some(Confirm::NewArchive) => self.new_archive(),
                    Some(Confirm::Quit) => {
                        for pane in [&mut self.pane, &mut self.back] {
                            if let Some(ed) = &mut pane.disk {
                                ed.dirty = false;
                            }
                            pane.archive_saved = pane.archive.clone();
                        }
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    None => {}
                },
                Some(1) => self.confirm = None,
                Some(2) => {
                    if self.pane.is_dirty() {
                        self.save_pane();
                    }
                    let quitting = matches!(self.confirm, Some(Confirm::Quit));
                    if quitting && self.back.is_dirty() {
                        // Save it as the focused pane, then swap back.
                        std::mem::swap(&mut self.pane, &mut self.back);
                        self.save_pane();
                        std::mem::swap(&mut self.pane, &mut self.back);
                    }
                    let unsaved = self.pane.is_dirty() || quitting && self.back.is_dirty();
                    if !unsaved {
                        match self.confirm.take() {
                            Some(Confirm::OpenFile(p)) => self.open(&p),
                            Some(Confirm::NewDisk) => self.create_disk(),
                            Some(Confirm::NewArchive) => self.new_archive(),
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
        if drop_editable && !dropped.is_empty() {
            acts.push(Act::AddPaths(dropped));
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
            if i.modifiers.command && i.key_pressed(egui::Key::N) {
                show_new_disk = true;
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
            // Ctrl+K opened the Copy window, which Dual Panes replaced.
            if i.modifiers.command && i.key_pressed(egui::Key::K) {
                dual = !dual;
                if dual {
                    show_preview = false;
                }
            }
            if i.modifiers.command && i.key_pressed(egui::Key::L) && is_disk {
                acts.push(if i.modifiers.shift {
                    Act::ToggleLock
                } else {
                    Act::Validate
                });
            }
            if i.modifiers.command && i.key_pressed(egui::Key::R) && can_run {
                acts.push(Act::RunVice);
            }
            if self.dual && i.key_pressed(egui::Key::F5) {
                acts.push(Act::CopyToPane);
            }
            if i.modifiers.command && i.key_pressed(egui::Key::P) {
                show_preview = !show_preview;
                if show_preview {
                    dual = false;
                }
            }
            // A Mac's application menu handles Cmd+Q itself.
            if !cfg!(target_os = "macos") && i.modifiers.command && i.key_pressed(egui::Key::Q) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if i.key_pressed(egui::Key::F1) {
                if show_help && help_page == HelpPage::Manual {
                    show_help = false;
                } else {
                    help_page = HelpPage::Manual;
                    show_help = true;
                }
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
                // Text fields keep Ctrl+A for selecting their own text.
                if i.modifiers.command && i.key_pressed(egui::Key::A) && n > 0 {
                    acts.push(Act::SelectAll);
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
                // The filter can hide rows, so clamp to what's shown.
                // With Alt they move the selected file through the directory.
                if i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp) {
                    let down = i.key_pressed(egui::Key::ArrowDown);
                    if i.modifiers.alt {
                        if editable && has_sel && in_dir_order {
                            acts.push(Act::MoveEntry(down));
                        }
                    } else if let Some(p) = step_in_order(&order, sel_now, down) {
                        acts.push(Act::Select(order[p]));
                    }
                }
            }
        });

        // Write edited strings back, then apply actions.
        self.pane.filter = filter;
        self.back.filter = back_filter;
        self.out_dir = out_dir;
        self.new_name = new_name;
        self.new_id = new_id;
        self.show_disk_name = show_disk_name;
        self.disk_name = disk_name;
        self.disk_id = disk_id;
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
        self.help_page = help_page;
        self.show_map = show_map;
        self.show_sector = show_sector;
        if show_find && !self.show_find {
            self.find.focus = true;
        }
        self.show_find = show_find;
        self.show_compare = show_compare;
        self.compare_all = compare_all;
        self.show_basic = show_basic;
        self.show_new_disk = show_new_disk;
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
