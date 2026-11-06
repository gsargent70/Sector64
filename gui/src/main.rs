//! Sector64 GUI — a native browser for Commodore disk images and archives.
//!
//! Open a disk image (`.d64/.d71/.d81/.g64/.d2m`) or an archive (`.arc/.sda/
//! .lha/.lzh/.sfx/.lbr/.t64/.lnx/.p00/.wr3`); it lists the contents and
//! extracts files to disk, reusing the `sector64` engine. The engine stays
//! dependency-free; this front-end is a separate workspace member.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use eframe::egui;
use sector64::{
    parse_arc, parse_lbr, parse_lha, parse_lnx, parse_p00, parse_sda, parse_t64, parse_wr3, Disk,
};

/// One listed item: its name, a short detail line, and either its decoded bytes
/// or a reason it can't be extracted.
struct Node {
    name: String,
    detail: String,
    data: Result<Vec<u8>, String>,
}

#[derive(Default)]
struct App {
    path_input: String,
    title: String,
    nodes: Vec<Node>,
    selected: Option<usize>,
    out_dir: String,
    status: String,
}

impl App {
    fn open(&mut self, path: &Path) {
        self.title.clear();
        self.nodes.clear();
        self.selected = None;
        match open_path(path) {
            Ok((title, nodes)) => {
                self.title = title;
                self.nodes = nodes;
                self.path_input = path.display().to_string();
                if self.out_dir.is_empty() {
                    if let Some(parent) = path.parent() {
                        self.out_dir = parent.display().to_string();
                    }
                }
                self.status = format!("Opened {} ({} item(s))", path.display(), self.nodes.len());
            }
            Err(e) => self.status = format!("Error: {e}"),
        }
    }

    fn extract_selected(&mut self) {
        let Some(i) = self.selected else {
            self.status = "Select a file first".into();
            return;
        };
        let node = &self.nodes[i];
        let data = match &node.data {
            Ok(d) => d.clone(),
            Err(e) => {
                self.status = format!("Cannot extract \"{}\": {e}", node.name);
                return;
            }
        };
        let dir = if self.out_dir.is_empty() { ".".into() } else { self.out_dir.clone() };
        let fname = sanitize(&node.name);
        let dest = PathBuf::from(&dir).join(&fname);
        match std::fs::write(&dest, &data) {
            Ok(()) => self.status = format!("Wrote {} ({} bytes)", dest.display(), data.len()),
            Err(e) => self.status = format!("Write failed: {e}"),
        }
    }

    fn extract_all(&mut self) {
        let dir = if self.out_dir.is_empty() { ".".into() } else { self.out_dir.clone() };
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
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📂 Open…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                        self.open(&p);
                    }
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text("path to a disk image or archive")
                        .desired_width(380.0),
                );
                if ui.button("Load").clicked() {
                    let p = PathBuf::from(self.path_input.trim());
                    self.open(&p);
                }
            });
            if !self.title.is_empty() {
                ui.label(egui::RichText::new(&self.title).strong());
            }
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Output dir:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.out_dir).desired_width(300.0),
                );
                if ui.button("…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        self.out_dir = d.display().to_string();
                    }
                }
                if ui.button("Extract selected").clicked() {
                    self.extract_selected();
                }
                if ui.button("Extract all").clicked() {
                    self.extract_all();
                }
            });
            ui.label(&self.status);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.nodes.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a disk image or archive to browse its contents.");
                });
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("files")
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("name").strong());
                        ui.label(egui::RichText::new("details").strong());
                        ui.label(egui::RichText::new("status").strong());
                        ui.end_row();
                        for i in 0..self.nodes.len() {
                            let selected = self.selected == Some(i);
                            let name = self.nodes[i].name.clone();
                            if ui.selectable_label(selected, &name).clicked() {
                                self.selected = Some(i);
                            }
                            ui.label(&self.nodes[i].detail);
                            match &self.nodes[i].data {
                                Ok(d) => ui.label(format!("{} bytes", d.len())),
                                Err(e) => ui.label(egui::RichText::new(e).weak()),
                            };
                            ui.end_row();
                        }
                    });
            });
        });
    }
}

