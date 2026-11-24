//! What the GUI needs to behave on macOS, where the windowing library leaves
//! some of it out.

use std::ffi::{c_char, c_void, CStr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use eframe::egui;

/// Whether anything is unsaved, as of the last frame.
static UNSAVED: AtomicBool = AtomicBool::new(false);
/// A quit was refused and the app hasn't asked about the changes yet.
static REFUSED: AtomicBool = AtomicBool::new(false);
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

/// Cocoa's `NSApplicationTerminateReply`.
const TERMINATE_CANCEL: usize = 0;
const TERMINATE_NOW: usize = 1;

// The Objective-C runtime, which every Mac program has loaded.
#[link(name = "objc", kind = "dylib")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn class_addMethod(
        class: *mut c_void,
        selector: *mut c_void,
        method: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> usize,
        types: *const c_char,
    ) -> bool;
}

/// `-[NSApplicationDelegate applicationShouldTerminate:]`
extern "C" fn should_terminate(_: *mut c_void, _: *mut c_void, _: *mut c_void) -> usize {
    if !UNSAVED.load(Ordering::Relaxed) {
        return TERMINATE_NOW;
    }
    REFUSED.store(true, Ordering::Relaxed);
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
    TERMINATE_CANCEL
}

/// Make Quit (⌘Q, the Dock's menu, logging out) ask about unsaved changes.
/// It ends the program without ever asking the window to close, and winit's
/// application delegate has no say in it, so this gives it one. Call it once
/// the event loop is running.
pub fn guard_quit(ctx: &egui::Context) {
    const CLASS: &CStr = c"WinitApplicationDelegate";
    const METHOD: &CStr = c"applicationShouldTerminate:";
    const TYPES: &CStr = c"Q@:@"; // NSUInteger (id self, SEL, id sender)
    let _ = CONTEXT.set(ctx.clone());
    // SAFETY: the strings are NUL-terminated, the method has the signature
    // TYPES gives, and the class, once registered, is never freed.
    // class_addMethod leaves a class that already has the method unchanged.
    let added = unsafe {
        let class = objc_getClass(CLASS.as_ptr());
        !class.is_null()
            && class_addMethod(
                class,
                sel_registerName(METHOD.as_ptr()),
                should_terminate,
                TYPES.as_ptr(),
            )
    };
    if !added {
        eprintln!("sector64-gui: Quit won't ask about unsaved changes");
    }
}

/// Note whether anything is unsaved, and return whether a quit has been
/// refused because of it since the last call.
pub fn quit_refused(unsaved: bool) -> bool {
    UNSAVED.store(unsaved, Ordering::Relaxed);
    REFUSED.swap(false, Ordering::Relaxed)
}

/// Finder starts programs in "/". Move to the home folder instead.
pub fn leave_root_dir() {
    if std::env::current_dir().is_ok_and(|d| d == Path::new("/")) {
        if let Some(home) = std::env::var_os("HOME") {
            let _ = std::env::set_current_dir(home);
        }
    }
}

/// Where VICE may be: Homebrew's folders, which aren't on the PATH of a
/// program started from Finder, and the `bin` of a VICE download kept in an
/// Applications folder.
pub fn vice_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    let mut apps = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        apps.push(Path::new(&home).join("Applications"));
    }
    for folder in apps {
        let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.starts_with("vice") && !name.ends_with(".app")
            })
            .map(|e| e.path().join("bin"))
            .collect();
        // The newest version first, when several are there.
        found.sort();
        found.reverse();
        dirs.extend(found);
    }
    dirs
}
