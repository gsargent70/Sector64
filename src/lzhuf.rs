//! LZHUF decompression — the algorithm behind LHA's `-lh1-` method.
//!
//! LZHUF is Haruyasu Yoshizaki's 1988 combination of LZSS (a 4096-byte ring
//! buffer, match lengths 3..=60) with an adaptive Huffman code over the
//! literal/length alphabet, and an explicit-plus-table position code. This is a
//! faithful clean-room port of the public `lzhuf.c` decoder, with two
//! adaptations for use inside an LHA archive: the output length comes from the
//! archive header (not a 4-byte prefix in the stream), and the bitstream is
//! read from a byte slice rather than a file.
//!
//! The decoder makes bounded progress and never indexes out of range, so no
//! input can panic or hang: output is capped at the requested length, and once
//! the real input is exhausted the decode is allowed only a short grace before
//! stopping.

const N: usize = 4096; // ring buffer size
const F: usize = 60; // lookahead (max match)
const THRESHOLD: usize = 2;
const N_CHAR: usize = 256 - THRESHOLD + F; // 314
const T: usize = N_CHAR * 2 - 1; // 627
const R: usize = T - 1; // 626 (root)
const MAX_FREQ: u16 = 0x8000;

const D_CODE: [u8; 256] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,
    0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03,
    0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x09, 0x09, 0x09, 0x09, 0x09, 0x09, 0x09, 0x09,
    0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0B, 0x0B, 0x0B, 0x0B, 0x0B, 0x0B, 0x0B, 0x0B,
    0x0C, 0x0C, 0x0C, 0x0C, 0x0D, 0x0D, 0x0D, 0x0D, 0x0E, 0x0E, 0x0E, 0x0E, 0x0F, 0x0F, 0x0F, 0x0F,
    0x10, 0x10, 0x10, 0x10, 0x11, 0x11, 0x11, 0x11, 0x12, 0x12, 0x12, 0x12, 0x13, 0x13, 0x13, 0x13,
    0x14, 0x14, 0x14, 0x14, 0x15, 0x15, 0x15, 0x15, 0x16, 0x16, 0x16, 0x16, 0x17, 0x17, 0x17, 0x17,
    0x18, 0x18, 0x19, 0x19, 0x1A, 0x1A, 0x1B, 0x1B, 0x1C, 0x1C, 0x1D, 0x1D, 0x1E, 0x1E, 0x1F, 0x1F,
    0x20, 0x20, 0x21, 0x21, 0x22, 0x22, 0x23, 0x23, 0x24, 0x24, 0x25, 0x25, 0x26, 0x26, 0x27, 0x27,
    0x28, 0x28, 0x29, 0x29, 0x2A, 0x2A, 0x2B, 0x2B, 0x2C, 0x2C, 0x2D, 0x2D, 0x2E, 0x2E, 0x2F, 0x2F,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F,
];
const D_LEN: [u8; 256] = [
    0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03,
    0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03,
    0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
    0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
    0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
    0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
    0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
    0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
    0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
];

struct Lzhuf<'a> {
    input: &'a [u8],
    pos: usize,
    getbuf: u32,
    getlen: i32,
    /// Set once the reader has run past the real input.
    eof: bool,
    freq: [u16; T + 1],
    // Parent pointers; indices run up to T + N_CHAR - 1.
    prnt: [u16; T + N_CHAR],
    son: [u16; T],
}

impl<'a> Lzhuf<'a> {
    fn new(input: &'a [u8]) -> Self {
        let mut d = Lzhuf {
            input,
            pos: 0,
            getbuf: 0,
            getlen: 0,
            eof: false,
            freq: [0; T + 1],
            prnt: [0; T + N_CHAR],
            son: [0; T],
        };
        d.start_huff();
        d
    }

    fn start_huff(&mut self) {
        for i in 0..N_CHAR {
            self.freq[i] = 1;
            self.son[i] = (i + T) as u16;
            self.prnt[i + T] = i as u16;
        }
        let (mut i, mut j) = (0usize, N_CHAR);
        while j <= R {
            self.freq[j] = self.freq[i] + self.freq[i + 1];
            self.son[j] = i as u16;
            self.prnt[i] = j as u16;
            self.prnt[i + 1] = j as u16;
            i += 2;
            j += 1;
        }
        self.freq[T] = 0xffff;
        self.prnt[R] = 0;
    }

    /// Rebuild the tree, halving frequencies, when the root count saturates.
    fn reconst(&mut self) {
        let mut j = 0usize;
        for i in 0..T {
            if self.son[i] as usize >= T {
                self.freq[j] = self.freq[i].div_ceil(2);
                self.son[j] = self.son[i];
                j += 1;
            }
        }
        let mut i = 0usize;
        let mut j = N_CHAR;
        while j < T {
            let k = i + 1;
            let f = self.freq[i] + self.freq[k];
            self.freq[j] = f;
            // find insertion point so freq[] stays sorted
            let mut k = j - 1;
            while f < self.freq[k] {
                k -= 1;
            }
            let k = k + 1;
            // shift [k..j] up by one
            self.freq.copy_within(k..j, k + 1);
            self.freq[k] = f;
            self.son.copy_within(k..j, k + 1);
            self.son[k] = i as u16;
            i += 2;
            j += 1;
        }
        for i in 0..T {
            let k = self.son[i] as usize;
            if k >= T {
                self.prnt[k] = i as u16;
            } else {
                self.prnt[k] = i as u16;
                self.prnt[k + 1] = i as u16;
            }
        }
    }

