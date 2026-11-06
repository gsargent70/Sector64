//! TAP checks against a real datasette image.
//!
//! A TAP carries raw pulses, not files, so correctness is the header parse and
//! that the pulse decode tiles the declared data exactly. The sample is not
//! committed; point `SECTOR64_SAMPLES` at a directory holding it (or use
//! `~/Downloads/Sector64 Test Files`). Skips cleanly when absent.

use std::path::PathBuf;

use sector64::parse_tap;

fn samples_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SECTOR64_SAMPLES") {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join("Downloads/Sector64 Test Files");
    p.is_dir().then_some(p)
}

#[test]
fn azimuth_tap() {
    let Some(dir) = samples_dir() else {
        eprintln!("skipping: no samples directory");
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join("Azimuth 3000 (1984)(Interceptor).tap")) else {
        eprintln!("skipping: Azimuth 3000 .tap not present");
        return;
    };
    let tap = parse_tap(&bytes).unwrap_or_else(|e| panic!("parse_tap: {e}"));
    assert_eq!(tap.version, 1);
    assert_eq!(tap.platform, 0); // C64
    assert_eq!(tap.video, 0); // PAL
    assert_eq!(tap.pulses.len(), 588_144);
    // Every pulse is positive (no v0 overflow markers in this v1 image).
    assert!(tap.pulses.iter().all(|&c| c > 0));
    // ~236 seconds of tape at the PAL clock.
    let secs = tap.duration_secs();
    assert!((230.0..240.0).contains(&secs), "duration {secs} s out of range");
    eprintln!("ok: Azimuth 3000 ({} pulses, {secs:.1} s)", tap.pulses.len());
}
