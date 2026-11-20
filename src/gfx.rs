//! Sprite and character-set graphics: a file's bytes drawn the way the
//! VIC-II would show them, as a sheet of 24×21 sprites or 8×8 characters.
//!
//! A sprite takes 63 bytes (21 rows of 3), padded to 64 so sprites sit on
//! 64-byte boundaries; a character takes 8 bytes (8 rows of 1). Each bit is a
//! pixel, most significant first. In multicolour mode bits are read in pairs,
//! each pair one double-width pixel.
//!
//! The sheet holds colour *roles*, not colours, so a viewer can pick its own
//! palette: [`BG`], [`MC1`], [`MC2`], [`FG`], and [`GAP`] for the 1-pixel
//! gutter between cells.

/// Background (bit pair `00`, or a clear hires bit).
pub const BG: u8 = 0;
/// First shared multicolour: `$D025` for sprites, `$D022` for characters
/// (bit pair `01`).
pub const MC1: u8 = 1;
/// Second shared multicolour: `$D026` for sprites (pair `11`), `$D023` for
/// characters (pair `10`).
pub const MC2: u8 = 2;
/// The cell's own colour: a set hires bit, the sprite colour (pair `10`) or a
/// character's colour-RAM colour (pair `11`).
pub const FG: u8 = 3;
/// The gutter between cells.
pub const GAP: u8 = 4;

/// What the bytes are drawn as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GfxKind {
    /// 24×21 sprites, 64 bytes apart.
    #[default]
    Sprites,
    /// 8×8 characters, 8 bytes apart.
    Chars,
}

impl GfxKind {
    /// Width and height of one cell, in pixels.
    pub fn cell_size(self) -> (usize, usize) {
        match self {
            GfxKind::Sprites => (24, 21),
            GfxKind::Chars => (8, 8),
        }
    }

    /// Bytes from one cell to the next.
    pub fn stride(self) -> usize {
        match self {
            GfxKind::Sprites => 64,
            GfxKind::Chars => 8,
        }
    }

    /// Bytes a cell actually uses (a sprite's 64th byte is padding).
    pub fn cell_bytes(self) -> usize {
        match self {
            GfxKind::Sprites => 63,
            GfxKind::Chars => 8,
        }
    }

    /// "sprite" / "char", for labels.
    pub fn noun(self) -> &'static str {
        match self {
            GfxKind::Sprites => "sprite",
            GfxKind::Chars => "char",
        }
    }
}

/// The file offset of the first whole cell. With a load address (a PRG's
/// first two bytes), cells line up with memory: the VIC-II fetches sprites
/// from 64-byte and characters from 8-byte boundaries, so the bytes before the
/// first boundary are skipped. Without one, cells start at offset 0.
pub fn first_cell(load: Option<u16>, kind: GfxKind) -> usize {
    match load {
        Some(addr) => {
            let s = kind.stride();
            2 + (s - addr as usize % s) % s
        }
        None => 0,
    }
}

/// How many whole cells `len` bytes hold from `start`. A sprite's padding
/// byte may be missing from the last one.
pub fn cell_count(len: usize, start: usize, kind: GfxKind) -> usize {
    let avail = len.saturating_sub(start);
    if avail < kind.cell_bytes() {
        return 0;
    }
    (avail - kind.cell_bytes()) / kind.stride() + 1
}

/// A rendered sheet of cells, `cols` to a row, separated by 1-pixel gutters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GfxSheet {
    pub kind: GfxKind,
    pub multicolor: bool,
    /// File offset of cell 0.
    pub start: usize,
    /// Cells drawn.
    pub count: usize,
    /// Cells per row.
    pub cols: usize,
    pub width: usize,
    pub height: usize,
    /// `width * height` colour roles ([`BG`] … [`GAP`]), row by row.
    pub pixels: Vec<u8>,
}

impl GfxSheet {
    /// The cell under pixel (`x`, `y`), or `None` for a gutter or empty slot.
    pub fn cell_at(&self, x: usize, y: usize) -> Option<usize> {
        let (cw, ch) = self.kind.cell_size();
        if x >= self.width || y >= self.height || x % (cw + 1) == cw || y % (ch + 1) == ch {
            return None;
        }
        let i = (y / (ch + 1)) * self.cols + x / (cw + 1);
        (i < self.count).then_some(i)
    }

    /// The file offset where cell `i` starts.
    pub fn offset_of(&self, i: usize) -> usize {
        self.start + i * self.kind.stride()
    }
}

