//! The C64 look: palette, font, theme, icons, toasts and the small widgets the menus use.

use crate::*;

/// The Commodore 64 palette this UI is themed around (Pepto values).
pub(crate) mod c64 {
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
pub(crate) fn load_c64_font(ctx: &egui::Context) -> bool {
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
pub(crate) fn find_c64_font() -> Option<Vec<u8>> {
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
pub(crate) fn apply_c64_theme(ctx: &egui::Context) {
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
pub(crate) mod art {
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
pub(crate) struct Icons {
    pub(crate) folder: egui::TextureHandle,
    pub(crate) plus: egui::TextureHandle,
    pub(crate) floppy: egui::TextureHandle,
    pub(crate) down: egui::TextureHandle,
    pub(crate) trash: egui::TextureHandle,
    pub(crate) pencil: egui::TextureHandle,
    pub(crate) doc: egui::TextureHandle,
    /// A transparent icon, so menu items without one line up with those with.
    pub(crate) blank: egui::TextureHandle,
    /// The app's floppy-disk logo (the window icon), for the menu bar.
    pub(crate) logo: egui::TextureHandle,
}

impl Icons {
    pub(crate) fn load(ctx: &egui::Context) -> Self {
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
pub(crate) fn pixel_image(grid: &[&str; 8], ink: Color32) -> egui::ColorImage {
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
pub(crate) fn key_name(keys: &str) -> String {
    if cfg!(target_os = "macos") {
        keys.replace("Ctrl", "Cmd")
    } else {
        keys.to_string()
    }
}

/// A menu item's shortcut, set off from its name by a dash ("— Ctrl+K"). The
/// leading spaces keep a gap before the dash on the menu's widest item, and
/// the keys are padded to one width (the font is monospaced) so the dashes
/// line up even though shortcuts are right-aligned.
pub(crate) fn key_hint(keys: &str) -> String {
    format!("    — {:<6}", key_name(keys))
}

/// A 16×16-icon button carrying `label` text.
pub(crate) fn icon_button<'a>(tex: &egui::TextureHandle, label: &'a str) -> egui::Button<'a> {
    let img = egui::Image::new(egui::load::SizedTexture::new(
        tex.id(),
        egui::vec2(16.0, 16.0),
    ));
    egui::Button::image_and_text(img, label)
}

/// A menu item: `label` with an icon (`blank` for none, so names line up)
/// and, unless `keys` is empty, its shortcut.
pub(crate) fn entry<'a>(
    icon: &egui::TextureHandle,
    label: &'a str,
    keys: &str,
) -> egui::Button<'a> {
    let b = icon_button(icon, label);
    if keys.is_empty() {
        b
    } else {
        b.shortcut_text(key_hint(keys))
    }
}

/// Add a menu item, enabled or not, with a tooltip for each state (empty
/// for none). Returns whether it was clicked, closing the menu if so.
pub(crate) fn menu_entry(
    ui: &mut egui::Ui,
    enabled: bool,
    button: egui::Button<'_>,
    tip: &str,
    disabled_tip: &str,
) -> bool {
    let mut r = ui.add_enabled(enabled, button);
    if !tip.is_empty() {
        r = r.on_hover_text(tip);
    }
    if !disabled_tip.is_empty() {
        r = r.on_disabled_hover_text(disabled_tip);
    }
    let clicked = r.clicked();
    if clicked {
        ui.close_menu();
    }
    clicked
}

/// The accent colour for a file-type mnemonic badge.
pub(crate) fn badge_color(mnemonic: &str) -> Color32 {
    match mnemonic {
        "prg" => c64::PRG,
        "seq" => c64::SEQ,
        "usr" => c64::USR,
        "del" => c64::DEL,
        "rel" => c64::REL,
        _ => c64::TEXT_DIM,
    }
}

/// How long a toast stays fully visible before it starts to fade, and how long
/// the fade-out lasts (seconds).
pub(crate) const TOAST_TTL: f64 = 4.5;
pub(crate) const TOAST_FADE: f64 = 0.6;

/// Severity of a transient toast, which picks its border colour and tag.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ToastKind {
    Success,
    Error,
    Info,
}

impl ToastKind {
    /// Classify a status message into a toast severity by its wording.
    pub(crate) fn of(msg: &str) -> ToastKind {
        let l = msg.to_ascii_lowercase();
        if l.contains("fail")
            || l.contains("error")
            || l.contains("cannot")
            || l.contains("unknown")
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

    pub(crate) fn color(self) -> egui::Color32 {
        match self {
            ToastKind::Success => c64::OK,
            ToastKind::Error => c64::WARN,
            ToastKind::Info => c64::ACCENT,
        }
    }

    pub(crate) fn tag(self) -> &'static str {
        match self {
            ToastKind::Success => "OK",
            ToastKind::Error => "ERR",
            ToastKind::Info => "i",
        }
    }
}

/// A transient on-screen notification, stacked at the bottom-right.
pub(crate) struct Toast {
    pub(crate) kind: ToastKind,
    pub(crate) text: String,
    /// `ctx.input().time` at which this toast was created.
    pub(crate) born: f64,
}

/// A small procedurally-drawn 3.5" floppy icon (48×48 RGBA) for the window.
pub(crate) fn make_icon() -> egui::IconData {
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
