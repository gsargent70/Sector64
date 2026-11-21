//! C64 bitmap pictures: the common paint-program formats decoded to a
//! 320×200 image of C64 colour numbers (0–15), so a viewer can apply its own
//! palette.
//!
//! All of them store the VIC-II's own data: an 8000-byte bitmap (40×25 cells
//! of 8 bytes, each byte one row of 8 pixels) and a 1000-byte screen whose
//! two nibbles colour each cell. Hires pictures have one bit per pixel: a set
//! bit takes the screen byte's high nibble, a clear one its low nibble.
//! Multicolour pictures read bits in pairs, each pair a double-wide pixel:
//! `00` the background, `01` the screen's high nibble, `10` its low nibble
//! and `11` the cell's colour-RAM nibble.

/// Picture width in pixels.
pub const WIDTH: usize = 320;
/// Picture height in pixels.
pub const HEIGHT: usize = 200;

const BITMAP: usize = 8000;
const SCREEN: usize = 1000;
/// How far past its usual size a file may run (padding some programs add)
/// and still be recognised.
const SLACK: usize = 256;

/// A paint program's file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PicFormat {
    /// Koala Painter (multicolour), loads at $6000.
    Koala,
    /// OCP Art Studio (hires), loads at $2000.
    ArtStudio,
    /// OCP Advanced Art Studio (multicolour), loads at $2000.
    AdvancedArtStudio,
    /// Doodle! (hires), loads at $5C00.
    Doodle,
}

/// Where each part sits, as offsets after the load address.
struct Layout {
    bitmap: usize,
    screen: usize,
    colour: Option<usize>,
    background: Option<usize>,
    border: Option<usize>,
    /// Bytes the file must have after its load address.
    min_len: usize,
}

impl PicFormat {
    /// Every format, in the order [`detect`] tries them.
    pub const ALL: [PicFormat; 4] = [
        PicFormat::Koala,
        PicFormat::AdvancedArtStudio,
        PicFormat::ArtStudio,
        PicFormat::Doodle,
    ];

    /// The program's name.
    pub fn name(self) -> &'static str {
        match self {
            PicFormat::Koala => "Koala Painter",
            PicFormat::ArtStudio => "Art Studio",
            PicFormat::AdvancedArtStudio => "Advanced Art Studio",
            PicFormat::Doodle => "Doodle!",
        }
    }

    /// Where its files load.
    pub fn load_address(self) -> u16 {
        match self {
            PicFormat::Koala => 0x6000,
            PicFormat::ArtStudio | PicFormat::AdvancedArtStudio => 0x2000,
            PicFormat::Doodle => 0x5C00,
        }
    }

    /// Whether its pictures are multicolour (else hires).
    pub fn multicolor(self) -> bool {
        matches!(self, PicFormat::Koala | PicFormat::AdvancedArtStudio)
    }

    fn layout(self) -> Layout {
        match self {
            // Bitmap $6000, screen $7F40, colour $8328, background $8710.
            PicFormat::Koala => Layout {
                bitmap: 0,
                screen: BITMAP,
                colour: Some(BITMAP + SCREEN),
                background: Some(BITMAP + 2 * SCREEN),
                border: None,
                min_len: BITMAP + 2 * SCREEN + 1,
            },
            // Bitmap $2000, screen $3F40, border $4328.
            PicFormat::ArtStudio => Layout {
                bitmap: 0,
                screen: BITMAP,
                colour: None,
                background: None,
                border: Some(BITMAP + SCREEN),
                min_len: BITMAP + SCREEN,
            },
            // Bitmap $2000, screen $3F40, border $4328, background $4329,
            // colour $4338.
            PicFormat::AdvancedArtStudio => Layout {
                bitmap: 0,
                screen: BITMAP,
                colour: Some(BITMAP + SCREEN + 16),
                background: Some(BITMAP + SCREEN + 1),
                border: Some(BITMAP + SCREEN),
                min_len: BITMAP + 2 * SCREEN + 16,
            },
            // Screen $5C00 (padded to 1 KB), bitmap $6000.
            PicFormat::Doodle => Layout {
                bitmap: 1024,
                screen: 0,
                colour: None,
                background: None,
                border: None,
                min_len: 1024 + BITMAP,
            },
        }
    }
}

/// A decoded picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub format: PicFormat,
    /// `WIDTH * HEIGHT` C64 colour numbers (0–15), row by row.
    pub pixels: Vec<u8>,
    /// The border colour, for formats that save one.
    pub border: Option<u8>,
}

/// Which format a PRG (load address included) is in, judged by its load
/// address and length.
pub fn detect(prg: &[u8]) -> Option<PicFormat> {
    let load = u16::from_le_bytes([*prg.first()?, *prg.get(1)?]);
    let len = prg.len() - 2;
    PicFormat::ALL.into_iter().find(|f| {
        let min = f.layout().min_len;
        f.load_address() == load && (min..=min + SLACK).contains(&len)
    })
}

/// Decode a PRG as whichever format it's in. `None` if it isn't one.
pub fn decode(prg: &[u8]) -> Option<Picture> {
    decode_as(prg, detect(prg)?)
}