/// Draw `data` from offset `start` as `kind` cells, `cols` to a row, at most
/// `max_cells` of them. Never panics, whatever the input.
pub fn render(
    data: &[u8],
    kind: GfxKind,
    multicolor: bool,
    start: usize,
    cols: usize,
    max_cells: usize,
) -> GfxSheet {
    let (cw, ch) = kind.cell_size();
    let count = cell_count(data.len(), start, kind).min(max_cells);
    let cols = cols.max(1).min(count.max(1));
    let rows = count.div_ceil(cols);
    let width = cols * (cw + 1) - 1;
    let height = (rows * (ch + 1)).saturating_sub(1);
    let mut pixels = vec![GAP; width * height];
    let bytes_per_row = cw / 8;
    for i in 0..count {
        let base = start + i * kind.stride();
        let (x0, y0) = ((i % cols) * (cw + 1), (i / cols) * (ch + 1));
        for row in 0..ch {
            let line = &mut pixels[(y0 + row) * width + x0..][..cw];
            for b in 0..bytes_per_row {
                let byte = data[base + row * bytes_per_row + b];
                let px = &mut line[b * 8..b * 8 + 8];
                if multicolor {
                    for p in 0..4 {
                        let role = match (byte >> (6 - 2 * p)) & 3 {
                            0 => BG,
                            1 => MC1,
                            2 if kind == GfxKind::Sprites => FG,
                            2 => MC2,
                            _ if kind == GfxKind::Sprites => MC2,
                            _ => FG,
                        };
                        px[2 * p] = role;
                        px[2 * p + 1] = role;
                    }
                } else {
                    for (bit, p) in px.iter_mut().enumerate() {
                        *p = if byte & (0x80 >> bit) != 0 { FG } else { BG };
                    }
                }
            }
        }
    }
    GfxSheet {
        kind,
        multicolor,
        start,
        count,
        cols,
        width,
        height,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One cell as rows of characters: `.` BG, `1` MC1, `2` MC2, `#` FG.
    fn cell(sheet: &GfxSheet, i: usize) -> Vec<String> {
        let (cw, ch) = sheet.kind.cell_size();
        let (x0, y0) = ((i % sheet.cols) * (cw + 1), (i / sheet.cols) * (ch + 1));
        (0..ch)
            .map(|y| {
                (0..cw)
                    .map(|x| match sheet.pixels[(y0 + y) * sheet.width + x0 + x] {
                        BG => '.',
                        MC1 => '1',
                        MC2 => '2',
                        FG => '#',
                        _ => '?',
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn hires_char_bits_read_msb_first() {
        let data = [0x80, 0x01, 0xff, 0, 0, 0, 0, 0x18];
        let s = render(&data, GfxKind::Chars, false, 0, 16, usize::MAX);
        assert_eq!((s.count, s.width, s.height), (1, 8, 8));
        let c = cell(&s, 0);
        assert_eq!(c[0], "#.......");
        assert_eq!(c[1], ".......#");
        assert_eq!(c[2], "########");
        assert_eq!(c[7], "...##...");
    }

    #[test]
    fn multicolour_pairs_map_to_roles_per_kind() {
        // 00 01 10 11
        let mut data = [0u8; 8];
        data[0] = 0b0001_1011;
        let c = render(&data, GfxKind::Chars, true, 0, 1, 1);
        assert_eq!(cell(&c, 0)[0], "..1122##");
        let mut spr = [0u8; 64];
        spr[0] = 0b0001_1011;
        let s = render(&spr, GfxKind::Sprites, true, 0, 1, 1);
        assert_eq!(&cell(&s, 0)[0][..8], "..11##22");
    }

    #[test]
    fn sprites_are_three_bytes_a_row_with_a_padding_byte() {
        // Two sprites; the second has its top-right pixel and bottom-left set.
        let mut data = vec![0u8; 64 + 63];
        data[64 + 2] = 0x01;
        data[64 + 60] = 0x80;
        let s = render(&data, GfxKind::Sprites, false, 0, 8, usize::MAX);
        assert_eq!((s.count, s.cols, s.width, s.height), (2, 2, 49, 21));
        let c = cell(&s, 1);
        assert!(c[0].ends_with(".#"));
        assert!(c[20].starts_with("#."));
        assert_eq!(s.pixels[24], GAP);
        assert_eq!(s.cell_at(24, 0), None);
        assert_eq!(s.cell_at(25, 0), Some(1));
        assert_eq!(s.offset_of(1), 64);
    }

    #[test]
    fn cells_line_up_with_memory_after_a_load_address() {
        assert_eq!(first_cell(None, GfxKind::Sprites), 0);
        assert_eq!(first_cell(Some(0x2000), GfxKind::Sprites), 2);
        assert_eq!(first_cell(Some(0x2001), GfxKind::Sprites), 65);
        assert_eq!(first_cell(Some(0x3ffe), GfxKind::Chars), 4);
        assert_eq!(cell_count(2 + 63, 2, GfxKind::Sprites), 1);
        assert_eq!(cell_count(2 + 62, 2, GfxKind::Sprites), 0);
        assert_eq!(cell_count(17, 0, GfxKind::Chars), 2);
        assert_eq!(cell_count(1, 5, GfxKind::Chars), 0);
    }

    #[test]
    fn rows_wrap_and_trailing_slots_are_empty() {
        let s = render(&[0xff; 8 * 5], GfxKind::Chars, false, 0, 4, usize::MAX);
        assert_eq!((s.cols, s.width, s.height), (4, 35, 17));
        assert_eq!(s.cell_at(0, 9), Some(4));
        assert_eq!(s.cell_at(9, 9), None);
        let capped = render(&[0xff; 8 * 5], GfxKind::Chars, false, 0, 4, 2);
        assert_eq!((capped.count, capped.cols, capped.height), (2, 2, 8));
        let empty = render(&[], GfxKind::Sprites, true, 0, 8, usize::MAX);
        assert_eq!((empty.count, empty.width, empty.height), (0, 24, 0));
    }
}
