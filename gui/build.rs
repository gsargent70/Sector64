//! Embed the app icon into the Windows executable, so Explorer, shortcuts and
//! the taskbar show the Sector64 floppy instead of the generic program icon.
//! On every other target this does nothing.
//!
//! The icon resource is compiled with the platform's resource compiler and no
//! extra crates: `windres` for the GNU target (cross-prefixed when building
//! from Linux; override with `WINDRES`), `rc.exe` for the MSVC target.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/sector64.ico");
    println!("cargo:rerun-if-env-changed=WINDRES");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let ico = manifest.join("assets").join("sector64.ico");
    let rc = out.join("sector64.rc");
    let res = out.join("sector64.res");

    // Resource scripts want forward slashes (or doubled backslashes).
    let ico_path = ico.display().to_string().replace('\\', "/");
    std::fs::write(&rc, format!("1 ICON \"{ico_path}\"\n")).expect("write .rc");

    let status = if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        Command::new("rc")
            .arg("/nologo")
            .arg("/fo")
            .arg(&res)
            .arg(&rc)
            .status()
    } else {
        let windres = env::var("WINDRES").unwrap_or_else(|_| {
            if env::var("HOST") == env::var("TARGET") {
                "windres".into()
            } else {
                let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
                format!("{arch}-w64-mingw32-windres")
            }
        });
        Command::new(windres)
            .args(["--input-format=rc", "--output-format=coff"])
            .arg("-i")
            .arg(&rc)
            .arg("-o")
            .arg(&res)
            .status()
    };

    match status {
        Ok(s) if s.success() => {
            println!("cargo:rustc-link-arg-bins={}", res.display());
        }
        // A missing icon must never break the build; just say so.
        other => println!(
            "cargo:warning=could not compile the Windows icon resource ({other:?}); \
             the .exe will use the default icon"
        ),
    }
}
