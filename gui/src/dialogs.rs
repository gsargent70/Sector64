//! Small windows: Help (manual, shortcuts, about), New Disk, and the unsaved-changes wording.

use crate::*;

/// The user manual, embedded so Help works with no external files.
pub(crate) const MANUAL: &str = include_str!("../MANUAL.md");

/// What the Help window shows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HelpPage {
    #[default]
    Manual,
    Shortcuts,
    About,
}

/// The Help window's About page: the name, version and licence.
pub(crate) fn about_ui(ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(format!("SECTOR64 v{}", env!("CARGO_PKG_VERSION")))
            .heading()
            .color(c64::ACCENT)
            .strong(),
    );
    ui.label("A toolkit for Commodore disk images and archives.");
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Licence: MIT or Apache-2.0").color(c64::TEXT_DIM));
    ui.hyperlink_to(
        "github.com/gsargent70/Sector64",
        "https://github.com/gsargent70/Sector64",
    );
}

/// The text of the manual's section `title` (e.g. "KEYBOARD SHORTCUTS"):
/// everything between its heading and the next one, trimmed.
pub(crate) fn manual_section(title: &str) -> &'static str {
    let rule = "\n----------------------------------------------------------------\n";
    // A heading is "17. KEYBOARD SHORTCUTS" between two rules.
    let is_heading = |part: &str| {
        part.trim()
            .split_once(". ")
            .is_some_and(|(_, t)| t == title)
    };
    MANUAL
        .split(rule)
        .skip_while(|part| !is_heading(part))
        .nth(1)
        .map(str::trim)
        .unwrap_or("")
}

/// What the unsaved-changes prompt says, given which disks have unsaved
/// changes (e.g. "the open disk").
/// The disk formats File > New > Disk Image offers, by `new_fmt`.
pub(crate) const NEW_DISK_FORMATS: [(&str, &str); 3] = [
    ("D64", "1541, 664 blocks free"),
    ("D71", "1571, 1328 blocks free"),
    ("D81", "1581, 3160 blocks free"),
];

/// The New Disk dialog's contents: name, ID, format, and Create / Cancel.
/// `first` is true on the frame it opens, to put the cursor in the name.
pub(crate) fn new_disk_ui(
    ui: &mut egui::Ui,
    (name, id, fmt): (&mut String, &mut String, &mut usize),
    first: bool,
    create: &mut bool,
    close: &mut bool,
) {
    // One line per format: grow the window rather than wrap them.
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    egui::Grid::new("new_disk_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Disk name");
            let field = ui.add(
                egui::TextEdit::singleline(name)
                    .char_limit(16)
                    .desired_width(180.0),
            );
            if first {
                field.request_focus();
            }
            ui.end_row();
            ui.label("ID");
            ui.add(
                egui::TextEdit::singleline(id)
                    .char_limit(2)
                    .desired_width(40.0),
            )
            .on_hover_text("two characters, shown after the name in the directory");
            ui.end_row();
        });
    // The formats go below, across the whole width, so the dialog still
    // fits a narrow window.
    ui.add_space(8.0);
    ui.label("Format");
    for (i, (label, about)) in NEW_DISK_FORMATS.iter().enumerate() {
        ui.radio_value(fmt, i, format!("{label}  {about}"));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Create").clicked() {
            *create = true;
        }
        if ui.button("Cancel").clicked() {
            *close = true;
        }
    });
}

pub(crate) fn unsaved_message(which: &[&str]) -> String {
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
