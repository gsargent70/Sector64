//! Tool windows: disk map, Find in Files, sector editor, validation and Compare Panes.

use crate::*;

/// One file the Find window matched.
pub(crate) struct FindHit {
    /// Its row in the listing, and its name then (to notice a changed list).
    pub(crate) node: usize,
    pub(crate) name: String,
    pub(crate) badge: String,
    /// Whether the name itself matches (text searches only).
    pub(crate) in_name: bool,
    /// The first few match offsets, and how many there are in all.
    pub(crate) offsets: Vec<usize>,
    pub(crate) total: usize,
    /// Whether its matches can be shown in the Sector Editor (a plain file
    /// on a disk image).
    pub(crate) on_disk: bool,
}

/// How many offsets the Find window lists per file.
pub(crate) const FIND_OFFSETS: usize = 24;

/// The Find window: the query, the last search's results, and the match
/// the hex preview highlights.
#[derive(Default)]
pub(crate) struct FindState {
    pub(crate) query: String,
    /// Search for hex bytes (`true`) or text.
    pub(crate) hex: bool,
    /// Matches open in the Sector Editor instead of the preview.
    pub(crate) in_sector: bool,
    pub(crate) hits: Vec<FindHit>,
    /// What the last search found (or why it couldn't run).
    pub(crate) summary: String,
    /// How long a match is, for the highlight.
    pub(crate) len: usize,
    /// Which listing the results are for (see `App::sector_source`).
    pub(crate) source: String,
    /// The match the hex preview highlights: (row, offset, length).
    pub(crate) mark: Option<(usize, usize, usize)>,
    /// Scroll the preview to the mark on the next frame.
    pub(crate) scroll: bool,
    /// Put the keyboard in the query box on the next frame.
    pub(crate) focus: bool,
}

