//! Unit tests and headless whole-UI tests for the GUI.

use super::*;

/// Run `f` inside one egui frame of a `width`-wide screen, so font
/// metrics and the screen size are available.
fn in_frame<R>(width: f32, f: impl FnOnce(&egui::Context) -> R) -> R {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 700.0),
        )),
        ..Default::default()
    };
    let mut out = None;
    let mut f = Some(f);
    let _ = ctx.run(input, |ctx| {
        if let Some(f) = f.take() {
            out = Some(f(ctx));
        }
    });
    out.unwrap()
}

fn sample_cols() -> [Cols; 3] {
    let disk = Cols {
        detail_chars: 7, // "162 BLK"
        size_digits: 5,
        has_errors: false,
        name_chars: 16,
    };
    let arc = Cols {
        detail_chars: 22, // "19374 BYTES  huffman+rle"
        size_digits: 5,
        has_errors: false,
        name_chars: 16,
    };
    let broken = Cols {
        detail_chars: 0,
        size_digits: 2,
        has_errors: true,
        name_chars: 16,
    };
    [disk, arc, broken]
}

#[test]
fn list_columns_never_overlap_at_any_width() {
    let cw = 10.0;
    let left = 8.0;
    for cols in sample_cols() {
        let mut shown_before = 0;
        for w in (0..2000).step_by(5) {
            let right = left + w as f32;
            let p = place_cols(left, right, cw, cols);
            let mut shown = 0;
            if let Some(sx) = p.status_x {
                shown += 1;
                assert!(sx + cols.size_chars() as f32 * cw <= right + 0.01);
                if let Some(dx) = p.detail_x {
                    shown += 1;
                    assert!(dx + cols.detail_chars as f32 * cw <= sx);
                    assert!(p.name_end <= dx);
                } else {
                    assert!(p.name_end <= sx);
                }
                // A column is only shown with a minimum-length name
                // beside it (after the 6-char type badge).
                assert!(p.name_end - (left + 6.0 * cw) >= MIN_NAME_CHARS as f32 * cw - 0.01);
            } else {
                assert!(p.detail_x.is_none());
                assert!(p.name_end <= right);
            }
            // Widening the row never hides a column.
            assert!(
                shown >= shown_before,
                "width {w}: columns went {shown_before} → {shown}"
            );
            shown_before = shown;
        }
        assert_eq!(shown_before, if cols.detail_chars > 0 { 2 } else { 1 });
    }
}

#[test]
fn preview_pane_fits_hex_lines_and_gives_way_to_the_list() {
    let dump = hexdump(&[0x41; 64], 64);
    let line = dump.lines().map(|l| l.chars().count()).max().unwrap();
    for cols in sample_cols() {
        let mut last = 0.0;
        for screen in (400..3000).step_by(20) {
            let screen = screen as f32;
            let (pane, cw) = in_frame(screen, |ctx| {
                let cw = ctx.fonts(|f| f.glyph_width(&egui::FontId::monospace(16.0), '0'));
                (
                    preview_width(ctx, PreviewMode::Hex, &dump, Some("STARFALL.GAME"), cols),
                    cw,
                )
            });
            let want_min = line as f32 * cw;
            let floor = 20.0 * cw;
            // Never wider than a full hex line needs (plus chrome).
            assert!(pane <= want_min + 120.0, "screen {screen}: pane {pane}");
            // Never narrower than the floor.
            assert!(
                pane >= floor,
                "screen {screen}: pane {pane} < floor {floor}"
            );
            // Wide screens: the whole hex line fits.
            let list_full = cols.row_chars_with_name(16, true, true) as f32 * cw + 80.0;
            if screen >= want_min + 120.0 + list_full {
                assert!(
                    pane >= want_min,
                    "screen {screen}: pane {pane} < line {want_min}"
                );
            }
            // Widening the window never shrinks the pane.
            assert!(pane >= last);
            last = pane;
        }
    }
}

#[test]
fn text_preview_is_forty_columns_wide() {
    let cols = sample_cols()[0];
    let (pane, cw) = in_frame(2400.0, |ctx| {
        let cw = ctx.fonts(|f| f.glyph_width(&egui::FontId::monospace(16.0), '0'));
        (
            preview_width(ctx, PreviewMode::Text, "HELLO", Some("README"), cols),
            cw,
        )
    });
    assert!(pane >= TEXT_PREVIEW_COLS as f32 * cw);
    assert!(pane <= (TEXT_PREVIEW_COLS + 12) as f32 * cw);
}

#[test]
#[cfg(unix)] // the expected strings are Unix absolute paths
fn remember_recent_dedupes_and_caps() {
    let mut list = Vec::new();
    for i in 0..12 {
        remember_recent(&mut list, Path::new(&format!("/disks/{i}.d64")));
    }
    assert_eq!(list.len(), RECENT_MAX);
    assert_eq!(list[0], "/disks/11.d64");
    assert_eq!(list[RECENT_MAX - 1], "/disks/2.d64");
    // Re-opening an older file moves it to the front, once.
    remember_recent(&mut list, Path::new("/disks/5.d64"));
    assert_eq!(list[0], "/disks/5.d64");
    assert_eq!(list.iter().filter(|p| *p == "/disks/5.d64").count(), 1);
    assert_eq!(list.len(), RECENT_MAX);
    // Relative paths are stored absolute, so they still work from
    // another working directory.
    remember_recent(&mut list, Path::new("rel.d64"));
    assert!(Path::new(&list[0]).is_absolute());
}