    /// Increment the frequency of code `c`'s leaf, keeping the tree ordered.
    fn update(&mut self, c0: usize) {
        if self.freq[R] == MAX_FREQ {
            self.reconst();
        }
        let mut c = self.prnt[c0 + T] as usize;
        loop {
            self.freq[c] += 1;
            let k = self.freq[c];
            // if the order is disturbed, exchange nodes
            let mut l = c + 1;
            if k > self.freq[l] {
                while k > self.freq[l + 1] {
                    l += 1;
                }
                self.freq[c] = self.freq[l];
                self.freq[l] = k;

                let i = self.son[c] as usize;
                self.prnt[i] = l as u16;
                if i < T {
                    self.prnt[i + 1] = l as u16;
                }
                let j = self.son[l] as usize;
                self.son[l] = i as u16;
                self.prnt[j] = c as u16;
                if j < T {
                    self.prnt[j + 1] = c as u16;
                }
                self.son[c] = j as u16;
                c = l;
            }
            c = self.prnt[c] as usize;
            if c == 0 {
                break;
            }
        }
    }

    #[inline]
    fn fill(&mut self) {
        while self.getlen <= 8 {
            let i = if self.pos < self.input.len() {
                let b = self.input[self.pos];
                self.pos += 1;
                b as u32
            } else {
                self.eof = true;
                0
            };
            self.getbuf |= i << (8 - self.getlen);
            self.getlen += 8;
        }
    }

    fn get_bit(&mut self) -> u32 {
        self.fill();
        let i = self.getbuf;
        self.getbuf = (self.getbuf << 1) & 0xffff;
        self.getlen -= 1;
        (i & 0x8000) >> 15
    }

    fn get_byte(&mut self) -> u32 {
        self.fill();
        let i = self.getbuf;
        self.getbuf = (self.getbuf << 8) & 0xffff;
        self.getlen -= 8;
        (i & 0xff00) >> 8
    }

    fn decode_char(&mut self) -> usize {
        let mut c = self.son[R] as usize;
        // travel from root to leaf, taking the 0- or 1-child per read bit
        while c < T {
            c += self.get_bit() as usize;
            c = self.son[c] as usize;
        }
        c -= T;
        self.update(c);
        c
    }

    fn decode_position(&mut self) -> usize {
        let i0 = self.get_byte() as usize;
        let c = (D_CODE[i0] as usize) << 6;
        let mut j = D_LEN[i0] as usize;
        let mut i = i0;
        j -= 2;
        while j > 0 {
            i = (i << 1) + self.get_bit() as usize;
            j -= 1;
        }
        c | (i & 0x3f)
    }
}

/// Decode an LZHUF (`-lh1-`) stream of `out_len` original bytes.
pub fn decode(input: &[u8], out_len: usize) -> Vec<u8> {
    let mut d = Lzhuf::new(input);
    let mut text = [0u8; N];
    for b in text.iter_mut().take(N - F) {
        *b = 0x20;
    }
    let mut r = N - F;
    let mut out = Vec::with_capacity(out_len.min(64 * 1024));
    let mut grace = 0u32;

    while out.len() < out_len {
        // Bound runaway on truncated/garbage input: once the real bytes are
        // used up, allow only a few more codes (to finish the last one).
        if d.eof {
            grace += 1;
            if grace > 4 {
                break;
            }
        }
        let c = d.decode_char();
        if c < 256 {
            out.push(c as u8);
            text[r] = c as u8;
            r = (r + 1) & (N - 1);
        } else {
            let pos = d.decode_position();
            let i = (r + N - pos - 1) & (N - 1);
            let len = c - 255 + THRESHOLD;
            for k in 0..len {
                let ch = text[(i + k) & (N - 1)];
                out.push(ch);
                text[r] = ch;
                r = (r + 1) & (N - 1);
                if out.len() >= out_len {
                    break;
                }
            }
        }
    }
    out.truncate(out_len);
    out
}

/// CRC-16/ARC (reflected, polynomial 0xA001, init 0) — the checksum LHA stores
/// in each header for the original data.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_check_value() {
        // The standard CRC-16/ARC check value for "123456789".
        assert_eq!(crc16(b"123456789"), 0xBB3D);
    }

    #[test]
    fn decode_respects_length_and_never_panics() {
        // Garbage in must not panic or hang, and must honor the length cap.
        for len in [0usize, 1, 100, 5000] {
            let out = decode(&[0xAB, 0xCD, 0x12, 0x34, 0x56], len);
            assert!(out.len() <= len);
        }
        // Empty input with a nonzero length ends quickly (EOF grace), bounded.
        assert!(decode(&[], 1_000_000).len() < 1_000_000);
    }
}