/// One-line totals for the disk map's header.
pub(crate) fn map_summary(map: &BlockMap) -> String {
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
pub(crate) fn block_color(
    map: &BlockMap,
    block: &sector64::MapBlock,
    selected: Option<usize>,
) -> Color32 {
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
pub(crate) fn block_tooltip(map: &BlockMap, track: u8, sector: u8) -> String {
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
pub(crate) fn disk_map_ui(
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
pub(crate) fn find_ui(
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
pub(crate) fn find_results_ui(ui: &mut egui::Ui, f: &FindState, stale: bool, acts: &mut Vec<Act>) {
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
pub(crate) struct SectorView<'a> {
    pub(crate) disk: &'a Disk,
    pub(crate) map: &'a BlockMap,
    /// The listing's selected file (its index in the map's files).
    pub(crate) selected: Option<usize>,
    /// Whether the disk can be changed (not a G64 or other read-only one).
    pub(crate) writable: bool,
}

/// Columns of the Sector Editor's grid, in characters: a two-digit offset,
/// sixteen bytes in hex (an extra space after the eighth), then the text.
pub(crate) const SEC_HEX0: usize = 4;
pub(crate) const SEC_TEXT0: usize = SEC_HEX0 + 16 * 3 + 1 + 2;
pub(crate) const SEC_COLS: usize = SEC_TEXT0 + 16;
/// The fill behind the bytes of the last Find match.
const SEC_MATCH: Color32 = Color32::from_rgb(0x3e, 0x5c, 0x3a);

/// The grid column where byte `i` of a row starts in hex.
pub(crate) fn sec_hex_col(i: usize) -> usize {
    SEC_HEX0 + i * 3 + usize::from(i >= 8)
}

/// The byte (0..16) under grid column `col` of a row, in either the hex or
/// the text part.
pub(crate) fn sec_byte_at(col: usize) -> Option<usize> {
    if (SEC_TEXT0..SEC_COLS).contains(&col) {
        return Some(col - SEC_TEXT0);
    }
    (0..16).find(|&i| (sec_hex_col(i)..sec_hex_col(i) + 2).contains(&col))
}

/// Draw the Sector Editor: where it is and how to move, who owns the block,
/// its link, the hex grid, and Write / Revert.
pub(crate) fn sector_ui(
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

    // Find: text (any C64 case) or hex bytes, anywhere on the disk, from
    // the cursor on.
    ui.horizontal_wrapped(|ui| {
        ui.label("Find");
        let hint = if ed.find_hex { "A9 ?? 8D" } else { "text" };
        let field = ui.add(
            egui::TextEdit::singleline(&mut ed.find_query)
                .desired_width(150.0)
                .hint_text(hint),
        );
        ui.selectable_value(&mut ed.find_hex, false, "TEXT")
            .on_hover_text("letters match in either case, whichever character set wrote them");
        ui.selectable_value(&mut ed.find_hex, true, "HEX")
            .on_hover_text("bytes in hex, ?? for any byte");
        // Enter in the field searches (and isn't also Extract).
        let enter = field.lost_focus()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let ready = !ed.find_query.trim().is_empty();
        let clicked = ui
            .add_enabled(idle && ready, egui::Button::new("Find next"))
            .on_hover_text("go to the next match on the disk, after the cursor (Back returns)")
            .on_disabled_hover_text(if idle { "type what to look for" } else { busy })
            .clicked();
        if (clicked || enter) && idle && ready {
            ed.find_next(disk);
        }
    });
    if !ed.find_msg.is_empty() {
        ui.label(egui::RichText::new(&ed.find_msg).color(c64::TEXT_DIM));
    }

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
                // The column clicked decides what typing puts in.
                ed.text_mode = col >= SEC_TEXT0;
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
                egui::Event::Text(text) if v.writable => {
                    if ed.text_mode {
                        text.chars().for_each(|c| ed.type_char(c));
                    } else {
                        text.chars()
                            .filter_map(sector::hex_digit)
                            .for_each(|d| ed.type_digit(d));
                    }
                }
                egui::Event::Key {
                    key, pressed: true, ..
                } => match key {
                    egui::Key::ArrowLeft => ed.move_cursor(-1),
                    egui::Key::Backspace if ed.text_mode => ed.move_cursor(-1),
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
            } else if ed.in_match(n) {
                c64::BRIGHT
            } else if n < 2 {
                c64::ACCENT
            } else {
                c64::TEXT
            };
            let (hx, tx) = (at(sec_hex_col(i), row), at(SEC_TEXT0 + i, row));
            let cell = |p: egui::Pos2, w: f32| {
                egui::Rect::from_min_size(p - egui::vec2(0.0, 1.5), egui::vec2(w, rh))
            };
            let matched = ed.in_match(n);
            if matched {
                painter.rect_filled(cell(hx, 2.0 * cw), 0.0, SEC_MATCH);
                painter.rect_filled(cell(tx, cw), 0.0, SEC_MATCH);
            }
            if n == ed.cursor {
                // The column typing goes into gets the stronger fill.
                let (hex_fill, text_fill) = if ed.text_mode {
                    (c64::STRIPE, cursor_fill)
                } else {
                    (cursor_fill, c64::STRIPE)
                };
                painter.rect_filled(cell(hx, 2.0 * cw), 1.0, hex_fill);
                painter.rect_filled(cell(tx, cw), 1.0, text_fill);
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
        "Click a byte in the hex to type hex digits, or in the text to type text. \
         Arrows move; PgUp/PgDn step through blocks."
    } else {
        "This disk is read-only (view only). Arrows move; PgUp/PgDn step through blocks."
    };
    ui.label(egui::RichText::new(hint).color(c64::TEXT_DIM));
}

/// The validation window's contents: the check's results, a checkbox per
/// fix, and the buttons.
pub(crate) fn validation_ui(
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
pub(crate) fn fix_tip(fix: &Fix) -> &'static str {
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
pub(crate) fn compare_items(p: &Pane) -> Vec<CompareItem<'_>> {
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
pub(crate) fn compare_colour(status: CompareStatus) -> Color32 {
    match status {
        CompareStatus::Same | CompareStatus::Unreadable => c64::TEXT_DIM,
        CompareStatus::Differs(_) => c64::WARN,
        CompareStatus::OnlyLeft | CompareStatus::OnlyRight => c64::ORPHAN,
    }
}

/// The Compare window's contents: what each pane holds, a summary, and a
/// row per file saying how it compares. Clicking a row selects that file
/// in both panes.
pub(crate) fn compare_ui(
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
