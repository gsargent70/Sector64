//! Minimal PETSCII-to-UTF-8 rendering for display of filenames and disk names.
//!
//! This is deliberately a small, legible approximation of the uppercase/
//! graphics PETSCII set — enough to print a readable directory. It is not a
//! full character-set emulation, and it uses none of the original program's
//! font or rendering code. Bytes it does not map are shown as `.`.

/// Trailing padding byte used in CBM filenames (shifted space).
pub const PAD: u8 = 0xA0;

/// Render a PETSCII byte as a UTF-8 `char` for display.
pub fn char_of(b: u8) -> char {
    match b {
        0x20..=0x3F => b as char,            // space, digits, punctuation
        0x40..=0x5A => b as char,            // @, A-Z (uppercase PETSCII)
        0x5B..=0x5F => b as char,            // [ \ ] ^ _
        0xC1..=0xDA => (b - 0x60) as char,   // shifted letters -> a-z
        _ => '.',                            // graphics and controls: placeholder
    }
}

/// Decode a fixed-width CBM field, stopping at the first `PAD` byte, into a
/// trimmed display string.
pub fn field_to_string(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == PAD).unwrap_or(field.len());
    field[..end].iter().map(|&b| char_of(b)).collect()
}

/// Encode one display character as an uppercase-PETSCII byte. Characters
/// outside the printable `$20..=$5F` range become `?`.
pub fn encode_char(c: char) -> u8 {
    let u = c.to_ascii_uppercase() as u32;
    if (0x20..=0x5F).contains(&u) {
        u as u8
    } else {
        b'?'
    }
}

/// Encode a name into a fixed-width CBM field, truncated to `width` and padded
/// with `PAD` (shifted space).
pub fn encode_field(name: &str, width: usize) -> Vec<u8> {
    let mut v: Vec<u8> = name.chars().take(width).map(encode_char).collect();
    v.resize(width, PAD);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_letters_and_padding() {
        // "TEST" padded with shifted spaces.
        let mut f = [PAD; 16];
        f[..4].copy_from_slice(b"TEST");
        assert_eq!(field_to_string(&f), "TEST");
    }

    #[test]
    fn shifted_letters_become_lowercase() {
        assert_eq!(char_of(0xC1), 'a');
        assert_eq!(char_of(0xDA), 'z');
    }
}
