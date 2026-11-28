//! SID music files: the PSID and RSID headers that the High Voltage SID
//! Collection's tunes carry, versions 1 to 4.
//!
//! Written from the published file format: a big-endian header (magic,
//! version, data offset, load/init/play addresses, song count, start song,
//! a speed bit per song, and the title, author and release as 32-byte
//! Latin-1 strings), then for version 2 and later a flags word (clock,
//! SID model) and, from version 3, the addresses of a second and third SID.
//! After the header comes the C64 data, which carries its own two-byte load
//! address when the header's is 0.

use crate::error::{Error, Result};

/// PSID (a tune any player can run) or RSID (one that needs a real C64
/// environment: it sets up its own interrupts, or is a BASIC program).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidKind {
    Psid,
    Rsid,
}

/// The video standard the tune was written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    Unknown,
    Pal,
    Ntsc,
    Both,
}

/// The SID chip the tune was written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidModel {
    Unknown,
    Mos6581,
    Mos8580,
    Both,
}

impl SidModel {
    fn from_bits(b: u16) -> SidModel {
        match b & 3 {
            1 => SidModel::Mos6581,
            2 => SidModel::Mos8580,
            3 => SidModel::Both,
            _ => SidModel::Unknown,
        }
    }

    /// How a listing names it.
    pub fn name(self) -> &'static str {
        match self {
            SidModel::Unknown => "not given",
            SidModel::Mos6581 => "6581",
            SidModel::Mos8580 => "8580",
            SidModel::Both => "6581 or 8580",
        }
    }
}

/// What a SID file's header says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidInfo {
    pub kind: SidKind,
    pub version: u16,
    /// Where the C64 data starts in the file.
    pub data_offset: usize,
    /// Where the data loads in C64 memory (from the header, or else the
    /// data's own first two bytes).
    pub load: u16,
    /// How many bytes load there.
    pub load_len: usize,
    /// The routine that sets up a song (the song number in A); the load
    /// address when the header says 0.
    pub init: u16,
    /// The routine called to play the music, or 0 when the init routine
    /// installs its own interrupt.
    pub play: u16,
    pub songs: u16,
    /// The song to play first, from 1.
    pub start_song: u16,
    /// One bit per song (the 32nd's bit for the rest): 0 plays on the
    /// screen's vertical blank, 1 on a CIA timer.
    pub speed: u32,
    pub title: String,
    pub author: String,
    pub released: String,
    /// Version 2 and later: the data is Compute!'s Sidplayer MUS music, not
    /// code.
    pub mus: bool,
    /// Version 2 and later: PSID, needs PlaySID's extra samples; RSID, a
    /// C64 BASIC program.
    pub psid_specific_or_basic: bool,
    pub clock: Clock,
    /// The models of the first SID and of any second and third.
    pub models: Vec<SidModel>,
    /// The second and third SIDs' addresses (`$D420` and so on), if any.
    pub extra_sids: Vec<u16>,
}

impl SidInfo {
    /// Whether song `n` (from 1) plays on a CIA timer rather than the
    /// vertical blank.
    pub fn uses_cia(&self, n: u16) -> bool {
        let bit = u32::from(n.clamp(1, 32) - 1);
        self.speed >> bit & 1 == 1
    }