/// Replace characters unsafe in a host file name.
fn sanitize(name: &str) -> String {
    let n: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || " .-_".contains(c) { c } else { '_' })
        .collect();
    let n = n.trim();
    if n.is_empty() {
        "file".into()
    } else {
        n.to_string()
    }
}

/// Open a path as a disk image or archive and return (title, items).
fn open_path(path: &Path) -> Result<(String, Vec<Node>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    // Disk image (D64/D71/D81/G64/D2M — detected by Disk::load).
    if let Ok(disk) = Disk::load(bytes.clone()) {
        let header = disk.header().map_err(|e| e.to_string())?;
        let entries = disk.directory().map_err(|e| e.to_string())?;
        let mut nodes = Vec::new();
        for e in &entries {
            let (splat, lock) = e.flags();
            let data = disk.read_entry(e).map_err(|x| x.to_string());
            nodes.push(Node {
                name: e.name.clone(),
                detail: format!("{} blk  {}{}{}", e.blocks, splat, e.file_type.mnemonic(), lock),
                data,
            });
        }
        let free = disk.blocks_free().map(|f| format!("{f} free")).unwrap_or_default();
        let title = format!(
            "Disk  0 \"{}\" {} {}   {}",
            header.name, header.id, header.dos_type, free
        );
        return Ok((title, nodes));
    }

    // Otherwise an archive, dispatched by extension.
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let import_nodes = |files: Vec<sector64::ImportFile>| {
        files
            .into_iter()
            .map(|f| Node {
                detail: format!("{} bytes  {}", f.data.len(), f.file_type.mnemonic()),
                name: f.name,
                data: Ok(f.data),
            })
            .collect::<Vec<_>>()
    };
    let is_p00 = {
        let b = ext.as_bytes();
        b.len() == 3 && matches!(b[0], b'p' | b's' | b'u' | b'r' | b'd') && b[1].is_ascii_digit() && b[2].is_ascii_digit()
    };

    let (kind, nodes) = match ext.as_str() {
        "arc" | "sda" => {
            let parse = if bytes.starts_with(&[0x01, 0x08]) { parse_sda } else { parse_arc };
            let entries = parse(&bytes).map_err(|e| e.to_string())?;
            let nodes = entries
                .into_iter()
                .map(|e| Node {
                    detail: format!("{} bytes  {}", e.orig_size, e.method.label()),
                    data: e.decode().map_err(|x| x.to_string()),
                    name: e.name,
                })
                .collect();
            ("ARC/SDA archive", nodes)
        }
        "lha" | "lzh" | "sfx" => {
            let entries = parse_lha(&bytes).map_err(|e| e.to_string())?;
            let nodes = entries
                .into_iter()
                .map(|e| Node {
                    detail: format!("{} → {} bytes  {}", e.compressed_size, e.original_size, e.method_str()),
                    data: e.decode().map_err(|x| x.to_string()),
                    name: e.name,
                })
                .collect();
            ("LHA/SFX archive", nodes)
        }
        "lbr" => ("LBR library", import_nodes(parse_lbr(&bytes).map_err(|e| e.to_string())?)),
        "t64" => ("T64 tape archive", import_nodes(parse_t64(&bytes).map_err(|e| e.to_string())?)),
        "lnx" => ("Lynx archive", import_nodes(parse_lnx(&bytes).map_err(|e| e.to_string())?)),
        "wr3" => {
            let entries = parse_wr3(&bytes).map_err(|e| e.to_string())?;
            let nodes = entries
                .into_iter()
                .map(|e| Node {
                    detail: format!("{} bytes packed", e.compressed.len()),
                    data: Err("Wraptor compression not decoded".to_string()),
                    name: e.name,
                })
                .collect();
            ("Wraptor-3 archive", nodes)
        }
        _ if is_p00 => {
            let f = parse_p00(&bytes).map_err(|e| e.to_string())?;
            ("PC64 container", import_nodes(vec![f]))
        }
        other => return Err(format!("unrecognized file type (.{other})")),
    };
    Ok((kind.to_string(), nodes))
}

fn main() -> eframe::Result<()> {
    let initial = std::env::args().nth(1);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([760.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Sector64",
        options,
        Box::new(move |_cc| {
            let mut app = App::default();
            if let Some(p) = initial {
                app.open(Path::new(&p));
            }
            Ok(Box::new(app))
        }),
    )
}
