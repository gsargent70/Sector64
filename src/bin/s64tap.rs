//! `s64tap` — summarise a Commodore **TAP** datasette image.
//!
//! Usage: `s64tap <file.tap>`
//!
//! Prints the header and pulse-stream statistics. A TAP records raw tape
//! pulses, not files: the bit encoding is loader-specific (most commercial
//! tapes use custom turbo loaders), so there is no general file extraction.

use std::process::ExitCode;

use sector64::parse_tap;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: s64tap <file.tap>");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("s64tap: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let tap = match parse_tap(&bytes) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("s64tap: {e}");
            return ExitCode::FAILURE;
        }
    };

    let platform = match tap.platform {
        0 => "C64",
        1 => "VIC-20",
        2 => "C16/plus4",
        _ => "unknown",
    };
    let video = match tap.video {
        0 => "PAL",
        1 => "NTSC",
        2 => "old NTSC",
        _ => "unknown",
    };
    println!("version:   {}", tap.version);
    println!("platform:  {platform}");
    println!("video:     {video}");
    println!("pulses:    {}", tap.pulses.len());
    println!("cycles:    {}", tap.total_cycles());
    println!("duration:  {:.1} s", tap.duration_secs());
    ExitCode::SUCCESS
}