    /// The last address the data fills.
    pub fn load_end(&self) -> u16 {
        self.load
            .wrapping_add(self.load_len.saturating_sub(1).min(0xFFFF) as u16)
    }
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

/// A 32-byte Latin-1 text field, up to its first NUL.
fn text(field: &[u8]) -> String {
    field
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| char::from(b))
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Whether `bytes` start like a SID file.
pub fn is_sid(bytes: &[u8]) -> bool {
    matches!(bytes.get(..4), Some(b"PSID" | b"RSID"))
}

/// Read a SID file's header.
pub fn parse_sid(bytes: &[u8]) -> Result<SidInfo> {
    let kind = match bytes.get(..4) {
        Some(b"PSID") => SidKind::Psid,
        Some(b"RSID") => SidKind::Rsid,
        _ => return Err(Error::InvalidContainer),
    };
    if bytes.len() < 0x76 {
        return Err(Error::InvalidContainer);
    }
    let version = be16(bytes, 0x04);
    let data_offset = usize::from(be16(bytes, 0x06));
    let v2 = version >= 2;
    let header_len = if v2 { 0x7C } else { 0x76 };
    if !(1..=4).contains(&version) || data_offset < header_len || data_offset > bytes.len() {
        return Err(Error::InvalidContainer);
    }
    let data = &bytes[data_offset..];
    let (load, load_len) = match be16(bytes, 0x08) {
        0 if data.len() >= 2 => (u16::from_le_bytes([data[0], data[1]]), data.len() - 2),
        0 => return Err(Error::InvalidContainer),
        a => (a, data.len()),
    };
    let init = match be16(bytes, 0x0A) {
        0 => load,
        a => a,
    };
    let flags = if v2 { be16(bytes, 0x76) } else { 0 };
    let mut models = vec![SidModel::from_bits(flags >> 4)];
    let mut extra_sids = Vec::new();
    // A second (v3+) and third (v4) SID: an address byte $xy is $Dxy0
    // ($42 is $D420); 0 means none. Their models are flag bits 6-7 and 8-9.
    for (n, at) in [(3u16, 0x7A), (4, 0x7B)] {
        let page = if version >= n { bytes[at] } else { 0 };
        if page != 0 {
            extra_sids.push(0xD000 | u16::from(page) << 4);
            models.push(SidModel::from_bits(flags >> (2 * n)));
        }
    }
    Ok(SidInfo {
        kind,
        version,
        data_offset,
        load,
        load_len,
        init,
        play: be16(bytes, 0x0C),
        songs: be16(bytes, 0x0E),
        start_song: be16(bytes, 0x10),
        speed: u32::from_be_bytes([bytes[0x12], bytes[0x13], bytes[0x14], bytes[0x15]]),
        title: text(&bytes[0x16..0x36]),
        author: text(&bytes[0x36..0x56]),
        released: text(&bytes[0x56..0x76]),
        mus: flags & 1 != 0,
        psid_specific_or_basic: flags & 2 != 0,
        clock: match flags >> 2 & 3 {
            1 => Clock::Pal,
            2 => Clock::Ntsc,
            3 => Clock::Both,
            _ => Clock::Unknown,
        },
        models,
        extra_sids,
    })
}

/// A listing of the header, the way the preview shows it.
pub fn describe(info: &SidInfo) -> String {
    let kind = match info.kind {
        SidKind::Psid => "PSID",
        SidKind::Rsid => "RSID",
    };
    let shown = |s: &str| {
        if s.is_empty() {
            "(not given)".to_string()
        } else {
            s.to_string()
        }
    };
    let mut out = format!("{kind} v{} music file\n\n", info.version);
    out += &format!("Title     {}\n", shown(&info.title));
    out += &format!("Author    {}\n", shown(&info.author));
    out += &format!("Released  {}\n\n", shown(&info.released));
    let plural = if info.songs == 1 { "" } else { "s" };
    out += &format!(
        "Songs     {} song{plural}, starting with song {}\n",
        info.songs, info.start_song
    );
    out += &format!(
        "Load      ${:04X}-${:04X} ({} bytes)\n",
        info.load,
        info.load_end(),
        info.load_len
    );
    out += &format!("Init      ${:04X}\n", info.init);
    if info.play == 0 {
        out += "Play      none: the init routine sets up its own interrupt\n";
    } else {
        out += &format!("Play      ${:04X}\n", info.play);
    }
    let speed = if info.kind == SidKind::Rsid {
        "set by the tune itself".to_string()
    } else {
        let n = info.songs.clamp(1, 32);
        let cia: Vec<u16> = (1..=n).filter(|&s| info.uses_cia(s)).collect();
        match cia.len() {
            0 => "screen refresh (vertical blank) for every song".into(),
            c if c == usize::from(n) => "CIA timer for every song".into(),
            _ => {
                let list: Vec<String> = cia.iter().map(u16::to_string).collect();
                let songs = if cia.len() == 1 { "song" } else { "songs" };
                format!(
                    "CIA timer for {songs} {}, screen refresh for the rest",
                    list.join(", ")
                )
            }
        }
    };
    out += &format!("Speed     {speed}\n");
    if info.version >= 2 {
        let clock = match info.clock {
            Clock::Unknown => "not given",
            Clock::Pal => "PAL",
            Clock::Ntsc => "NTSC",
            Clock::Both => "PAL or NTSC",
        };
        out += &format!("Clock     {clock}\n");
        out += &format!("SID chip  {}\n", info.models[0].name());
        for (i, (addr, model)) in info.extra_sids.iter().zip(&info.models[1..]).enumerate() {
            let which = if i == 0 { "2nd SID" } else { "3rd SID" };
            out += &format!("{which:<9} ${addr:04X} ({})\n", model.name());
        }
        if info.mus {
            out += "\nThe data is Compute!'s Sidplayer music (MUS), not code.\n";
        }
        if info.psid_specific_or_basic {
            out += match info.kind {
                SidKind::Psid => "\nIt needs PlaySID's extra sample playback.\n",
                SidKind::Rsid => "\nIt's a C64 BASIC program, started with RUN.\n",
            };
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header built field by field, as the format describes it.
    pub(crate) fn header(kind: &[u8; 4], version: u16) -> Vec<u8> {
        let len = if version >= 2 { 0x7C } else { 0x76 };
        let mut h = vec![0u8; len];
        h[..4].copy_from_slice(kind);
        h[4..6].copy_from_slice(&version.to_be_bytes());
        h[6..8].copy_from_slice(&(len as u16).to_be_bytes());
        h
    }

    fn put(h: &mut [u8], at: usize, v: u16) {
        h[at..at + 2].copy_from_slice(&v.to_be_bytes());
    }

    fn put_text(h: &mut [u8], at: usize, s: &[u8]) {
        h[at..at + s.len()].copy_from_slice(s);
    }

    #[test]
    fn a_v2_psid_with_its_load_address_in_the_data() {
        let mut f = header(b"PSID", 2);
        put(&mut f, 0x0A, 0x1000); // init
        put(&mut f, 0x0C, 0x1003); // play
        put(&mut f, 0x0E, 3); // songs
        put(&mut f, 0x10, 2); // start song
        f[0x15] = 0b010; // song 2 on a CIA timer
        put_text(&mut f, 0x16, b"Test Tune");
        put_text(&mut f, 0x36, b"J\xF6rg Composer"); // Latin-1 o-umlaut
        put_text(&mut f, 0x56, b"2026 Nobody");
        put(&mut f, 0x76, 0b01_01_00); // PAL, 6581
        f.extend([0x00, 0x10]); // loads at $1000
        f.extend([0x60; 10]);
        let i = parse_sid(&f).unwrap();
        assert_eq!((i.kind, i.version), (SidKind::Psid, 2));
        assert_eq!((i.load, i.load_len, i.load_end()), (0x1000, 10, 0x1009));
        assert_eq!(
            (i.init, i.play, i.songs, i.start_song),
            (0x1000, 0x1003, 3, 2)
        );
        assert!(!i.uses_cia(1) && i.uses_cia(2) && !i.uses_cia(3));
        assert_eq!(i.author, "Jörg Composer");
        assert_eq!(
            (i.clock, i.models.clone()),
            (Clock::Pal, vec![SidModel::Mos6581])
        );
        let text = describe(&i);
        for want in [
            "PSID v2 music file",
            "Title     Test Tune",
            "Author    Jörg Composer",
            "Songs     3 songs, starting with song 2",
            "Load      $1000-$1009 (10 bytes)",
            "Play      $1003",
            "Speed     CIA timer for song 2, screen refresh for the rest",
            "Clock     PAL",
            "SID chip  6581",
        ] {
            assert!(text.contains(want), "{want:?} in:\n{text}");
        }
    }

    #[test]
    fn a_v1_header_with_a_load_address_and_no_flags() {
        let mut f = header(b"PSID", 1);
        put(&mut f, 0x08, 0xC000); // load
        put(&mut f, 0x0E, 1);
        put(&mut f, 0x10, 1);
        f.extend([0xEA; 4]);
        let i = parse_sid(&f).unwrap();
        assert_eq!(
            (i.load, i.load_len, i.init),
            (0xC000, 4, 0xC000),
            "init 0 is the load address"
        );
        let text = describe(&i);
        assert!(text.contains("Songs     1 song,"), "{text}");
        assert!(text.contains("Play      none: the init routine"), "{text}");
        assert!(!text.contains("Clock"), "v1 has no flags: {text}");
        assert!(text.contains("Title     (not given)"), "{text}");
    }

    #[test]
    fn rsid_v4_with_three_sids_and_basic() {
        let mut f = header(b"RSID", 4);
        put(&mut f, 0x0E, 1);
        put(&mut f, 0x10, 1);
        // BASIC, NTSC, 8580, 2nd 6581, 3rd 8580.
        put(&mut f, 0x76, 0b10_01_10_10_00 | 0b10);
        f[0x7A] = 0x42;
        f[0x7B] = 0x50;
        f.extend([0x01, 0x08, 0, 0]);
        let i = parse_sid(&f).unwrap();
        assert_eq!(i.extra_sids, [0xD420, 0xD500]);
        assert_eq!(
            i.models,
            [SidModel::Mos8580, SidModel::Mos6581, SidModel::Mos8580]
        );
        let text = describe(&i);
        for want in [
            "RSID v4",
            "Clock     NTSC",
            "2nd SID   $D420 (6581)",
            "3rd SID   $D500 (8580)",
            "Speed     set by the tune itself",
            "BASIC program",
        ] {
            assert!(text.contains(want), "{want:?} in:\n{text}");
        }
    }

    #[test]
    fn what_is_not_a_sid_file_is_refused() {
        assert!(parse_sid(b"PRG data").is_err());
        assert!(parse_sid(&header(b"PSID", 2)[..0x70]).is_err(), "too short");
        let mut bad = header(b"PSID", 9);
        bad.extend([0, 0x10]);
        assert!(parse_sid(&bad).is_err(), "unknown version");
        // Load address 0 but no data to take it from.
        assert!(parse_sid(&header(b"PSID", 2)).is_err());
        let mut f = header(b"PSID", 2);
        put(&mut f, 0x06, 0x200); // data offset past the end
        assert!(parse_sid(&f).is_err());
        assert!(is_sid(b"RSID....") && !is_sid(b"PSI"));
    }
}
