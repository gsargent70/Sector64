//! The sector editor (DISK-05): one 256-byte block of a disk in hex, with
//! edits staged until they're written back, and the block's T/S link to
//! follow a file or directory chain.
//!
//! This holds the editor's state and rules; `main.rs` draws it.

use sector64::Disk;

/// What a block's first two bytes say about the chain it's part of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// The next block of the chain.
    Next(u8, u8),
    /// The last block: bytes 2..=`last` are used.
    Last(u8),
    /// A link to a track/sector that isn't on this disk.
    Invalid(u8, u8),
}

/// The open block, the edits typed into it, and where it was reached from.
#[derive(Default)]
pub struct SectorEditor {
    pub track: u8,
    pub sector: u8,
    /// The block as it is on the disk.
    orig: Vec<u8>,
    /// The block with the staged edits.
    pub buf: Vec<u8>,
    /// The byte the cursor is on.
    pub cursor: usize,
    /// The high nibble of the cursor byte has been typed; the next digit
    /// fills the low one.
    pub low_nibble: bool,
    /// Blocks left by following links, newest last, for Back.
    history: Vec<(u8, u8)>,
    /// Why the block couldn't be read, if it couldn't.
    pub error: Option<String>,
    /// Which disk it shows (path and pane), to notice when that changes.
    pub source: String,
}

/// Bytes per block.
pub const SIZE: usize = 256;

impl SectorEditor {
    /// Show `track`/`sector` of `disk`, dropping any staged edits.
    pub fn load(&mut self, disk: &Disk, track: u8, sector: u8) {
        self.track = track;
        self.sector = sector;
        self.low_nibble = false;
        match disk.block(track, sector) {
            Ok(b) => {
                self.orig = b.to_vec();
                self.buf = b.to_vec();
                self.error = None;
            }
            Err(e) => {
                self.orig.clear();
                self.buf.clear();
                self.error = Some(e.to_string());
            }
        }
    }

    /// Open a block reached by a link or a jump, remembering this one for
    /// Back.
    pub fn jump(&mut self, disk: &Disk, track: u8, sector: u8) {
        if (track, sector) != (self.track, self.sector) {
            self.history.push((self.track, self.sector));
            if self.history.len() > 64 {
                self.history.remove(0);
            }
        }
        self.load(disk, track, sector);
    }

