//! Robustness fuzzing: no parser may panic on arbitrary or corrupted input.
//!
//! This is a dependency-free, deterministic fuzzer that runs as part of
//! `cargo test`. It throws random byte buffers — including ones at the exact
//! sizes that pass image detection, and valid disks with random bytes flipped —
//! at every parser and reader, and asserts none of them panic (a panic aborts
//! the closure, which the test catches and reports with the failing seed).
//!
//! For coverage-guided fuzzing with libFuzzer, see `fuzz/` (needs nightly).

use std::panic;

use sector64::{parse_arc, parse_lbr, parse_lnx, parse_t64, parse_zipcode, Disk, FileType};

/// Tiny deterministic PRNG (SplitMix64-ish) so failures are reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn fill(&mut self, buf: &mut [u8]) {
        for b in buf {
            *b = self.next() as u8;
        }
    }
    fn range(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Image sizes that pass `Disk::load`'s detection (so the content, not the
/// length, is what gets fuzzed).
const VALID_SIZES: [usize; 8] =
    [174848, 175531, 196608, 197376, 349696, 351062, 819200, 822400];

/// Run every read path on an image. Results are discarded; the point is that
/// nothing panics.
fn exercise(bytes: Vec<u8>) {
    let Ok(disk) = Disk::load(bytes) else {
        return;
    };
    let _ = disk.header();
    let _ = disk.blocks_free();
    let _ = disk.validate();
    if let Ok(dir) = disk.directory() {
        for entry in dir.iter().take(48) {
            let _ = disk.read_entry(entry);
            let _ = disk.file_block_chain(entry.first_track, entry.first_sector);
            let _ = disk.side_sector_chain(entry);
            let _ = disk.read_rel(entry);
        }
    }
}

/// A valid disk of one of the three families, with some files written.
fn base_disk(rng: &mut Rng) -> Vec<u8> {
    let mut disk = match rng.range(3) {
        0 => Disk::format("fuzz", "fz"),
        1 => Disk::format_d71("fuzz", "fz"),
        _ => Disk::format_d81("fuzz", "fz"),
    };
    for _ in 0..rng.range(4) {
        let len = rng.range(2000);
        let mut data = vec![0u8; len];
        rng.fill(&mut data);
        let ty = [FileType::Prg, FileType::Seq, FileType::Usr][rng.range(3)];
        let name = format!("f{}", rng.next() % 1000);
        let _ = disk.write_file(&name, ty, &data);
    }
    disk.as_bytes().to_vec()
}

#[test]
fn no_panics_on_arbitrary_input() {
    // Silence the panic hook so a (hypothetical) failure doesn't flood output;
    // catch_unwind still reports it.
    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));

    let result = panic::catch_unwind(|| {
        let mut rng = Rng(0xD64_F0FF_1234_5678);
        for _ in 0..1500 {
            // 1. A valid-size image full of random bytes.
            let size = VALID_SIZES[rng.range(VALID_SIZES.len())];
            let mut img = vec![0u8; size];
            rng.fill(&mut img);
            exercise(img);

            // 2. A real disk with a handful of random bytes corrupted.
            let mut disk = base_disk(&mut rng);
            for _ in 0..rng.range(96) {
                let i = rng.range(disk.len());
                disk[i] = rng.next() as u8;
            }
            exercise(disk);

            // 3. Random buffers of arbitrary (usually wrong) length.
            let n = rng.range(4096);
            let mut buf = vec![0u8; n];
            rng.fill(&mut buf);
            let _ = Disk::load(buf.clone());
            let _ = parse_t64(&buf);
            let _ = parse_lnx(&buf);
            let _ = parse_lbr(&buf);
            let _ = sector64::parse_p00(&buf);
            let _ = sector64::parse_cvt(&buf);
            let _ = parse_zipcode([&buf, &buf, &buf, &buf]);
            // Exercise the G64 GCR decoder with a valid magic so the decode
            // path runs on fuzzed track tables and bitstreams.
            let mut g64 = b"GCR-1541".to_vec();
            g64.extend_from_slice(&buf);
            let _ = Disk::load(g64.clone());
            let _ = sector64::g64_to_d64(&g64);
            if let Ok(entries) = parse_arc(&buf) {
                for e in entries {
                    let _ = e.decode();
                }
            }
            let _ = sector64::parse_sda(&buf);
            if let Ok(entries) = sector64::parse_lha(&buf) {
                for e in entries {
                    let _ = e.decode();
                }
            }
        }
    });

    panic::set_hook(prev);
    assert!(result.is_ok(), "a parser panicked on fuzzed input");
}
