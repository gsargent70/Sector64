//! The character viewer against the real C64 character ROM, which VICE
//! installs. Its first character set starts with `@`, `A`, `B`…, so drawing
//! the ROM must show their familiar shapes. Skipped when the ROM is absent.

use sector64::gfx::{render, FG};
use sector64::GfxKind;

fn chargen() -> Option<Vec<u8>> {
    let dirs = [
        "/usr/share/vice/C64",
        "/usr/lib/vice/C64",
        "/usr/local/share/vice/C64",
    ];
    dirs.iter()
        .find_map(|d| std::fs::read(format!("{d}/chargen-901225-01.bin")).ok())
        .or_else(|| {
            dirs.iter()
                .find_map(|d| std::fs::read(format!("{d}/chargen")).ok())
        })
        .filter(|r| r.len() == 4096)
}

#[test]
fn character_rom_draws_its_letters() {
    let Some(rom) = chargen() else {
        eprintln!("skipping: no C64 character ROM");
        return;
    };
    let sheet = render(&rom, GfxKind::Chars, false, 0, 16, usize::MAX);
    // Two sets of 256 characters, 16 to a row.
    assert_eq!((sheet.count, sheet.width, sheet.height), (512, 143, 287));
    let glyph = |i: usize| -> Vec<String> {
        let (x0, y0) = ((i % 16) * 9, (i / 16) * 9);
        (0..8)
            .map(|y| {
                (0..8)
                    .map(|x| {
                        if sheet.pixels[(y0 + y) * sheet.width + x0 + x] == FG {
                            '#'
                        } else {
                            '.'
                        }
                    })
                    .collect()
            })
            .collect()
    };
    assert_eq!(
        glyph(1),
        [
            "...##...", "..####..", ".##..##.", ".######.", ".##..##.", ".##..##.", ".##..##.",
            "........"
        ]
    );
    // Character 160 is the reverse space: solid.
    assert!(glyph(160).iter().all(|r| r == "########"));
    // Character 257 starts the second set: lower-case `a`.
    assert_eq!(glyph(257)[7], "........");
    assert_eq!(glyph(257)[2], "..####..");
}
