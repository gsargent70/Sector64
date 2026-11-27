//! Run in VICE: what to start and where VICE is.

use crate::*;

/// What Run in VICE starts.
pub(crate) struct VicePlan {
    /// The file written to the temporary folder ("games.d64", "name.prg"):
    /// the disk, or else the selected program.
    pub(crate) file_name: String,
    /// The program to load from a disk, as VICE's `-autostart image:name`
    /// takes it.
    pub(crate) program: Option<String>,
    /// Run it on a C128 (it loads where C128 BASIC starts).
    pub(crate) c128: bool,
    /// The program, for the status line.
    pub(crate) what: String,
}

/// Whether a program loads where a C128 keeps BASIC ($1C01, or $4001 with a
/// graphics screen), so it should run on a C128.
pub(crate) fn is_c128_load(data: &[u8]) -> bool {
    matches!(data, [0x01, 0x1C, ..] | [0x01, 0x40, ..])
}

/// A directory name as VICE's autostart takes it. VICE types
/// `LOAD"name",8,1` with lowercase ASCII becoming PETSCII capitals, so
/// letters go in lowercase and plain characters as they are. Anything that
/// can't be typed, or that would end the name early (`"`, `,`, `:`), becomes
/// the `?` wildcard, which matches any one character.
pub(crate) fn vice_load_name(raw: &[u8; 16]) -> String {
    let name: String = raw
        .iter()
        .take_while(|&&b| b != 0xA0)
        .map(|&b| match b {
            b'A'..=b'Z' => b.to_ascii_lowercase() as char,
            b'"' | b',' | b':' => '?',
            0x20..=0x40 => b as char,
            _ => '?',
        })
        .collect();
    if name.is_empty() {
        "*".into()
    } else {
        name
    }
}

/// A listed name made safe as a file name (the emulator shows it).
pub(crate) fn safe_file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || " -_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() {
        "program".into()
    } else {
        s
    }
}

/// VICE's emulators to try, best first.
pub(crate) fn vice_names(c128: bool) -> &'static [&'static str] {
    if c128 {
        &["x128"]
    } else {
        &["x64sc", "x64"]
    }
}

/// Find a VICE emulator: in the chosen folder (or its `bin`, as VICE's
/// Windows and macOS downloads have it), then on the PATH, then (macOS) in
/// the usual places.
pub(crate) fn find_vice(names: &[&str], dir: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if !dir.is_empty() {
        dirs.push(PathBuf::from(dir));
        dirs.push(Path::new(dir).join("bin"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    #[cfg(target_os = "macos")]
    dirs.extend(mac::vice_dirs());
    let ext = std::env::consts::EXE_SUFFIX;
    names.iter().find_map(|n| {
        dirs.iter()
            .map(|d| d.join(format!("{n}{ext}")))
            .find(|p| p.is_file())
    })
}
