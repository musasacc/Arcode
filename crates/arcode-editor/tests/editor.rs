//! Editor behavior through GPUI's test platform.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use arcode_buffer::{Buffer, Point, Selection, SelectionSet};
use arcode_config::{Config, ThemeRegistry};
use arcode_editor::{DiagnosticSeverity, Editor, EditorDiagnostic, EditorEvent};
use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(Config::default(), cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry
            .get("Arcode Dark")
            .cloned()
            .unwrap_or_else(|| registry.fallback(arcode_config::Appearance::Dark).clone());
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        arcode_editor::init(cx);
    });
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<Editor>, &'a mut VisualTestContext, Rc<RefCell<Vec<EditorEvent>>>) {
    setup(cx);
    let text = text.to_string();
    let (editor, cx) = cx.add_window_view(move |_, cx| {
        let mut buffer = Buffer::new(&text);
        buffer.set_path("test.rs");
        Editor::new(buffer, cx)
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|window, cx| {
        window.focus(&editor.focus_handle(cx));
        cx.subscribe(&editor, move |_, e: &EditorEvent, _| sink.borrow_mut().push(e.clone()))
            .detach();
    });
    cx.run_until_parked();
    (editor, cx, events)
}

fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.buffer().to_string())
}

#[gpui::test]
fn typing_inserts_text(cx: &mut TestAppContext) {
    let (editor, cx, events) = open(cx, "");
    cx.simulate_input("fn x");
    assert_eq!(text(&editor, cx), "fn x");
    cx.simulate_input("(");
    assert_eq!(text(&editor, cx), "fn x()");
    assert_eq!(editor.read_with(cx, |e, _| e.cursor_position()), Point::new(0, 5));
    let edited: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|e| match e {
            EditorEvent::Edited { edits, version } => Some((edits.clone(), *version)),
            _ => None,
        })
        .collect();
    assert_eq!(edited.len(), 5);
    assert_eq!(edited[0].0.len(), 1);
    assert_eq!(edited[0].0[0].new_text, "f");
    assert_eq!(edited[0].0[0].start_byte, 0);
    assert_eq!(edited[4].0[0].new_text, "()");
    assert_eq!(edited[4].1, editor.read_with(cx, |e, _| e.buffer().version()));
    assert!(events.borrow().contains(&EditorEvent::DirtyChanged(true)));
}

#[gpui::test]
fn multi_cursor_typing_and_add_cursor_below(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "ab\ncd\nef\n");
    editor.update(cx, |e, cx| {
        let set = SelectionSet::from_vec(vec![Selection::cursor(1), Selection::cursor(4)], 0)
            .expect("set");
        e.set_selections(set, cx);
    });
    cx.simulate_input("X");
    assert_eq!(text(&editor, cx), "aXb\ncXd\nef\n");
    cx.simulate_keystrokes("escape");
    assert_eq!(editor.read_with(cx, |e, _| e.selections().len()), 1);
    cx.simulate_keystrokes("alt-shift-down");
    assert_eq!(editor.read_with(cx, |e, _| e.selections().len()), 2);
    cx.simulate_input("-");
    assert_eq!(text(&editor, cx), "aX-b\ncX-d\nef\n");
}

#[gpui::test]
fn undo_and_redo(cx: &mut TestAppContext) {
    let (editor, cx, events) = open(cx, "hello");
    cx.simulate_keystrokes("end");
    cx.simulate_input(" world");
    assert_eq!(text(&editor, cx), "hello world");
    events.borrow_mut().clear();
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&editor, cx), "hello");
    assert!(!editor.read_with(cx, |e, _| e.is_dirty()));
    let undo_edits: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|e| match e {
            EditorEvent::Edited { edits, .. } => Some(edits.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(undo_edits.len(), 1);
    assert_eq!(undo_edits[0][0].range, 5..11);
    assert_eq!(undo_edits[0][0].new_text, "");
    assert!(events.borrow().contains(&EditorEvent::DirtyChanged(false)));
    cx.simulate_keystrokes("secondary-shift-z");
    assert_eq!(text(&editor, cx), "hello world");
    cx.simulate_keystrokes("secondary-z secondary-y");
    assert_eq!(text(&editor, cx), "hello world");
}

#[gpui::test]
fn select_next_occurrence(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "foo bar foo baz foo");
    cx.simulate_keystrokes("secondary-d");
    let ranges = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| e.selections().iter().map(|s| s.range()).collect::<Vec<_>>())
    };
    assert_eq!(ranges(cx), vec![0..3]);
    cx.simulate_keystrokes("secondary-d secondary-d");
    assert_eq!(ranges(cx), vec![0..3, 8..11, 16..19]);
    cx.simulate_input("x");
    assert_eq!(text(&editor, cx), "x bar x baz x");
}

