//! Formatting (document, selection, on save) and read-only editors with a
//! scripted `LanguageServices` provider.

mod common;

use std::time::Duration;

use arcode_buffer::Point;
use common::lang_fake::*;
use common::*;
use gpui::TestAppContext;

#[gpui::test]
fn format_document_keeps_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, CODE);
    let log = install_services(&editor, false, cx);
    move_to(&editor, Point::new(2, 12), cx);
    cx.simulate_keystrokes("shift-alt-f");
    cx.run_until_parked();
    assert_eq!(
        text(&editor, cx),
        "fn main() {\n    let x = 1;\n    let y = x;\n}\n"
    );
    assert_eq!(
        editor.read_with(cx, |e, _| e.cursor_position()),
        Point::new(2, 12)
    );
    // One undo step.
    editor.update(cx, |e, cx| e.undo(cx));
    assert_eq!(text(&editor, cx), CODE);
    // Format selection asks for the cursor line.
    cx.simulate_keystrokes("secondary-k secondary-f");
    cx.run_until_parked();
    assert_eq!(
        log.borrow().formats.last().cloned().flatten(),
        Some(Point::new(2, 0)..Point::new(2, 14))
    );
}

#[gpui::test]
fn format_on_save_never_hangs(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "").unwrap();
    let mut config = config();
    config.editor.format_on_save = true;
    config.editor.format_on_save_timeout_ms = 200;
    let (editor, cx) = open_at(cx, CODE, path.to_str().unwrap(), config);
    let log = install_services(&editor, true, cx);
    cx.simulate_keystrokes("secondary-s");
    cx.run_until_parked();
    assert!(editor.read_with(cx, |e, _| e.is_saving()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    cx.executor().advance_clock(Duration::from_millis(250));
    cx.run_until_parked();
    assert!(!editor.read_with(cx, |e, _| e.is_saving()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), CODE);
    assert_eq!(log.borrow().formats.len(), 1);
}

#[gpui::test]
fn format_on_save_formats_first(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    let mut config = config();
    config.editor.format_on_save = true;
    let (editor, cx) = open_at(cx, CODE, path.to_str().unwrap(), config);
    install_services(&editor, false, cx);
    cx.simulate_keystrokes("secondary-s");
    cx.run_until_parked();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("let x = 1;"), "{saved}");
    assert!(!editor.read_with(cx, |e, _| e.is_dirty()));
}

#[gpui::test]
fn read_only_files_offer_no_edits(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, CODE);
    let log = install_services(&editor, false, cx);
    editor.update(cx, |e, cx| e.set_read_only(true, cx));
    move_to(&editor, Point::new(1, 6), cx);
    settle(cx);
    assert_eq!(editor.read_with(cx, |e, _| e.lightbulb_line()), None);
    for keys in ["secondary-.", "f2"] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        let notice = editor.read_with(cx, |e, cx| e.notice_text(cx));
        assert_eq!(notice.as_deref(), Some("The file is read-only"), "{keys}");
        cx.simulate_keystrokes("escape");
    }
    cx.simulate_keystrokes("shift-alt-f");
    cx.run_until_parked();
    assert!(log.borrow().formats.is_empty());
    assert_eq!(text(&editor, cx), CODE);
}
