//! Every BASIC program on the sample disks and archives: its listing,
//! tokenized, must list the same again. Skipped when the samples are absent.

use std::path::{Path, PathBuf};

use sector64::listing::{disk_files, read_archive};
use sector64::tokenize::tokenize;
use sector64::{detokenize_basic, detokenize_basic_as, Disk};

fn samples() -> PathBuf {
    std::env::var_os("SECTOR64_SAMPLES").map_or_else(
        || {
            Path::new(&std::env::var_os("HOME").unwrap_or_default())
                .join("Downloads/Sector64 Test Files")
        },
        PathBuf::from,
    )
}

#[test]
fn sample_programs_survive_listing_and_tokenizing() {
    let Ok(dir) = std::fs::read_dir(samples()) else {
        eprintln!("samples not found; skipping");
        return;
    };
    let (mut programs, mut exact) = (0, 0);
    for path in dir.flatten().map(|e| e.path()).filter(|p| p.is_file()) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let files = match Disk::load(bytes.as_slice()) {
            Ok(d) => disk_files(&d).unwrap_or_default(),
            Err(_) => match read_archive(ext, &bytes) {
                Some(Ok(a)) => a.files,
                _ => continue,
            },
        };
        for f in files {
            let Ok(data) = f.data else { continue };
            let Some(l) = detokenize_basic(&data) else {
                continue;
            };
            let name = format!("{} in {}", f.name, path.display());
            let t = match tokenize(&l.text, l.dialect, Some(l.load_address)) {
                Ok(t) => t,
                Err(p) => panic!("{name}: {}", p[0]),
            };
            let again = detokenize_basic_as(&t.prg, l.dialect).unwrap();
            assert_eq!(again.text, l.text, "{name}");
            programs += 1;
            exact += usize::from(data.starts_with(&t.prg));
        }
    }
    eprintln!("{programs} programs, {exact} byte for byte");
}