    /// Return to the block the last jump left.
    pub fn back(&mut self, disk: &Disk) -> bool {
        match self.history.pop() {
            Some((t, s)) => {
                self.load(disk, t, s);
                true
            }
            None => false,
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    /// Forget the Back history (a different disk is showing).
    pub fn forget(&mut self) {
        self.history.clear();
    }

    /// Whether `track`/`sector` exists on `disk`.
    pub fn exists(disk: &Disk, track: u8, sector: u8) -> bool {
        disk.geometry()
            .sector_count(track)
            .is_some_and(|n| u16::from(sector) < n)
    }

    /// The block `delta` blocks on in disk order (track by track), or `None`
    /// past either end.
    pub fn neighbour(disk: &Disk, track: u8, sector: u8, delta: i32) -> Option<(u8, u8)> {
        let g = disk.geometry();
        let (mut t, mut s) = (track, i32::from(sector) + delta);
        while s < 0 {
            t = t.checked_sub(1).filter(|&t| t >= 1)?;
            s += i32::from(g.sector_count(t)?);
        }
        loop {
            let n = i32::from(g.sector_count(t)?);
            if s < n {
                return Some((t, s as u8));
            }
            s -= n;
            t = t.checked_add(1).filter(|&t| t <= g.tracks())?;
        }
    }

    /// Step to the next (`1`) or previous (`-1`) block.
    pub fn step(&mut self, disk: &Disk, delta: i32) -> bool {
        match Self::neighbour(disk, self.track, self.sector, delta) {
            Some((t, s)) => {
                self.load(disk, t, s);
                true
            }
            None => false,
        }
    }

    /// The T/S link in the block's first two bytes (as edited).
    pub fn link(&self, disk: &Disk) -> Option<Link> {
        let (&t, &s) = (self.buf.first()?, self.buf.get(1)?);
        Some(if t == 0 {
            Link::Last(s)
        } else if Self::exists(disk, t, s) {
            Link::Next(t, s)
        } else {
            Link::Invalid(t, s)
        })
    }

    /// Follow the link to the next block of the chain.
    pub fn follow(&mut self, disk: &Disk) -> bool {
        match self.link(disk) {
            Some(Link::Next(t, s)) => {
                self.jump(disk, t, s);
                true
            }
            _ => false,
        }
    }

    /// Whether any byte differs from the disk.
    pub fn modified(&self) -> bool {
        self.buf != self.orig
    }

    /// Whether byte `i` has a staged edit.
    pub fn changed(&self, i: usize) -> bool {
        self.buf.get(i) != self.orig.get(i)
    }

    /// Drop the staged edits.
    pub fn revert(&mut self) {
        self.buf = self.orig.clone();
        self.low_nibble = false;
    }

    /// Re-read the block if nothing is staged, so the view follows the disk
    /// (after an Undo, say). Edits in progress are left alone.
    pub fn sync(&mut self, disk: &Disk) {
        if !self.modified() {
            let cursor = self.cursor;
            let low = self.low_nibble;
            self.load(disk, self.track, self.sector);
            self.cursor = cursor;
            self.low_nibble = low && self.error.is_none();
        }
    }

    /// Type one hex digit at the cursor: the high nibble, then the low one,
    /// then on to the next byte.
    pub fn type_digit(&mut self, digit: u8) {
        let Some(b) = self.buf.get_mut(self.cursor) else {
            return;
        };
        let d = digit & 0x0F;
        if self.low_nibble {
            *b = (*b & 0xF0) | d;
            self.low_nibble = false;
            self.cursor = (self.cursor + 1).min(SIZE - 1);
        } else {
            *b = (*b & 0x0F) | (d << 4);
            self.low_nibble = true;
        }
    }

    /// Move the cursor by `delta` bytes, staying in the block.
    pub fn move_cursor(&mut self, delta: i32) {
        let to = (self.cursor as i32 + delta).clamp(0, SIZE as i32 - 1);
        self.cursor = to as usize;
        self.low_nibble = false;
    }

    /// Put the cursor on byte `i`.
    pub fn place(&mut self, i: usize) {
        self.cursor = i.min(SIZE - 1);
        self.low_nibble = false;
    }

    /// Write the staged edits to `disk`. The caller records the undo step.
    pub fn write(&mut self, disk: &mut Disk) -> Result<(), String> {
        if self.buf.len() != SIZE {
            return Err("no block to write".into());
        }
        let block = disk
            .block_mut(self.track, self.sector)
            .map_err(|e| e.to_string())?;
        block.copy_from_slice(&self.buf);
        self.orig = self.buf.clone();
        self.low_nibble = false;
        Ok(())
    }
}

/// The text column's character for one byte: printable PETSCII as the C64
/// font draws it, shifted letters folded to their base glyph, `.` otherwise.
pub fn text_char(b: u8) -> char {
    match b {
        0x20..=0x7E => b as char,
        0xA0 => ' ',
        0xC1..=0xDA => (b - 0x80) as char,
        _ => '.',
    }
}

/// The hex value of a typed character, if it's a hex digit.
pub fn hex_digit(c: char) -> Option<u8> {
    c.to_digit(16).map(|d| d as u8)
}

/// A one-line description of a link, for the editor's status line.
pub fn describe(link: Link) -> String {
    match link {
        Link::Next(t, s) => format!("links to track {t}, sector {s}"),
        Link::Last(0) | Link::Last(1) => "last block of a chain (empty)".into(),
        Link::Last(n) => format!("last block of a chain: bytes 2–{n} used ({} bytes)", n - 1),
        Link::Invalid(t, s) => format!("links to track {t}, sector {s}, which isn't on this disk"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk() -> Disk {
        let mut d = Disk::format("TEST", "01");
        d.write_file("PROG", sector64::FileType::Prg, &vec![0xAB; 600])
            .unwrap();
        d
    }

    #[test]
    fn steps_across_tracks_and_stops_at_the_ends() {
        let d = disk();
        assert_eq!(SectorEditor::neighbour(&d, 1, 20, 1), Some((2, 0)));
        assert_eq!(SectorEditor::neighbour(&d, 2, 0, -1), Some((1, 20)));
        assert_eq!(SectorEditor::neighbour(&d, 18, 18, 1), Some((19, 0)));
        assert_eq!(SectorEditor::neighbour(&d, 1, 0, -1), None);
        assert_eq!(SectorEditor::neighbour(&d, 35, 16, 1), None);
        assert_eq!(SectorEditor::neighbour(&d, 1, 0, 21 + 21), Some((3, 0)));
        let d81 = Disk::format_d81("X", "01");
        assert_eq!(SectorEditor::neighbour(&d81, 1, 39, 1), Some((2, 0)));
        assert_eq!(SectorEditor::neighbour(&d81, 80, 39, 1), None);
    }

    #[test]
    fn follows_a_file_chain_and_goes_back() {
        let d = disk();
        let e = d.find_entry("PROG").unwrap().unwrap();
        let chain = d.file_block_chain(e.first_track, e.first_sector).unwrap();
        assert_eq!(chain.len(), 3);
        let mut ed = SectorEditor::default();
        ed.load(&d, chain[0].0, chain[0].1);
        assert_eq!(ed.link(&d), Some(Link::Next(chain[1].0, chain[1].1)));
        assert!(ed.follow(&d));
        assert!(ed.follow(&d));
        assert_eq!((ed.track, ed.sector), chain[2]);
        // 600 bytes = 254 + 254 + 92: the last block uses bytes 2..=93.
        assert_eq!(ed.link(&d), Some(Link::Last(93)));
        assert!(!ed.follow(&d));
        assert!(ed.back(&d));
        assert!(ed.back(&d));
        assert_eq!((ed.track, ed.sector), chain[0]);
        assert!(!ed.can_go_back());
    }

    #[test]
    fn typed_digits_stage_until_written() {
        let mut d = disk();
        let mut ed = SectorEditor::default();
        ed.load(&d, 1, 5);
        ed.place(10);
        for c in ['4', 'c', '0', '1'] {
            ed.type_digit(hex_digit(c).unwrap());
        }
        assert_eq!(&ed.buf[10..12], &[0x4C, 0x01]);
        assert_eq!(ed.cursor, 12);
        assert!(ed.modified() && ed.changed(10) && !ed.changed(9));
        // Not on the disk until written.
        assert_eq!(d.block(1, 5).unwrap()[10], 0);
        ed.write(&mut d).unwrap();
        assert_eq!(&d.block(1, 5).unwrap()[10..12], &[0x4C, 0x01]);
        assert!(!ed.modified());

        ed.type_digit(0xF);
        assert!(ed.modified());
        ed.revert();
        assert!(!ed.modified());
        assert_eq!(ed.cursor, 12);
    }

    #[test]
    fn sync_follows_the_disk_but_keeps_staged_edits() {
        let mut d = disk();
        let mut ed = SectorEditor::default();
        ed.load(&d, 2, 0);
        d.block_mut(2, 0).unwrap()[0] = 0x99;
        ed.sync(&d);
        assert_eq!(ed.buf[0], 0x99);
        ed.place(1);
        ed.type_digit(1);
        d.block_mut(2, 0).unwrap()[0] = 0x11;
        ed.sync(&d);
        assert_eq!(ed.buf[0], 0x99, "staged edits keep the block as it was");
    }

    #[test]
    fn bad_links_and_unreadable_blocks() {
        let d = disk();
        let mut ed = SectorEditor::default();
        ed.load(&d, 1, 0);
        ed.buf[0] = 1;
        ed.buf[1] = 21;
        assert_eq!(ed.link(&d), Some(Link::Invalid(1, 21)));
        ed.load(&d, 36, 0);
        assert!(ed.error.is_some() && ed.buf.is_empty());
        assert_eq!(ed.link(&d), None);
        ed.type_digit(1); // nothing to edit; no panic
        assert!(SectorEditor::exists(&d, 35, 16) && !SectorEditor::exists(&d, 35, 17));
    }

    #[test]
    fn cursor_stays_in_the_block() {
        let mut ed = SectorEditor::default();
        ed.move_cursor(-5);
        assert_eq!(ed.cursor, 0);
        ed.move_cursor(300);
        assert_eq!(ed.cursor, 255);
        ed.buf = vec![0; SIZE];
        ed.type_digit(1);
        ed.type_digit(2);
        assert_eq!((ed.cursor, ed.buf[255]), (255, 0x12));
    }

    #[test]
    fn text_column_and_link_wording() {
        assert_eq!(text_char(b'A'), 'A');
        assert_eq!(text_char(0xC1), 'A');
        assert_eq!(text_char(0xA0), ' ');
        assert_eq!(text_char(0x00), '.');
        assert_eq!(hex_digit('F'), Some(15));
        assert_eq!(hex_digit('g'), None);
        assert!(describe(Link::Last(93)).contains("92 bytes"));
    }
}
