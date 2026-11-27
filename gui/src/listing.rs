//! The file listing: columns, header, rows, renaming in place and keyboard stepping.

use crate::*;

/// The PACKED column text for compressed archive members: each packed size
/// right-aligned to the widest one, so the method names after it line up.
pub(crate) fn packed_details(items: impl Iterator<Item = (usize, String)>) -> Vec<String> {
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

/// Where Up (`down` false) or Down moves the selection in the listing's
/// display `order`: the row's position next to `current`'s, or the first
/// row when nothing shown is selected. `None` when nothing is shown.
pub(crate) fn step_in_order(order: &[usize], current: Option<usize>, down: bool) -> Option<usize> {
    let last = order.len().checked_sub(1)?;
    let pos = current.and_then(|s| order.iter().position(|&x| x == s));
    Some(match (pos, down) {
        (None, _) => 0,
        (Some(p), true) => (p + 1).min(last),
        (Some(p), false) => p.saturating_sub(1),
    })
}

/// Fixed column widths (in characters) for the listing, computed once per
/// frame from the widest row so the columns line up.
#[derive(Clone, Copy)]
pub(crate) struct Cols {
    pub(crate) detail_chars: usize,
    pub(crate) size_digits: usize,
    /// Some row can't be extracted, so the size column must also fit the
    /// "⚠ can't extract" marker.
    pub(crate) has_errors: bool,
    /// The longest name in the listing.
    pub(crate) name_chars: usize,
}

/// The marker shown in the size column for a row whose bytes can't be decoded.
pub(crate) const CANT_EXTRACT: &str = "⚠ can't extract";

/// The fewest name characters a row keeps before the listing starts dropping
/// columns to make room.
pub(crate) const MIN_NAME_CHARS: usize = 8;

impl Cols {
    /// Column widths for a listing: the widest detail text and size.
    pub(crate) fn of(nodes: &[Node]) -> Cols {
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
    pub(crate) fn size_chars(self) -> usize {
        let bytes = self.size_digits + 6;
        if self.has_errors {
            bytes.max(CANT_EXTRACT.chars().count())
        } else {
            bytes
        }
    }

    /// Characters a row needs for the type badge, a minimum-length name and
    /// the given optional columns, including the gaps between them.
    pub(crate) fn row_chars(self, detail: bool, size: bool) -> usize {
        self.row_chars_with_name(MIN_NAME_CHARS, detail, size)
    }

    /// As [`Cols::row_chars`], with room for a `name`-character name.
    pub(crate) fn row_chars_with_name(self, name: usize, detail: bool, size: bool) -> usize {
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
pub(crate) struct Placement {
    /// Left edge of the size column, if shown.
    pub(crate) status_x: Option<f32>,
    /// Left edge of the detail (BLK / PACKED) column, if shown.
    pub(crate) detail_x: Option<f32>,
    /// Right limit for the name.
    pub(crate) name_end: f32,
}

/// Lay out a row from `left` (the row's left edge, inside padding) to
/// `right_edge`. Header and rows call this with the same inputs, so they
/// always agree on which columns are shown.
pub(crate) fn place_cols(left: f32, right_edge: f32, cw: f32, cols: Cols) -> Placement {
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
pub(crate) enum RenameEdit {
    /// Not editing this row.
    None,
    /// Enter pressed — apply the new name.
    Commit,
    /// Focus lost or Escape — discard.
    Cancel,
}

/// The inline-rename state a pane's list edits: the row being renamed, the
/// text typed so far, and whether the field should take focus this frame.
pub(crate) type RenameState<'a> = (&'a mut Option<usize>, &'a mut String, &'a mut bool);

/// Draw one pane: the welcome screen when it's empty, otherwise the header,
/// the filter box and the file list. `side` keeps each pane's widget ids
/// apart; `rename` is given only to the focused pane, the only one that
/// renames in place. Clicks and menu picks become `acts`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pane_ui(
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
pub(crate) fn header_row(
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
pub(crate) enum RowSel {
    No,
    Yes,
    Unfocused,
}

/// Draw one file as a full-width, painted, selectable row: a colour-coded type
/// badge, the name, its detail, and a right-aligned size (or an error). When
/// `edit` is `Some`, the name is shown as an editable field instead.
pub(crate) fn file_row(
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
