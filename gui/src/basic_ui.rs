//! The BASIC editor window and its state.

use crate::*;

/// What the BASIC editor's two load buttons read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BasicSource {
    File,
    Selected,
}

/// The BASIC editor: a program's text, and where it's to go.
pub(crate) struct BasicEditor {
    pub(crate) text: String,
    pub(crate) name: String,
    pub(crate) dialect: BasicDialect,
    /// Where the program loads, when that isn't the dialect's usual address.
    pub(crate) load: Option<u16>,
    /// The text has changed since it was loaded, saved or added.
    pub(crate) modified: bool,
    /// A load button was pressed while the text was modified: press it again
    /// to lose the changes.
    pub(crate) confirm: Option<BasicSource>,
    /// One of its fields had the keyboard last frame (Esc takes it away
    /// before this frame's widgets can tell).
    pub(crate) typing: bool,
    /// The text tokenized as it stands; `None` when it needs doing.
    pub(crate) checked: Option<Result<Tokenized, Vec<TextProblem>>>,
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
    pub(crate) fn check(&mut self) -> &Result<Tokenized, Vec<TextProblem>> {
        self.checked
            .get_or_insert_with(|| tokenize(&self.text, self.dialect, self.load))
    }

    /// Start again with other text, unmodified.
    pub(crate) fn set_text(&mut self, text: String, load: Option<u16>) {
        self.text = text;
        self.load = load;
        self.modified = false;
        self.confirm = None;
        self.checked = None;
    }

    /// The program's name as a disk keeps it.
    pub(crate) fn disk_name(&self) -> String {
        self.name.trim().to_uppercase().chars().take(16).collect()
    }
}

/// Put a BASIC program on `disk`, replacing the program of that name if
/// there is one. Only an unlocked, plain BASIC PRG is replaced. Returns
/// whether it was; on an error the disk is unchanged.
pub(crate) fn put_basic(disk: &mut Disk, name: &str, prg: &[u8]) -> Result<bool, String> {
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

/// The BASIC dialects, with a button label and a description each.
pub(crate) const BASIC_DIALECTS: [(BasicDialect, &str, &str); 3] = [
    (BasicDialect::V2, "V2", "Commodore BASIC V2 (C64, VIC-20)"),
    (BasicDialect::V7, "7.0", "the C128's BASIC 7.0"),
    (BasicDialect::Simons, "SIMONS", "Simons' BASIC (C64)"),
];

/// Where the BASIC editor's program would go.
pub(crate) struct BasicTarget {
    pub(crate) editable: bool,
    pub(crate) is_archive: bool,
    /// There's a file of that name already.
    pub(crate) exists: bool,
    /// A file is selected in the list.
    pub(crate) selected: bool,
}

/// The BASIC editor's contents: name, dialect, the text, what's wrong with
/// it, and the buttons.
pub(crate) fn basic_editor_ui(
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
pub(crate) fn basic_check_ui(ui: &mut egui::Ui, ed: &mut BasicEditor) {
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