#[gpui::test]
fn go_to_moves_and_scrolls(cx: &mut TestAppContext) {
    let body: String = (0..500).map(|i| format!("line {i}\n")).collect();
    let (editor, cx, _) = open(cx, &body);
    assert_eq!(editor.read_with(cx, |e, _| e.visible_line_range().start), 0);
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| e.go_to(Point::new(300, 3), window, cx));
    });
    cx.run_until_parked();
    let (pos, visible) = editor.read_with(cx, |e, _| (e.cursor_position(), e.visible_line_range()));
    assert_eq!(pos, Point::new(300, 3));
    assert!(visible.contains(&300), "line 300 not in {visible:?}");
    assert!(visible.start > 200, "not centered: {visible:?}");
    // Out-of-range points are clamped.
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| e.go_to(Point::new(9999, 99), window, cx));
    });
    assert_eq!(editor.read_with(cx, |e, _| e.cursor_position()), Point::new(500, 0));
}

#[gpui::test]
fn diagnostics_follow_edits(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "fn a() {}\nlet x = y;\nlet z = 1;\n");
    let diag = |line, a, b, message: &str| EditorDiagnostic {
        range: Point::new(line, a)..Point::new(line, b),
        severity: DiagnosticSeverity::Error,
        message: message.into(),
        source: "test".into(),
    };
    editor.update(cx, |e, cx| {
        e.set_diagnostics(vec![diag(1, 8, 9, "y"), diag(2, 4, 5, "z")], cx)
    });
    // A new line above shifts both.
    cx.simulate_keystrokes("enter");
    let ranges = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| {
            e.diagnostics().iter().map(|d| d.range.clone()).collect::<Vec<_>>()
        })
    };
    assert_eq!(
        ranges(cx),
        vec![Point::new(2, 8)..Point::new(2, 9), Point::new(3, 4)..Point::new(3, 5)]
    );
    // Replacing the `y` drops that diagnostic only.
    cx.update(|window, cx| editor.update(cx, |e, cx| e.go_to(Point::new(2, 8), window, cx)));
    cx.simulate_keystrokes("shift-right");
    cx.simulate_input("w");
    assert_eq!(ranges(cx), vec![Point::new(3, 4)..Point::new(3, 5)]);
    assert!(text(&editor, cx).contains("let x = w;"));
}

#[gpui::test]
fn copy_line_and_paste(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "one\ntwo\n");
    cx.simulate_keystrokes("secondary-c");
    let clip = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(clip.as_deref(), Some("one\n"));
    cx.simulate_keystrokes("down secondary-v");
    assert_eq!(text(&editor, cx), "one\none\ntwo\n");
    cx.simulate_keystrokes("secondary-x");
    assert_eq!(text(&editor, cx), "one\none\n");
}

#[gpui::test]
fn line_commands(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "a\nb\n");
    cx.simulate_keystrokes("secondary-/");
    assert_eq!(text(&editor, cx), "// a\nb\n");
    cx.simulate_keystrokes("secondary-shift-d");
    assert_eq!(text(&editor, cx), "// a\n// a\nb\n");
    cx.simulate_keystrokes("alt-down");
    assert_eq!(text(&editor, cx), "// a\nb\n// a\n");
    cx.simulate_keystrokes("tab");
    assert_eq!(text(&editor, cx), "// a\nb\n    // a\n");
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(text(&editor, cx), "// a\nb\n// a\n");
}

#[gpui::test]
fn read_only_blocks_edits(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "keep");
    editor.update(cx, |e, cx| e.set_read_only(true, cx));
    cx.simulate_input("x");
    cx.simulate_keystrokes("backspace delete enter");
    assert_eq!(text(&editor, cx), "keep");
}