/// Decode a PRG as `format`, whatever its load address. `None` if it's too
/// short to hold that format's data.
pub fn decode_as(prg: &[u8], format: PicFormat) -> Option<Picture> {
    let l = format.layout();
    let data = prg.get(2..)?;
    if data.len() < l.min_len {
        return None;
    }
    let bitmap = &data[l.bitmap..l.bitmap + BITMAP];
    let screen = &data[l.screen..l.screen + SCREEN];
    let colour = l.colour.map(|o| &data[o..o + SCREEN]);
    let background = l.background.map_or(0, |o| data[o] & 15);
    let mut pixels = vec![0u8; WIDTH * HEIGHT];
    for cell in 0..SCREEN {
        let (cx, cy) = (cell % 40, cell / 40);
        let hi = screen[cell] >> 4;
        let lo = screen[cell] & 15;
        let cram = colour.map_or(0, |c| c[cell] & 15);
        for row in 0..8 {
            let byte = bitmap[cell * 8 + row];
            let line = &mut pixels[(cy * 8 + row) * WIDTH + cx * 8..][..8];
            if format.multicolor() {
                for p in 0..4 {
                    let c = match (byte >> (6 - 2 * p)) & 3 {
                        0 => background,
                        1 => hi,
                        2 => lo,
                        _ => cram,
                    };
                    line[2 * p] = c;
                    line[2 * p + 1] = c;
                }
            } else {
                for (bit, px) in line.iter_mut().enumerate() {
                    *px = if byte & (0x80 >> bit) != 0 { hi } else { lo };
                }
            }
        }
    }
    Some(Picture {
        format,
        pixels,
        border: l.border.and_then(|o| data.get(o)).map(|b| b & 15),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(load: u16, len: usize) -> Vec<u8> {
        let mut f = load.to_le_bytes().to_vec();
        f.resize(2 + len, 0);
        f
    }

    #[test]
    fn formats_are_told_apart_by_load_address_and_length() {
        assert_eq!(detect(&file(0x6000, 10001)), Some(PicFormat::Koala));
        assert_eq!(detect(&file(0x6000, 10004)), Some(PicFormat::Koala));
        assert_eq!(detect(&file(0x2000, 9007)), Some(PicFormat::ArtStudio));
        assert_eq!(
            detect(&file(0x2000, 10016)),
            Some(PicFormat::AdvancedArtStudio)
        );
        assert_eq!(detect(&file(0x5C00, 9216)), Some(PicFormat::Doodle));
        // Too short, far too long, or loading elsewhere.
        assert_eq!(detect(&file(0x6000, 10000)), None);
        assert_eq!(detect(&file(0x6000, 30000)), None);
        assert_eq!(detect(&file(0x0801, 10001)), None);
        assert_eq!(detect(&[0x00]), None);
    }

    #[test]
    fn koala_pairs_pick_background_screen_and_colour_ram() {
        let mut f = file(0x6000, 10001);
        let d = &mut f[2..];
        d[0] = 0b00_01_10_11; // cell 0, row 0
        d[8000] = 0x52; // screen: high 5, low 2
        d[9000] = 0xF7; // colour RAM: 7 (high nibble ignored)
        d[10000] = 0x16; // background: 6
        let p = decode(&f).unwrap();
        assert_eq!(&p.pixels[..8], &[6, 6, 5, 5, 2, 2, 7, 7]);
        assert_eq!(p.border, None);
        // Cell 41 is the second cell of the second cell row.
        let mut f = file(0x6000, 10001);
        f[2 + 41 * 8 + 3] = 0b01_00_00_00;
        f[2 + 8000 + 41] = 0xE0;
        let p = decode(&f).unwrap();
        assert_eq!(p.pixels[(8 + 3) * WIDTH + 8], 14);
        assert_eq!(p.pixels[(8 + 3) * WIDTH + 10], 0);
    }

    #[test]
    fn hires_bits_pick_the_screen_nibbles() {
        // Doodle: screen first, bitmap at +1024.
        let mut f = file(0x5C00, 9216);
        f[2] = 0x1B; // set bits white (1), clear bits light grey (11)
        f[2 + 1024 + 7] = 0b1000_0001; // cell 0, row 7
        let p = decode(&f).unwrap();
        let row = &p.pixels[7 * WIDTH..7 * WIDTH + 8];
        assert_eq!(row, &[1, 11, 11, 11, 11, 11, 11, 1]);
        // Art Studio: bitmap first; the border byte follows the screen.
        let mut f = file(0x2000, 9009);
        f[2 + 9000] = 0x0E;
        assert_eq!(decode(&f).unwrap().border, Some(14));
        // Advanced Art Studio's background sits after the border.
        let mut f = file(0x2000, 10016);
        f[2 + 9001] = 3;
        let p = decode(&f).unwrap();
        assert_eq!((p.pixels[0], p.format.multicolor()), (3, true));
    }

    #[test]
    fn any_file_can_be_forced_into_a_format_if_long_enough() {
        let f = file(0x4000, 10001);
        assert_eq!(decode(&f), None);
        assert!(decode_as(&f, PicFormat::Koala).is_some());
        assert_eq!(decode_as(&f[..100], PicFormat::Koala), None);
    }
}