#[test]
fn recent_labels_add_the_folder_only_for_clashing_names() {
    let list: Vec<String> = ["/a/games/disk.d64", "/a/tools/disk.d64", "/a/x/demo.lnx"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let labels: Vec<String> = recent_labels(&list).into_iter().map(|(l, _)| l).collect();
    assert_eq!(
        labels,
        ["disk.d64  (games)", "disk.d64  (tools)", "demo.lnx"]
    );
    assert_eq!(recent_labels(&list)[1].1, "/a/tools/disk.d64");
}

#[test]
fn gfx_preview_aligns_prg_cells_to_memory() {
    // A PRG loading at $2FFF: the first sprite boundary is $3000, one
    // byte in, which is sprite pointer $C0 in its VIC bank.
    let mut prg = vec![0xff, 0x2f, 0xaa];
    prg.extend(std::iter::repeat_n(0x80, 64 * 2));
    let (sheet, load, total) = gfx_sheet(&prg, true, GfxKind::Sprites, false, 8, 100);
    assert_eq!(
        (load, sheet.start, total, sheet.count),
        (Some(0x2fff), 3, 2, 2)
    );
    assert_eq!(
        gfx_cell_info(&sheet, load, 1),
        "sprite 1 · offset $0043\nloads at $3040 · sprite pointer $C1"
    );
    // The same bytes as a SEQ file start at byte 0 and have no address.
    let (seq, load, _) = gfx_sheet(&prg, false, GfxKind::Chars, true, 16, 3);
    assert_eq!((load, seq.start, seq.count), (None, 0, 3));
    assert_eq!(gfx_cell_info(&seq, None, 2), "char 2 · offset $0010");
    let (chars, load, _) = gfx_sheet(&prg, true, GfxKind::Chars, false, 16, 100);
    assert_eq!(
        gfx_cell_info(&chars, load, 0),
        "char 0 · offset $0003\nloads at $3000 · screen code $00"
    );
    assert_eq!(gfx_color(sector64::gfx::GAP), Color32::TRANSPARENT);
}

#[test]
fn pic_failures_say_what_was_expected() {
    let why = pic_failure(&[0x01, 0x08, 0x00], None);
    assert!(
        why.contains("Koala Painter pictures load at $6000"),
        "{why}"
    );
    assert!(why.contains("this file loads at $0801"), "{why}");
    assert_eq!(
        pic_failure(&[0x00, 0x60], Some(PicFormat::Doodle)),
        "(too short for a Doodle! picture)"
    );
    assert_eq!(c64_color(6), c64::BG);
}

#[test]
fn dis_preview_starts_at_the_sys_target() {
    // 10 SYS2061, then INC $D020 / RTS at $080D.
    let prg = [
        0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x9E, 0x32, 0x30, 0x36, 0x31, 0x00, 0x00, 0x00, 0xEE,
        0x20, 0xD0, 0x60,
    ];
    assert_eq!(
        dis_preview(&prg, true),
        "; SYS 2061 → $080D (loads at $0801)\n\
         080D  EE 20 D0  INC $D020\n\
         0810  60        RTS\n"
    );
    // Without a load address, everything from $0000.
    assert_eq!(
        dis_preview(&[0xA7, 0x10], false),
        "; no load address: listed from $0000\n0000  A7 10    *LAX $10\n"
    );
    assert!(
        dis_preview(&[0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x99, 0, 0, 0], true)
            .starts_with("; a BASIC program with no SYS")
    );
    let big = vec![0xEA; DIS_MAX_LINES + 5];
    assert!(dis_preview(&big, false).ends_with("(5 more bytes)\n"));
}

#[test]
fn basic_preview_lists_programs_and_flags_the_rest() {
    // 10 PRINT "HI"  (link $080D), end marker, then 2 bytes of code.
    let prg = [
        0x01, 0x08, 0x0D, 0x08, 0x0A, 0x00, 0x99, 0x20, 0x22, 0x48, 0x49, 0x22, 0x00, 0x00, 0x00,
        0xEA, 0x60,
    ];
    assert_eq!(
        basic_preview(&prg, None),
        "10 PRINT \"HI\"\n\n(followed by 2 bytes of machine code or data)"
    );
    assert_eq!(basic_preview(&prg[..15], None), "10 PRINT \"HI\"\n");
    assert!(basic_preview(b"\x00\xc0\xa9\x00\x60", None).starts_with("(not a BASIC program"));

    // 10 HIRES 0 in Simons' BASIC: detected and labelled, or listed as
    // chosen.
    let simons = [
        0x01, 0x08, 0x0C, 0x08, 0x0A, 0x00, 0x64, 0x01, 0x20, 0x30, 0x00, 0x00, 0x00,
    ];
    assert_eq!(
        basic_preview(&simons, None),
        "(listed as Simons' BASIC)\n10 HIRES 0\n"
    );
    assert_eq!(
        basic_preview(&simons, Some(BasicDialect::Simons)),
        "10 HIRES 0\n"
    );
    assert_eq!(
        basic_preview(&simons, Some(BasicDialect::V2)),
        "10 {$64}{$01} 0\n"
    );
}

/// An app with a blank, saved D64 open (as if loaded from a file).
fn app_with_disk() -> App {
    let mut app = App {
        pane: Pane {
            disk: Some(EditDisk {
                disk: Disk::format("UNDO TEST", "UT"),
                path: None,
                dirty: false,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    app.pane.refresh();
    app
}

fn names(app: &App) -> Vec<String> {
    app.pane.nodes.iter().map(|n| n.name.clone()).collect()
}

#[test]
fn tab_focus_swaps_the_panes_only_in_dual_view() {
    let mut app = app_with_disk();
    app.add("ONE".into(), &[1, 8, 0]);
    app.apply(Act::FocusOther);
    assert_eq!(names(&app), ["ONE"], "single view: nothing to focus");
    app.dual = true;
    app.apply(Act::FocusOther);
    assert!(app.focus_right && app.pane.nodes.is_empty());
    assert_eq!(app.back.nodes[0].name, "ONE");
    // Each pane keeps its own undo history.
    app.apply(Act::Undo);
    assert_eq!(app.status, "Nothing to undo");
    app.apply(Act::FocusOther);
    assert!(!app.focus_right);
    app.apply(Act::Undo);
    assert!(names(&app).is_empty());
}

#[test]
fn f5_copies_into_the_other_panes_disk_or_archive() {
    let mut app = app_with_disk();
    app.dual = true;
    app.add("ONE".into(), &[1, 8, 0]);
    app.add("TWO".into(), &[1, 8, 1]);
    app.pane.marked.extend([0, 1]);
    // Nothing open in the other pane yet.
    app.apply(Act::CopyToPane);
    assert!(app.status.contains("in the other pane to copy into"));

    // Into a blank disk: one undo step there, nothing changed here.
    app.apply(Act::FocusOther);
    app.apply(Act::Create);
    app.apply(Act::FocusOther);
    app.pane.marked.extend([0, 1]);
    app.apply(Act::CopyToPane);
    assert_eq!(app.status, "Copied 2 files to the other pane");
    let back: Vec<_> = app.back.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(back, ["ONE", "TWO"]);
    assert!(app.back.is_dirty());
    assert_eq!(app.back.undo.len(), 1);
    assert_eq!(names(&app), ["ONE", "TWO"]);
    // Copying again clashes on the names.
    app.pane.marked.extend([0]);
    app.apply(Act::CopyToPane);
    assert!(app.status.starts_with("Nothing copied; skipped \"ONE\""));

    // Into a new archive, with the bytes and type.
    app.apply(Act::FocusOther);
    app.pane.disk.as_mut().unwrap().dirty = false; // or it asks first
    app.apply(Act::NewArchive);
    app.apply(Act::FocusOther);
    app.apply(Act::Select(1));
    app.apply(Act::CopyToPane);
    let stage = app.back.archive.as_ref().unwrap();
    assert_eq!(
        (stage[0].name.as_str(), &stage[0].data[..]),
        ("TWO", &[1, 8, 1][..])
    );
}

#[test]
fn compare_rows_select_the_file_in_both_panes() {
    let mut app = app_with_disk();
    app.dual = true;
    app.add("A".into(), &[1, 8, 0]);
    app.add("B".into(), &[1, 8, 1]);
    app.apply(Act::FocusOther);
    app.apply(Act::Create);
    app.add("B".into(), &[1, 8, 2]);
    // The focused pane is now the right one.
    app.pane.marked.insert(0);
    app.apply(Act::CompareSelect(Some(1), Some(0)));
    assert_eq!((app.back.selected, app.pane.selected), (Some(1), Some(0)));
    assert!(app.pane.marked.is_empty());
    // A file on one side only leaves the other pane's selection alone.
    app.apply(Act::CompareSelect(Some(0), None));
    assert_eq!((app.back.selected, app.pane.selected), (Some(0), Some(0)));

    let rows = sector64::compare::compare(&compare_items(&app.back), &compare_items(&app.pane));
    let s = sector64::compare::summarize(&rows);
    assert_eq!(s.to_string(), "1 differ, 1 only on the left");
    let note = rows[1].status.describe([Some("prg"), Some("prg")]);
    assert_eq!(
        (rows[1].status.mark(), note.as_str()),
        ("*", "differs from byte 2")
    );
}

#[test]
fn compare_copies_the_missing_files_either_way() {
    let names = |p: &Pane| p.nodes.iter().map(|n| n.name.clone()).collect::<Vec<_>>();
    let mut app = app_with_disk();
    app.dual = true;
    for name in ["A", "BOTH", "C"] {
        app.add(name.into(), &[1, 8, 0]);
    }
    app.apply(Act::FocusOther);
    app.apply(Act::Create);
    app.add("BOTH".into(), &[1, 8, 9]);
    app.add("D".into(), &[1, 8, 0]);

    // The right pane has the focus, so this copies into it.
    let undo = app.pane.undo.len();
    app.apply(Act::CopyMissing(true));
    assert_eq!(app.status, "Copied 2 files to the right pane");
    assert_eq!(names(&app.pane), ["BOTH", "D", "A", "C"]);
    assert_eq!(names(&app.back), ["A", "BOTH", "C"]);
    assert_eq!(app.pane.nodes[0].data.as_deref(), Ok(&[1, 8, 9][..]));
    // One Undo takes all of them back.
    assert_eq!(app.pane.undo.len(), undo + 1);
    app.apply(Act::Undo);
    assert_eq!(names(&app.pane), ["BOTH", "D"]);

    // Into the unfocused left pane, then the same with the focus there.
    app.apply(Act::CopyMissing(false));
    assert_eq!(app.status, "Copied \"D\" to the left pane");
    assert_eq!(names(&app.back), ["A", "BOTH", "C", "D"]);
    app.apply(Act::FocusOther);
    app.apply(Act::CopyMissing(true));
    assert_eq!(names(&app.back), ["BOTH", "D", "A", "C"]);
    let rows = sector64::compare::compare(&compare_items(&app.pane), &compare_items(&app.back));
    let s = sector64::compare::summarize(&rows);
    assert_eq!((s.same, s.differ, s.only_left + s.only_right), (3, 1, 0));

    // A read-only pane is left alone.
    app.back.disk = None;
    app.back.nodes.clear();
    app.apply(Act::CopyMissing(true));
    assert!(app.status.starts_with("Open a disk"), "{}", app.status);
}

#[test]
fn copying_a_long_archive_name_twice_is_refused() {
    let node = Node {
        name: "A NAME LONGER THAN SIXTEEN".into(),
        badge: "prg".into(),
        detail: String::new(),
        blocks: 0,
        data: Ok(vec![1, 8, 0]),
    };
    let mut disk = Disk::format("D", "D1");
    copy_bytes(&mut disk, &node).unwrap();
    assert!(copy_bytes(&mut disk, &node).is_err());
    assert_eq!(disk.directory().unwrap().len(), 1);
}

#[test]
fn validating_offers_repairs_as_one_undo_step() {
    let mut app = app_with_disk();
    app.add("ONE".into(), &[1; 600]);
    app.apply(Act::Validate);
    assert!(app.show_validation && !app.validation_bad);
    assert!(app.repair.choices.is_empty());

    // Break ONE's chain after its first block.
    let disk = &mut app.pane.disk.as_mut().unwrap().disk;
    let e = disk.find_entry("ONE").unwrap().unwrap();
    disk.block_mut(e.first_track, e.first_sector).unwrap()[0] = 99;
    let broken = disk.as_bytes().to_vec();
    // The window's list is out of date, so it's only checked again.
    app.apply(Act::Repair);
    assert_eq!(app.pane.disk.as_ref().unwrap().disk.as_bytes(), broken);
    assert!(app.validation_bad, "{}", app.validation);
    let undo = app.pane.undo.len();
    app.apply(Act::Repair);
    assert_eq!(app.status, "Repaired the disk (1 fix)");
    assert_eq!(app.pane.nodes[0].data.as_ref().unwrap().len(), 254);
    assert!(app.pane.is_dirty());
    // What's left is the cut-off blocks, which aren't damage.
    assert!(!app.validation_bad, "{}", app.validation);
    assert_eq!(app.repair.choices.len(), 1);
    assert_eq!(app.pane.undo.len(), undo + 1);
    app.apply(Act::Undo);
    assert_eq!(app.pane.disk.as_ref().unwrap().disk.as_bytes(), broken);
}

#[test]
fn basic_editor_adds_lists_and_replaces() {
    let mut app = app_with_disk();
    app.basic.text = "10 PRINT \"HI\"\n20 GOTO 10\n".into();
    app.apply(Act::BasicAdd);
    assert_eq!(app.status, "Enter a name for the program");
    app.basic.name = "hello".into();
    app.basic.modified = true;
    app.apply(Act::BasicAdd);
    assert_eq!(app.status, "Added \"HELLO\" from the BASIC editor");
    assert_eq!(app.pane.selected, Some(0));
    assert!(!app.basic.modified && app.pane.is_dirty());

    // Listed back, changed and put back: one file still, one Undo each.
    app.basic = BasicEditor::default();
    app.apply(Act::BasicFromSelected);
    assert_eq!(app.basic.text, "10 PRINT \"HI\"\n20 GOTO 10\n");
    assert_eq!((app.basic.name.as_str(), app.basic.load), ("HELLO", None));
    app.basic.text.push_str("30 END\n");
    app.basic.checked = None;
    app.apply(Act::BasicAdd);
    assert_eq!(app.status, "Replaced \"HELLO\" from the BASIC editor");
    assert_eq!(names(&app), ["HELLO"]);
    let data = app.pane.nodes[0].data.as_ref().unwrap();
    assert_eq!(sector64::detokenize_basic(data).unwrap().lines, 3);
    app.apply(Act::Undo);
    let data = app.pane.nodes[0].data.as_ref().unwrap();
    assert_eq!(sector64::detokenize_basic(data).unwrap().lines, 2);

    // Nothing but a plain BASIC program is replaced.
    app.add("CODE".into(), &[0, 0xC0, 0x60]);
    app.basic.name = "CODE".into();
    app.apply(Act::BasicAdd);
    assert!(
        app.status.contains("isn't a BASIC program"),
        "{}",
        app.status
    );
    let mut stub = app.basic.check().as_ref().unwrap().prg.clone();
    stub.push(0x60);
    app.add("STUB".into(), &stub);
    app.basic.name = "STUB".into();
    app.apply(Act::BasicAdd);
    assert!(app.status.contains("machine code"), "{}", app.status);

    // Bad text is refused, saying where.
    app.basic.text = "10 {NOPE}".into();
    app.basic.checked = None;
    app.apply(Act::BasicAdd);
    assert!(
        app.status.starts_with("Error in text line 1"),
        "{}",
        app.status
    );
    assert_eq!(names(&app), ["HELLO", "CODE", "STUB"]);

    // A new archive works too.
    app.pane.disk.as_mut().unwrap().dirty = false; // or it asks first
    app.apply(Act::NewArchive);
    app.basic.set_text("10 END".into(), Some(0x1001));
    app.basic.name = "vic".into();
    app.apply(Act::BasicAdd);
    app.apply(Act::BasicAdd);
    assert_eq!(names(&app), ["VIC"]);
    let stage = app.pane.archive.as_ref().unwrap();
    assert_eq!(stage[0].data[..2], [0x01, 0x10]);
}

#[test]
fn unsaved_prompt_names_each_disk() {
    assert_eq!(
        unsaved_message(&["the open disk"]),
        "The open disk has unsaved changes."
    );
    assert_eq!(
        unsaved_message(&["this pane's disk", "the other pane's disk"]),
        "This pane's disk and the other pane's disk have unsaved changes."
    );
    assert_eq!(
        unsaved_message(&["one", "two", "three"]),
        "One, two and three have unsaved changes."
    );
}

#[test]
fn vice_load_names_are_typeable_with_wildcards_for_the_rest() {
    let raw = |b: &[u8]| {
        let mut r = [0xA0u8; 16];
        r[..b.len()].copy_from_slice(b);
        r
    };
    assert_eq!(vice_load_name(&raw(b"GAME V1.2")), "game v1.2");
    assert_eq!(vice_load_name(&raw(b"A\"B,C:D")), "a?b?c?d");
    // Shifted letters, graphics and control codes can't be typed.
    assert_eq!(vice_load_name(&raw(&[b'X', 0xC1, 0x5C, 0x0D])), "x???");
    assert_eq!(vice_load_name(&raw(b"")), "*");
    assert_eq!(vice_load_name(&[b'Z'; 16]), "z".repeat(16));
}

#[test]
fn c128_programs_and_safe_file_names() {
    assert!(is_c128_load(&[0x01, 0x1C, 0]));
    assert!(is_c128_load(&[0x01, 0x40, 0]));
    assert!(!is_c128_load(&[0x01, 0x08, 0]));
    assert!(!is_c128_load(&[0x01]));
    assert_eq!(safe_file_name("my game/v2*"), "my game_v2_");
    assert_eq!(safe_file_name(".."), "program");
}

#[test]
fn host_files_take_their_type_from_their_name_or_pc64_header() {
    let get = |name: &str, bytes: &[u8]| {
        host_import(Path::new(name), bytes.to_vec(), FileType::Usr)
            .map(|f| (f.name, f.file_type, f.data))
    };
    assert_eq!(
        get("game.prg", b"\x01\x08"),
        Ok(("GAME".into(), FileType::Prg, vec![1, 8]))
    );
    assert_eq!(get("/x/Notes.SEQ", b"hi").map(|f| f.1), Ok(FileType::Seq));
    assert_eq!(get("a.usr", b"").map(|f| f.1), Ok(FileType::Usr));
    assert_eq!(get("old.del", b"").map(|f| f.1), Ok(FileType::Del));
    // No type in the name: the fallback, and the whole name is kept.
    assert_eq!(
        get("GAME V1.2", b"x").map(|f| (f.0, f.1)),
        Ok(("GAME V1.2".into(), FileType::Usr))
    );
    assert_eq!(
        get("a very long file name.txt", b"").map(|f| f.0.len()),
        Ok(16)
    );
    // PC64: the header's name, the extension's type, the data after it.
    let mut p00 = b"C64File\0".to_vec();
    p00.extend(b"MY NOTES\0\0\0\0\0\0\0\0");
    p00.extend([0, 0, b'h', b'i']);
    assert_eq!(
        get("mynote~1.s00", &p00),
        Ok(("MY NOTES".into(), FileType::Seq, b"hi".to_vec()))
    );
    assert_eq!(get("x.P01", &p00).map(|f| f.1), Ok(FileType::Prg));
    assert!(get("x.p00", b"not a pc64 file").is_err());
    assert!(get("x.rel", b"").is_err());
    assert!(get("x.r00", &p00).is_err());
}

#[test]
fn adding_several_files_is_one_undo_step_and_reports_what_failed() {
    let dir = std::env::temp_dir().join(format!("s64-add-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths: Vec<PathBuf> = ["intro.prg", "readme.seq", "data.rel"]
        .iter()
        .map(|n| {
            let p = dir.join(n);
            std::fs::write(&p, [1, 8, 0]).unwrap();
            p
        })
        .collect();
    let mut app = app_with_disk();
    app.apply(Act::AddPaths(paths));
    std::fs::remove_dir_all(&dir).unwrap();
    let listed: Vec<(&str, &str)> = app
        .pane
        .nodes
        .iter()
        .map(|n| (n.name.as_str(), n.badge.as_str()))
        .collect();
    assert_eq!(listed, [("INTRO", "prg"), ("README", "seq")]);
    assert_eq!(
        app.status,
        "Added 2 files; cannot add data.rel (REL files can't be added)"
    );
    assert!(ToastKind::of(&app.status) == ToastKind::Error);
    assert_eq!(app.pane.undo.len(), 1);
    app.apply(Act::Undo);
    assert!(app.pane.nodes.is_empty());

    // Into a new archive too; a file that can't be read is named.
    let mut app = App::default();
    app.apply(Act::NewArchive);
    app.apply(Act::AddPaths(vec![dir.join("gone.prg")]));
    assert!(
        app.status.starts_with("Cannot add gone.prg ("),
        "{}",
        app.status
    );
    assert!(app.pane.undo.is_empty());
}

#[test]
fn vice_runs_the_selected_program_or_else_the_first() {
    let mut app = app_with_disk();
    assert_eq!(
        app.vice_plan().err().as_deref(),
        Some("there's no program on this disk to run")
    );
    app.add_type = 1; // SEQ
    app.add("NOTES".into(), b"hello");
    app.add_type = 0; // PRG
    app.add("FIRST".into(), b"\x01\x08\x00\x00");
    app.add("SECOND".into(), b"\x01\x1c\x00\x00");
    let i = |app: &App, n: &str| app.pane.nodes.iter().position(|x| x.name == n);
    app.pane.selected = i(&app, "NOTES");
    let p = app.vice_plan().unwrap();
    assert_eq!(p.program.as_deref(), Some("first"));
    assert_eq!((p.file_name.as_str(), p.c128), ("disk.d64", false));
    app.pane.selected = i(&app, "SECOND");
    let p = app.vice_plan().unwrap();
    assert_eq!((p.program.as_deref(), p.c128), (Some("second"), true));

    // From an archive listing, the selected program runs on its own.
    let mut list = App {
        pane: Pane {
            nodes: vec![Node {
                name: "DEMO/1".into(),
                badge: "prg".into(),
                detail: String::new(),
                blocks: 0,
                data: Ok(vec![0x01, 0x08, 0x00]),
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(list.vice_plan().is_err());
    list.pane.selected = Some(0);
    let p = list.vice_plan().unwrap();
    assert_eq!((p.file_name.as_str(), p.program), ("DEMO_1.prg", None));
}

#[test]
fn vice_is_found_in_the_chosen_folder_or_its_bin() {
    let d = std::env::temp_dir().join(format!("s64-vice-find-{}", std::process::id()));
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join(format!("x128{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&exe, b"").unwrap();
    assert_eq!(find_vice(&["x128"], &d.display().to_string()), Some(exe));
    assert_eq!(
        find_vice(&["no-such-emulator"], &d.display().to_string()),
        None
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn undo_and_redo_step_through_disk_edits() {
    let mut app = app_with_disk();
    app.add("ONE".into(), b"\x01\x08");
    app.add("TWO".into(), b"\x01\x08");
    app.pane.selected = Some(0);
    app.pane.rename_to = "FIRST".into();
    app.rename();
    app.pane.selected = Some(1);
    app.delete();
    assert_eq!(names(&app), ["FIRST"]);

    app.undo(); // delete
    assert_eq!(names(&app), ["FIRST", "TWO"]);
    assert_eq!(app.status, "Undone: Deleted \"TWO\" (1 blocks freed)");
    app.undo(); // rename
    assert_eq!(names(&app), ["ONE", "TWO"]);
    app.undo(); // add TWO
    app.undo(); // add ONE
    assert!(names(&app).is_empty());
    assert!(!app.pane.is_dirty(), "back to the state that was loaded");
    app.undo();
    assert_eq!(app.status, "Nothing to undo");

    app.redo();
    app.redo();
    assert_eq!(names(&app), ["ONE", "TWO"]);
    assert!(app.pane.is_dirty());
    // A new edit drops the redo history.
    app.add("NEW".into(), b"\x01\x08");
    app.redo();
    assert_eq!(app.status, "Nothing to redo");
    assert_eq!(names(&app), ["ONE", "TWO", "NEW"]);
}

#[test]
fn sector_editor_writes_one_undoable_block() {
    let mut app = app_with_disk();
    app.add("PROG".into(), &[0x11; 300]);
    let first = app.pane.map_disk().unwrap().block_map().chains[0][0];
    // Opened from the disk map at the file's first block.
    app.apply(Act::SectorOpen(first.0, first.1));
    assert!(app.show_sector);
    assert_eq!((app.sector.track, app.sector.sector), first);
    app.sector_sync();
    app.sector.place(2);
    app.sector.type_digit(0xA);
    app.sector.type_digit(0x9);
    // Not on the disk until Write.
    let disk = |app: &App| app.pane.disk.as_ref().unwrap().disk.clone();
    assert_eq!(disk(&app).block(first.0, first.1).unwrap()[2], 0x11);
    // Jumping elsewhere is refused while edits are staged.
    app.apply(Act::SectorOpen(18, 0));
    assert_eq!((app.sector.track, app.sector.sector), first);
    app.apply(Act::SectorWrite);
    assert_eq!(
        app.status,
        format!("Wrote track {}, sector {}", first.0, first.1)
    );
    assert_eq!(disk(&app).block(first.0, first.1).unwrap()[2], 0xA9);
    assert!(app.pane.is_dirty());
    let data = app.pane.nodes[0].data.as_ref().unwrap();
    assert_eq!(data[0], 0xA9, "the listing shows the new bytes");

    app.undo();
    assert_eq!(disk(&app).block(first.0, first.1).unwrap()[2], 0x11);
    app.sector_sync();
    assert_eq!(app.sector.buf[2], 0x11, "the editor follows the undo");
}

#[test]
fn sector_editor_drops_edits_for_another_disk_and_is_view_only_on_read_only_ones() {
    let mut app = app_with_disk();
    app.show_sector = true;
    app.sector_sync();
    // A fresh editor opens on the header block.
    assert_eq!((app.sector.track, app.sector.sector), (18, 0));
    app.sector.type_digit(0);
    assert!(app.sector.modified());

    // A Zip-Code set opens read-only: the staged edits go, Write refuses.
    let dir = scratch("sector");
    for (i, part) in sector64::write_zipcode(&sample_d64())
        .unwrap()
        .iter()
        .enumerate()
    {
        std::fs::write(dir.join(format!("{}!GAME", i + 1)), part).unwrap();
    }
    app.open(&dir.join("1!GAME"));
    app.sector_sync();
    assert!(!app.sector.modified());
    assert_eq!((app.sector.track, app.sector.sector), (18, 0));
    app.sector.type_digit(1);
    app.apply(Act::SectorWrite);
    assert_eq!(
        app.status,
        "This disk is read-only, so its blocks can't be changed"
    );
}

#[test]
fn sector_grid_columns_map_back_to_bytes() {
    for i in 0..16 {
        assert_eq!(sec_byte_at(sec_hex_col(i)), Some(i));
        assert_eq!(sec_byte_at(sec_hex_col(i) + 1), Some(i));
        assert_eq!(sec_byte_at(SEC_TEXT0 + i), Some(i));
    }
    assert_eq!(sec_byte_at(0), None);
    assert_eq!(sec_byte_at(sec_hex_col(7) + 2), None);
    assert_eq!(sec_hex_col(8), sec_hex_col(7) + 4, "gap after the 8th byte");
    assert_eq!(sec_byte_at(SEC_COLS), None);
}

#[test]
fn find_searches_names_and_contents_and_shows_matches() {
    let mut app = app_with_disk();
    app.add("HELLO".into(), b"\x01\x08hello world");
    let mut big = vec![0u8; 600];
    big[300..303].copy_from_slice(&[0xA9, 0x00, 0x8D]);
    big[500..503].copy_from_slice(&[0xA9, 0x07, 0x8D]);
    app.add("CODE".into(), &big);
    app.add("OTHER".into(), b"nothing");

    app.find.query = "HELLO".into();
    app.apply(Act::Find);
    assert_eq!(app.find.hits.len(), 1);
    let h = &app.find.hits[0];
    assert!(h.in_name && h.total == 1 && h.offsets == [2]);
    assert_eq!(app.find.summary, "1 match in 1 file of 3 · 1 file by name");

    app.find.hex = true;
    app.find.query = "A9 ?? 8D".into();
    app.apply(Act::Find);
    assert_eq!(app.find.hits.len(), 1);
    assert_eq!(app.find.hits[0].offsets, [300, 500]);
    assert!(app.find.hits[0].on_disk);

    // Shown in the hex preview...
    app.apply(Act::FindShow(1, 500));
    assert_eq!(app.pane.selected, Some(1));
    assert!(app.preview_mode == PreviewMode::Hex && app.show_preview);
    assert_eq!(app.find.mark, Some((1, 500, 3)));
    assert!(app.find.scroll);

    // ...or in the Sector Editor: offset 500 is block 2, byte 2 + 246.
    app.find.in_sector = true;
    app.apply(Act::FindShow(1, 500));
    let chain = app.pane.map_disk().unwrap().block_map().chains[1].clone();
    assert!(app.show_sector);
    assert_eq!((app.sector.track, app.sector.sector), chain[1]);
    app.sector_sync();
    assert_eq!(app.sector.cursor, 248);
    assert_eq!(&app.sector.buf[248..251], &[0xA9, 0x07, 0x8D]);

    // Bad patterns explain themselves; a changed list is noticed.
    app.find.query = "A9 0".into();
    app.apply(Act::Find);
    assert!(app.find.summary.starts_with('⚠') && app.find.hits.is_empty());
    app.find.query = "8D".into();
    app.apply(Act::Find);
    app.pane.selected = Some(0);
    app.delete();
    app.find.in_sector = false;
    app.apply(Act::FindShow(1, 302));
    assert_eq!(
        app.status,
        "The listing has changed since the search; search again"
    );
}

#[test]
fn hex_preview_highlights_the_match_in_both_columns() {
    let data: Vec<u8> = (0u8..40).collect();
    let dump = hexdump(&data, 64);
    let job = hex_marked_layout(&dump, 7, 3, egui::FontId::monospace(10.0), Color32::WHITE);
    let marked: String = job
        .sections
        .iter()
        .filter(|s| s.format.background == c64::ACCENT_DIM)
        .map(|s| &job.text[s.byte_range.clone()])
        .collect::<Vec<_>>()
        .join("|");
    // Bytes 7, 8 and 9 (across the gap after the eighth), then their text.
    assert_eq!(marked, "07|08|09|...");
    assert_eq!(job.text, dump);
}

#[test]
fn f5_copies_archive_members_with_their_types_and_skips_rel() {
    // A read-only listing (as an archive opens) in the left pane.
    let mut app = App {
        pane: Pane {
            nodes: vec![
                Node {
                    name: "DOC".into(),
                    badge: "seq".into(),
                    detail: String::new(),
                    blocks: 0,
                    data: Ok(b"TEXT".to_vec()),
                },
                Node {
                    name: "DATA".into(),
                    badge: "rel".into(),
                    detail: String::new(),
                    blocks: 0,
                    data: Ok(vec![1; 10]),
                },
            ],
            ..Default::default()
        },
        dual: true,
        ..Default::default()
    };
    // A blank disk in the right pane.
    app.apply(Act::FocusOther);
    app.apply(Act::Create);
    app.apply(Act::FocusOther);
    app.pane.marked.extend([0, 1]);
    app.apply(Act::CopyToPane);
    assert!(app
        .status
        .ends_with("skipped \"DATA\": can't copy REL files"));
    let disk = &app.back.disk.as_ref().unwrap().disk;
    let e = &disk.directory().unwrap()[0];
    assert_eq!((e.name.as_str(), e.file_type), ("DOC", FileType::Seq));
    assert_eq!(disk.read_entry(e).unwrap(), b"TEXT");

    // A read-only listing can't receive a copy back.
    app.apply(Act::FocusOther);
    app.apply(Act::Select(0));
    app.apply(Act::CopyToPane);
    assert!(app.status.contains("in the other pane to copy into"));
    // The disk's Undo takes the copy back off.
    app.apply(Act::Undo);
    assert!(app.pane.nodes.is_empty());
}

#[test]
fn failed_edits_are_not_recorded_and_history_is_capped() {
    let mut app = app_with_disk();
    app.pane.selected = None;
    app.delete(); // nothing selected: no change
    assert!(app.pane.undo.is_empty());
    for i in 0..UNDO_MAX + 5 {
        app.add(format!("F{i}"), b"\x01\x08");
    }
    assert_eq!(app.pane.undo.len(), UNDO_MAX);
}

#[test]
fn undo_after_save_marks_the_disk_unsaved() {
    let dir = std::env::temp_dir().join(format!("s64-undo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("u.d64");
    let mut app = app_with_disk();
    app.pane.disk.as_mut().unwrap().path = Some(path.clone());
    app.add("ONE".into(), b"\x01\x08");
    app.save(false);
    assert!(!app.pane.is_dirty());
    app.undo();
    assert!(
        app.pane.is_dirty(),
        "the disk no longer matches the saved file"
    );
    app.redo();
    assert!(!app.pane.is_dirty(), "back to exactly what was saved");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undo_works_on_a_new_archive_and_opening_clears_history() {
    let mut app = App::default();
    app.apply(Act::NewArchive);
    app.add("A".into(), b"aaa");
    app.add("B".into(), b"bbb");
    app.undo();
    assert_eq!(names(&app), ["A"]);
    app.apply(Act::NewArchive);
    assert!(
        matches!(app.confirm, Some(Confirm::NewArchive)),
        "asks first"
    );
    app.new_archive(); // what Discard does
    assert!(app.pane.undo.is_empty() && app.pane.redo.is_empty());
}

#[test]
fn sector64_x11_values() {
    use std::ffi::OsStr;
    assert!(!force_x11(None));
    assert!(!force_x11(Some(OsStr::new(""))));
    assert!(!force_x11(Some(OsStr::new("0"))));
    assert!(force_x11(Some(OsStr::new("1"))));
    assert!(force_x11(Some(OsStr::new("yes"))));
}

#[test]
fn packed_details_align_the_method_column() {
    let d = packed_details(
        [(6068, "lz"), (992, "lz"), (19374, "huffman+rle")]
            .into_iter()
            .map(|(n, m)| (n, m.to_string())),
    );
    assert_eq!(
        d,
        [
            " 6068 BYTES  lz",
            "  992 BYTES  lz",
            "19374 BYTES  huffman+rle"
        ]
    );
}
use sector64::FileType;

/// A fresh, empty scratch folder under the system temp dir.
fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("s64gui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A small D64 with one file on it.
/// Run `frames` frames of the whole window at `width` × 760 and return
/// the x positions (sampled every pixel) along a line through the
/// middle that no opaque fill covers: places where whatever is behind
/// the window would show through.
fn unpainted_columns(app: &mut App, ctx: &egui::Context, width: f32) -> Vec<f32> {
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 760.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run(input(), |ctx| app.frame_ui(ctx));
    for _ in 0..3 {
        out = ctx.run(input(), |ctx| app.frame_ui(ctx));
    }
    fn fills(shape: &egui::Shape, clip: egui::Rect, acc: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Rect(r) if r.fill.a() == 255 => {
                acc.push(r.rect.intersect(clip));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| fills(s, clip, acc)),
            _ => {}
        }
    }
    let mut rects = Vec::new();
    for cs in &out.shapes {
        fills(&cs.shape, cs.clip_rect, &mut rects);
    }
    let y = 400.0;
    (0..width as usize)
        .map(|x| x as f32 + 0.5)
        .filter(|&x| !rects.iter().any(|r| r.contains(egui::pos2(x, y))))
        .collect()
}

#[test]
fn preview_pane_never_leaves_part_of_the_window_unpainted() {
    let ctx = egui::Context::default();
    load_c64_font(&ctx);
    apply_c64_theme(&ctx);
    for width in [520.0, 640.0, 760.0, 900.0, 1100.0, 1400.0] {
        for (mode, ..) in PREVIEW_MODES {
            let mut app = app_with_disk();
            app.add("A LONG FILE NAME".into(), &[0x41; 9000]);
            app.add("B".into(), &[1, 2, 3]);
            app.pane.selected = Some(0);
            app.show_preview = true;
            app.preview_mode = mode;
            let gaps = unpainted_columns(&mut app, &ctx, width);
            let label = PREVIEW_MODES.iter().find(|m| m.0 == mode).unwrap().1;
            assert!(
                gaps.is_empty(),
                "{width} px wide, {label}: unpainted from x={} to {} ({} px)",
                gaps[0],
                gaps[gaps.len() - 1],
                gaps.len()
            );
            // Nothing in the pane runs off the window's edge either.
            let panel = egui::containers::panel::PanelState::load(&ctx, "preview".into())
                .expect("the preview pane was drawn");
            assert!(
                panel.rect.max.x <= width + 0.5,
                "{width} px wide, {label}: the pane reaches x={}",
                panel.rect.max.x
            );
        }
    }
}

fn sample_d64() -> Vec<u8> {
    let mut disk = Disk::format("ZIP TEST", "ZT");
    let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
    disk.write_file("HELLO", FileType::Prg, &data).unwrap();
    disk.as_bytes().to_vec()
}

#[test]
fn disk_map_is_rebuilt_after_edits_and_covers_read_only_disks() {
    let mut app = app_with_disk();
    app.pane.map_cache = app.pane.map_disk().map(Disk::block_map);
    assert_eq!(app.pane.map_cache.as_ref().unwrap().files.len(), 0);
    app.add("NEW".into(), &[1u8; 600]);
    assert!(app.pane.map_cache.is_none(), "an edit drops the stale map");
    let map = app.pane.map_disk().unwrap().block_map();
    assert_eq!(map.blocks_of(0).len(), 3);
    app.pane.map_cache = Some(map);
    app.undo();
    assert!(app.pane.map_cache.is_none(), "undo drops it too");

    // A Zip-Code set opens read-only but still has a map.
    let dir = scratch("map");
    for (i, part) in sector64::write_zipcode(&sample_d64())
        .unwrap()
        .iter()
        .enumerate()
    {
        std::fs::write(dir.join(format!("{}!GAME", i + 1)), part).unwrap();
    }
    app.open(&dir.join("1!GAME"));
    assert!(app.pane.disk.is_none() && app.pane.read_only_disk);
    let map = app
        .pane
        .map_disk()
        .expect("read-only disk is mapped")
        .block_map();
    assert_eq!(map.files[0].name, "HELLO");
    // Opening a plain archive leaves nothing to map.
    app.pane.archive = Some(Vec::new());
    app.open(&dir.join("missing.d64"));
    assert!(app.pane.map_disk().is_none());
}

#[test]
fn disk_map_tooltips_and_summary() {
    let mut disk = Disk::format("TIPS", "01");
    disk.write_file("GAME", FileType::Prg, &[1u8; 600]).unwrap();
    let map = disk.block_map();
    let (t, s) = map.chains[0][1];
    assert_eq!(
        block_tooltip(&map, t, s),
        format!("Track {t}, sector {s}\n\"GAME\" PRG · block 2 of 3")
    );
    assert_eq!(
        block_tooltip(&map, 18, 0),
        "Track 18, sector 0\nDOS: header, BAM or directory"
    );
    assert_eq!(block_tooltip(&map, 35, 0), "Track 35, sector 0\nFree");
    assert_eq!(block_tooltip(&map, 99, 0), "");
    assert_eq!(
        map_summary(&map),
        "683 blocks · 3 in files · 2 DOS · 678 free"
    );

    // Point a second file's start at GAME's last block: both claim it.
    disk.write_file("COPY", FileType::Seq, &[2u8; 10]).unwrap();
    let mut bytes = disk.as_bytes().to_vec();
    let dir_off = (17 * 21 + 1) * 256; // track 18, sector 1
    let (lt, ls) = map.chains[0][2];
    bytes[dir_off + 32 + 3] = lt;
    bytes[dir_off + 32 + 4] = ls;
    let map = Disk::load(bytes).unwrap().block_map();
    let tip = block_tooltip(&map, lt, ls);
    assert!(
        tip.contains("⚠ also used by \"COPY\" SEQ · block 1 of 1"),
        "{tip}"
    );
    assert!(map_summary(&map).ends_with("· 1 allocated but unused · 1 in conflict"));
}

#[test]
fn zipcode_set_opens_from_any_part() {
    let d64 = sample_d64();
    let dir = scratch("zip");
    for (i, part) in sector64::write_zipcode(&d64).unwrap().iter().enumerate() {
        std::fs::write(dir.join(format!("{}!GAME", i + 1)), part).unwrap();
    }
    for n in 1..=4 {
        let got = zipcode_set(&dir.join(format!("{n}!GAME"))).expect("is a zip-code part");
        assert_eq!(got.unwrap(), d64, "opened via part {n}");
    }
    match open_path(&dir.join("3!GAME")).unwrap() {
        Opened::ReadOnlyDisk(title, nodes, _) => {
            assert!(
                title.contains("ZIP TEST") && title.contains("read-only"),
                "{title}"
            );
            assert!(nodes.iter().any(|n| n.name == "HELLO"));
        }
        _ => panic!("a Zip-Code set must open as a read-only disk"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn zipcode_set_reports_a_missing_part() {
    let dir = scratch("zipmissing");
    let parts = sector64::write_zipcode(&sample_d64()).unwrap();
    for i in [0, 1, 3] {
        std::fs::write(dir.join(format!("{}!GAME", i + 1)), &parts[i]).unwrap();
    }
    let err = zipcode_set(&dir.join("1!GAME")).unwrap().unwrap_err();
    assert!(err.contains("3!GAME"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn non_zipcode_names_are_ignored() {
    for name in ["disk.d64", "5!GAME", "1!", "!GAME", "x1!GAME"] {
        assert!(zipcode_set(Path::new(name)).is_none(), "{name}");
    }
}

/// The real sample set, when present (see the repo's tests/ for the convention).
#[test]
fn zipcode_sample_set() {
    let dir = match std::env::var("SECTOR64_SAMPLES") {
        Ok(p) => PathBuf::from(p),
        Err(_) => match std::env::var("HOME") {
            Ok(h) => PathBuf::from(h).join("Downloads/Sector64 Test Files"),
            Err(_) => return,
        },
    };
    let first = dir.join("1!ZIP-CODE-2");
    if !first.is_file() {
        eprintln!("skipping: Zip-Code sample not present");
        return;
    }
    match open_path(&first).unwrap() {
        Opened::ReadOnlyDisk(title, nodes, _) => {
            assert!(title.contains("Zip-Code"), "{title}");
            assert!(!nodes.is_empty());
        }
        _ => panic!("a Zip-Code set must open as a read-only disk"),
    }
}

#[test]
fn arrow_keys_stay_inside_the_filtered_listing() {
    // Up and Down step through the rows shown, and stop at the ends.
    let order = [4, 1, 7];
    assert_eq!(step_in_order(&order, None, true), Some(0));
    assert_eq!(step_in_order(&order, Some(1), true), Some(2));
    assert_eq!(step_in_order(&order, Some(7), true), Some(2));
    assert_eq!(step_in_order(&order, Some(4), false), Some(0));
    assert_eq!(step_in_order(&order, Some(9), false), Some(0));
    assert_eq!(step_in_order(&[], Some(3), true), None);

    // These used to index past the end of the filtered list and panic.
    let ctx = egui::Context::default();
    let mut app = app_with_disk();
    for name in ["ALPHA", "BETA", "GAMMA", "DELTA"] {
        app.add(name.into(), &[1, 2, 3]);
    }
    let press = |app: &mut App, key: egui::Key| {
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.frame_ui(ctx));
    };
    app.pane.filter = "zzz".into();
    app.pane.selected = Some(0);
    press(&mut app, egui::Key::ArrowDown);
    press(&mut app, egui::Key::ArrowUp);
    app.pane.filter = "ta".into(); // BETA and DELTA
    let beta = names(&app).iter().position(|n| n == "BETA").unwrap();
    let delta = names(&app).iter().position(|n| n == "DELTA").unwrap();
    app.pane.selected = Some(delta);
    press(&mut app, egui::Key::ArrowDown);
    assert_eq!(
        app.pane.selected,
        Some(delta),
        "stays on the last row shown"
    );
    press(&mut app, egui::Key::ArrowUp);
    assert_eq!(app.pane.selected, Some(beta));
}

#[test]
fn select_all_marks_the_files_the_filter_shows() {
    let mut app = app_with_disk();
    for name in ["ALPHA", "BETA", "GAMMA", "DELTA"] {
        app.add(name.into(), &[1, 2, 3]);
    }
    app.pane.filter = "ta".into();
    app.apply(Act::SelectAll);
    let marked: Vec<String> = app
        .pane
        .marked
        .iter()
        .map(|&i| app.pane.nodes[i].name.clone())
        .collect();
    assert_eq!(marked.len(), 2);
    assert!(marked.contains(&"BETA".to_string()) && marked.contains(&"DELTA".to_string()));
    assert!(app.pane.marked.contains(&app.pane.selected.unwrap()));
    app.pane.filter.clear();
    app.apply(Act::SelectAll);
    assert_eq!(app.pane.marked.len(), 4);
}

#[test]
fn help_shows_the_manuals_shortcut_section() {
    let keys = manual_section("KEYBOARD SHORTCUTS");
    assert!(keys.contains("Ctrl+O") && keys.contains("F1"), "{keys}");
    assert!(
        !keys.contains("KEYBOARD SHORTCUTS"),
        "the heading is left out"
    );
    assert!(
        !keys.contains("SETTINGS THAT PERSIST"),
        "stops at the next section"
    );
    assert_eq!(manual_section("NO SUCH SECTION"), "");
}

/// Every text the UI drew in `out`, with where it was drawn.
fn drawn_texts(out: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn walk(shape: &egui::Shape, acc: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => {
                acc.push((t.galley.text().to_string(), t.visual_bounding_rect()))
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, acc)),
            _ => {}
        }
    }
    let mut acc = Vec::new();
    for cs in &out.shapes {
        walk(&cs.shape, &mut acc);
    }
    acc
}

#[test]
fn a_new_archive_with_files_counts_as_unsaved_until_saved() {
    let mut app = App::default();
    app.apply(Act::NewArchive);
    assert!(!app.pane.is_dirty(), "an empty archive has nothing to lose");
    app.add("ONE".into(), &[1, 8, 0]);
    assert!(app.pane.is_dirty());
    assert!(app.pane.title.ends_with("(unsaved)"), "{}", app.pane.title);
    assert_eq!(
        unsaved_message(&[app.pane.unsaved_name(true, false)]),
        "The new archive has unsaved changes."
    );

    // Open, New disk and New archive all ask first.
    app.apply(Act::OpenDropped(PathBuf::from("x.d64"), 0));
    assert!(matches!(app.confirm, Some(Confirm::OpenFile(_))));
    for act in [Act::Create, Act::NewArchive] {
        app.confirm = None;
        app.apply(act);
        assert!(app.confirm.is_some());
        assert_eq!(app.pane.nodes.len(), 1, "nothing replaced yet");
    }
    app.confirm = None;

    // Once saved (as save_archive records it), it's clean, until it
    // changes; undoing back to what was saved makes it clean again.
    app.pane.archive_saved = app.pane.archive.clone();
    app.pane.refresh();
    assert!(!app.pane.is_dirty());
    assert!(app.pane.title.ends_with("(saved)"), "{}", app.pane.title);
    app.add("TWO".into(), &[1, 8, 1]);
    assert!(app.pane.is_dirty());
    app.apply(Act::Undo);
    assert!(!app.pane.is_dirty());

    // In dual view the prompt says which pane's archive.
    assert_eq!(app.pane.unsaved_name(true, true), "this pane's new archive");
    assert_eq!(
        app.pane.unsaved_name(false, true),
        "the other pane's new archive"
    );
    assert_eq!(
        App::default().pane.unsaved_name(false, true),
        "the other pane's disk"
    );
}

#[test]
fn ctrl_n_opens_the_new_disk_dialog_and_enter_creates_the_disk() {
    let ctx = egui::Context::default();
    load_c64_font(&ctx);
    apply_c64_theme(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(520.0, 640.0));
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    let frame = |app: &mut App, events: Vec<egui::Event>| {
        let modifiers = match events.first() {
            Some(egui::Event::Key { modifiers, .. }) => *modifiers,
            _ => egui::Modifiers::NONE,
        };
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            modifiers,
            ..Default::default()
        };
        ctx.run(input, |ctx| app.frame_ui(ctx))
    };
    let mut app = App::default();
    let _ = frame(&mut app, vec![key(egui::Key::N, egui::Modifiers::COMMAND)]);
    assert!(app.show_new_disk);
    // It fits in a narrow window, with the cursor in the name field.
    // (egui takes a few frames to size a new window and centre it.)
    for _ in 0..3 {
        let _ = frame(&mut app, vec![]);
    }
    let out = frame(&mut app, vec![]);
    let texts = drawn_texts(&out);
    for want in [
        "NEW DISK IMAGE",
        "Disk name",
        "D81  1581, 3160 blocks free",
        "Create",
    ] {
        let (_, rect) = texts
            .iter()
            .find(|(t, _)| t == want)
            .unwrap_or_else(|| panic!("{want:?} isn't drawn"));
        assert!(screen.contains_rect(*rect), "{want:?} at {rect:?}");
        assert!(rect.height() < 24.0, "{want:?} wraps: {rect:?}");
    }
    assert!(ctx.wants_keyboard_input(), "the name field has the cursor");

    // Esc closes it and makes nothing.
    let _ = frame(
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!app.show_new_disk && app.pane.disk.is_none());

    // Enter creates the disk as set up.
    let _ = frame(&mut app, vec![key(egui::Key::N, egui::Modifiers::COMMAND)]);
    app.new_name = "GAMES".into();
    app.new_id = "G1".into();
    app.new_fmt = 2;
    let _ = frame(&mut app, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    assert!(!app.show_new_disk);
    let ed = app.pane.disk.as_ref().expect("a disk was created");
    let header = ed.disk.header().unwrap();
    assert_eq!((header.name.as_str(), header.id.as_str()), ("GAMES", "G1"));
    assert_eq!(ed.disk.track_count(), 80, "a D81");
    assert!(ed.dirty);
    assert!(
        app.status.starts_with("Created a blank D81 image"),
        "{}",
        app.status
    );
}

#[test]
fn new_disk_formats_say_how_many_blocks_are_free() {
    for (i, (_, about)) in NEW_DISK_FORMATS.iter().enumerate() {
        let mut app = App {
            new_fmt: i,
            ..Default::default()
        };
        app.apply(Act::Create);
        let free = app.pane.disk.as_ref().unwrap().disk.blocks_free().unwrap();
        assert!(
            about.ends_with(&format!(" {free} blocks free")),
            "{about}: {free}"
        );
    }
}

#[test]
fn a_new_disk_or_archive_asks_before_replacing_unsaved_changes() {
    for act in [Act::Create, Act::NewArchive] {
        let mut app = app_with_disk();
        app.add("KEEP ME".into(), &[1, 8, 0]);
        assert!(app.pane.is_dirty());
        app.apply(act);
        assert!(matches!(
            app.confirm,
            Some(Confirm::NewDisk | Confirm::NewArchive)
        ));
        assert_eq!(app.pane.nodes.len(), 1, "nothing replaced yet");
    }
}

#[test]
fn the_title_gives_way_to_the_menus_in_a_narrow_window() {
    for (w, title) in [(1100.0, true), (520.0, false)] {
        let ctx = egui::Context::default();
        load_c64_font(&ctx);
        apply_c64_theme(&ctx);
        let mut app = app_with_disk();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w, 640.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input(), |ctx| app.frame_ui(ctx));
        let out = ctx.run(input(), |ctx| app.frame_ui(ctx));
        let texts = drawn_texts(&out);
        let shown = |name: &str| {
            texts
                .iter()
                .any(|(t, r)| t == name && r.max.x <= w && r.min.y < 40.0)
        };
        assert_eq!(shown("SECTOR64"), title, "{w} px wide");
        for menu in ["File", "Edit", "View", "Tools", "Help"] {
            assert!(shown(menu), "{w} px wide: {menu} is cut off");
        }
    }
}

#[test]
fn every_menu_opens_whole_inside_a_narrow_window() {
    let menus: [(&str, &[&str]); 5] = [
        ("File", &["New ▸", "Open…", "Save As…", "Extract All"]),
        (
            "Edit",
            &["Undo", "Redo", "Rename", "Select All", "Copy to Other Pane"],
        ),
        ("View", &["Preview", "Dual Panes", "Disk Map"]),
        (
            "Tools",
            &["Validate / Repair", "Find in Files…", "Run in VICE"],
        ),
        ("Help", &["Manual", "Keyboard Shortcuts", "About Sector64"]),
    ];
    let (w, h) = (520.0, 640.0);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
    for (menu, items) in menus {
        let ctx = egui::Context::default();
        load_c64_font(&ctx);
        apply_c64_theme(&ctx);
        let mut app = app_with_disk();
        app.add("HELLO".into(), &[1, 2, 3]);
        let frame = |app: &mut App, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| app.frame_ui(ctx))
        };
        // The first frame measures the menus, the second lays them out.
        frame(&mut app, vec![]);
        let out = frame(&mut app, vec![]);
        let (_, at) = drawn_texts(&out)
            .into_iter()
            .find(|(t, _)| t == menu)
            .unwrap_or_else(|| panic!("no {menu} menu in the bar"));
        let pos = at.center();
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut app, vec![egui::Event::PointerMoved(pos), button(true)]);
        frame(&mut app, vec![button(false)]);
        let out = frame(&mut app, vec![]);
        let texts = drawn_texts(&out);
        for item in items {
            let (_, r) = texts
                .iter()
                .find(|(t, _)| t == item)
                .unwrap_or_else(|| panic!("{menu} has no \"{item}\""));
            assert!(
                screen.contains_rect(*r),
                "{menu} > {item} is drawn at {r:?}, outside the {w}x{h} window"
            );
        }
    }
}

#[test]
fn ctrl_k_turns_dual_panes_on_and_off() {
    let ctx = egui::Context::default();
    let mut app = app_with_disk();
    app.show_preview = true;
    let ctrl_k = |app: &mut App| {
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::K,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            modifiers: egui::Modifiers::COMMAND,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.frame_ui(ctx));
    };
    ctrl_k(&mut app);
    assert!(
        app.dual && !app.show_preview,
        "Dual Panes replaces the preview"
    );
    ctrl_k(&mut app);
    assert!(!app.dual);
}

#[test]
fn esc_closes_one_window_per_press_front_first() {
    let ctx = egui::Context::default();
    let esc = |app: &mut App| {
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.frame_ui(ctx));
    };
    let open = |app: &App| {
        [
            ("help", app.show_help),
            ("sector", app.show_sector),
            ("map", app.show_map),
            ("find", app.show_find),
        ]
        .into_iter()
        .filter(|w| w.1)
        .map(|w| w.0)
        .collect::<Vec<_>>()
    };
    let mut app = app_with_disk();
    (app.show_help, app.show_sector, app.show_map, app.show_find) = (true, true, true, true);
    for left in [
        &["sector", "map", "find"][..],
        &["map", "find"],
        &["find"],
        &[],
    ] {
        esc(&mut app);
        assert_eq!(open(&app), left);
    }

    // With the cursor in the BASIC editor's text, Esc only leaves the text.
    let mut app = app_with_disk();
    (app.show_basic, app.show_find) = (true, true);
    app.basic.typing = true;
    esc(&mut app);
    assert!(app.show_basic && app.show_find);
    app.basic.typing = false;
    esc(&mut app);
    assert!(!app.show_basic && app.show_find);
}
