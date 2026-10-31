//! Vim mode: read-only buffers, `:w` messages, the app-wide toggle and
//! search highlights in large files.

mod common;

use arcode_buffer::Point;
use arcode_config::Config;
use arcode_editor::{Editor, ToggleVimMode};
use arcode_vim::{Mode, READ_ONLY_MESSAGE};
use common::{open_at, text};
use gpui::{Entity, TestAppContext, VisualTestContext};

fn vim_config() -> Config {
    let mut config = common::config();
    config.editor.vim_mode = true;
    config
}

fn open<'a>(cx: &'a mut TestAppContext, text: &str) -> (Entity<Editor>, &'a mut VisualTestContext) {
    let (editor, cx) = open_at(cx, text, "test.rs", vim_config());
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(0, 0)..Point::new(0, 0)], cx)
    });
    cx.run_until_parked();
    (editor, cx)
}

fn message(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<String> {
    editor.read_with(cx, |e, _| e.vim_status().and_then(|s| s.message))
}

#[gpui::test]
fn read_only_changes_are_refused_with_a_message(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "abc\ndef");
    editor.update(cx, |e, cx| e.set_read_only(true, cx));
    for keys in [
        "x",
        "d d",
        "3 i x escape",
        "u",
        "ctrl-r",
        ". ",
        ": s / a / b / enter",
    ] {
        cx.simulate_keystrokes(keys);
        assert_eq!(text(&editor, cx), "abc\ndef", "{keys}");
        assert_eq!(
            editor.read_with(cx, |e, _| e.vim_mode()),
            Some(Mode::Normal),
            "{keys}"
        );
    }
    assert_eq!(message(&editor, cx).as_deref(), Some(READ_ONLY_MESSAGE));
    // Motions and yanks still work.
    cx.simulate_keystrokes("j y y");
    assert_eq!(
        editor.read_with(cx, |e, _| e.cursor_position()),
        Point::new(1, 0)
    );
    assert!(!editor.read_with(cx, |e, _| e.is_dirty()));
}

#[gpui::test]
fn write_reports_the_result_of_the_save(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    std::fs::write(&path, "abc\n").unwrap();
    let (editor, cx) = open(cx, "abc\n");
    editor.update(cx, |e, cx| e.set_path(&path, cx));
    cx.simulate_keystrokes("x : w enter");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "bc\n");
    assert_eq!(message(&editor, cx).as_deref(), Some("\"a.rs\" written"));
    // A file that cannot be written gives an error, not "written".
    let missing = dir.path().join("no/such/dir/b.rs");
    editor.update(cx, |e, cx| e.set_path(&missing, cx));
    cx.simulate_keystrokes("x : w enter");
    cx.run_until_parked();
    let msg = message(&editor, cx).unwrap_or_default();
    assert!(msg.starts_with("E212"), "{msg}");
}

#[gpui::test]
fn toggle_vim_mode_works_without_a_focused_editor(cx: &mut TestAppContext) {
    let (editor, cx) = open_at(cx, "abc", "t.rs", common::config());
    assert_eq!(editor.read_with(cx, |e, _| e.vim_mode()), None);
    cx.update(|window, _| window.blur());
    cx.dispatch_action(ToggleVimMode);
    cx.run_until_parked();
    assert_eq!(
        editor.read_with(cx, |e, _| e.vim_mode()),
        Some(Mode::Normal)
    );
    cx.dispatch_action(ToggleVimMode);
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |e, _| e.vim_mode()), None);
}

#[gpui::test]
fn search_highlights_follow_the_viewport_in_large_files(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, &"a b\n".repeat(5_000));
    let highlights = |editor: &Entity<Editor>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| {
            let lines: Vec<usize> = e
                .background_highlights("vim-search")
                .iter()
                .filter_map(|h| match &h.range {
                    arcode_editor::HighlightRange::Text(r) => Some(r.start.line),
                    _ => None,
                })
                .collect();
            lines
        })
    };
    cx.simulate_keystrokes("/ a enter");
    cx.run_until_parked();
    let lines = highlights(&editor, cx);
    assert!(!lines.is_empty());
    assert!(
        lines.len() < 2_000,
        "only the lines around the view: {}",
        lines.len()
    );
    cx.simulate_keystrokes("shift-g");
    cx.run_until_parked();
    let lines = highlights(&editor, cx);
    assert!(
        lines.iter().any(|&l| l >= 4_990),
        "matches at the end are highlighted after G"
    );
    cx.simulate_keystrokes(": n o h enter");
    assert!(highlights(&editor, cx).is_empty());
}
