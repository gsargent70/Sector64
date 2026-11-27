//! The preview pane: hex, text, BASIC, disassembly, graphics and pictures.

use crate::*;

/// Columns of PETSCII text the preview shows per line: a C64 screen's 40.
pub(crate) const TEXT_PREVIEW_COLS: usize = 40;

/// The preview pane's modes: what each toggle is called and does.
pub(crate) const PREVIEW_MODES: [(PreviewMode, &str, &str); 6] = [
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
pub(crate) fn preview_toggles_width(ui: &egui::Ui) -> f32 {
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
pub(crate) fn preview_width(
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
pub(crate) const PREVIEW_CAP: usize = 16 * 1024;

/// The hex preview with bytes `from..from + len` highlighted, in both the
/// hex and the ASCII part (`text` is [`hexdump`]'s output).
pub(crate) fn hex_marked_layout(
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
pub(crate) fn hexdump(data: &[u8], cap: usize) -> String {
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
pub(crate) fn petscii_text(data: &[u8], cap: usize) -> String {
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
pub(crate) fn gfx_color(role: u8) -> Color32 {
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
pub(crate) fn gfx_sheet(
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
pub(crate) fn gfx_cell_info(sheet: &GfxSheet, load: Option<u16>, i: usize) -> String {
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
pub(crate) fn gfx_ui(
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
pub(crate) const C64_PALETTE: [(u8, u8, u8, &str); 16] = [
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
pub(crate) fn c64_color(c: u8) -> Color32 {
    let (r, g, b, _) = C64_PALETTE[(c & 15) as usize];
    Color32::from_rgb(r, g, b)
}

/// Why a file didn't decode as a picture, for the PIC preview.
pub(crate) fn pic_failure(data: &[u8], format: Option<PicFormat>) -> String {
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
pub(crate) fn pic_ui(
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
pub(crate) fn basic_preview(data: &[u8], dialect: Option<BasicDialect>) -> String {
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
pub(crate) const DIS_MAX_LINES: usize = 8192;

/// The DIS preview: a note on where the listing starts, then one 6502
/// instruction per line. A PRG starts at the address a BASIC `SYS` line jumps
/// to, or else at its load address; any other file has no load address and
/// is listed from $0000.
pub(crate) fn dis_preview(data: &[u8], is_prg: bool) -> String {
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
pub(crate) fn dis_layout(
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) -> egui::text::LayoutJob {
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
pub(crate) fn basic_layout(
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
