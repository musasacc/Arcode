//! Programmatic editor APIs through GPUI's test platform: multi-range
//! edits, snippets, highlights following edits, scrolling, large files.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use arcode_buffer::{Buffer, Point};
use arcode_config::{Config, ThemeRegistry};
use arcode_editor::{
    BackgroundHighlight, ColorKey, Editor, EditorEvent, HighlightRange, ScrollAnchor,
};
use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};

type Events = Rc<RefCell<Vec<EditorEvent>>>;

fn open_with<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    config: Config,
) -> (Entity<Editor>, &'a mut VisualTestContext, Events) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(config, cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        arcode_editor::init(cx);
    });
    let text = text.to_string();
    let (editor, cx) = cx.add_window_view(move |_, cx| {
        let mut buffer = Buffer::new(&text);
        buffer.set_path("test.rs");
        Editor::new(buffer, cx)
    });
    let events: Events = Rc::default();
    let sink = events.clone();
    cx.update(|window, cx| {
        window.focus(&editor.focus_handle(cx));
        cx.subscribe(&editor, move |_, e: &EditorEvent, _| {
            sink.borrow_mut().push(e.clone())
        })
        .detach();
    });
    cx.run_until_parked();
    (editor, cx, events)
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<Editor>, &'a mut VisualTestContext, Events) {
    open_with(cx, text, Config::default())
}

fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.buffer().to_string())
}

fn cursor(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Point {
    editor.read_with(cx, |e, _| e.cursor_position())
}

#[gpui::test]
fn apply_edits_is_one_undo_step(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "alpha beta\ngamma\n");
    cx.simulate_keystrokes("down end");
    let changed = editor.update(cx, |e, cx| {
        e.apply_edits(
            vec![
                (Point::new(0, 6)..Point::new(0, 10), "B".into()),
                (Point::new(0, 0)..Point::new(0, 5), "A".into()),
                (Point::new(1, 0)..Point::new(1, 0), ">> ".into()),
            ],
            cx,
        )
    });
    assert!(changed);
    assert_eq!(text(&editor, cx), "A B\n>> gamma\n");
    assert_eq!(cursor(&editor, cx), Point::new(1, 8));
    let word = editor.read_with(cx, |e, _| e.word_at(Point::new(1, 5)));
    assert_eq!(
        word,
        Some((Point::new(1, 3)..Point::new(1, 8), "gamma".to_string()))
    );
    let slice = editor.read_with(cx, |e, _| {
        e.text_in_range(Point::new(0, 2)..Point::new(1, 2))
    });
    assert_eq!(slice, "B\n>>");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&editor, cx), "alpha beta\ngamma\n");
}

#[gpui::test]
fn output_is_appended_to_a_read_only_editor(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "one\n");
    editor.update(cx, |e, cx| e.set_read_only(true, cx));
    // The cursor at the start stays there.
    editor.update(cx, |e, cx| e.append_output("two\n", cx));
    assert_eq!(text(&editor, cx), "one\ntwo\n");
    assert_eq!(cursor(&editor, cx), Point::new(0, 0));
    assert!(!editor.read_with(cx, |e, _| e.is_dirty()));
    // A cursor at the end follows the output.
    cx.simulate_keystrokes("secondary-end");
    editor.update(cx, |e, cx| e.append_output("three\n", cx));
    assert_eq!(cursor(&editor, cx), Point::new(3, 0));
    assert!(editor.read_with(cx, |e, _| e.is_read_only() && !e.is_dirty()));
    // Typing is still blocked.
    cx.simulate_input("x");
    assert_eq!(text(&editor, cx), "one\ntwo\nthree\n");
}

#[gpui::test]
fn an_edit_group_is_one_undo_step(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "one\ntwo\n");
    cx.simulate_input("x");
    assert_eq!(text(&editor, cx), "xone\ntwo\n");
    editor.update(cx, |e, cx| {
        e.begin_edit_group();
        // The second edit is computed against the text the first left.
        e.apply_edits(
            vec![(Point::new(0, 0)..Point::new(0, 0), "use a;\n".into())],
            cx,
        );
        e.apply_edits(vec![(Point::new(1, 0)..Point::new(1, 4), "ONE".into())], cx);
        e.end_edit_group();
    });
    assert_eq!(text(&editor, cx), "use a;\nONE\ntwo\n");
    // Typing afterwards is a step of its own.
    cx.simulate_input("y");
    assert_eq!(text(&editor, cx), "use a;\nONEy\ntwo\n");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&editor, cx), "use a;\nONE\ntwo\n");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&editor, cx), "xone\ntwo\n", "the group is one step");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&editor, cx), "one\ntwo\n");
}

#[gpui::test]
fn snippet_tabstops(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "\n");
    editor.update(cx, |e, cx| {
        e.insert_snippet(
            Point::new(0, 0)..Point::new(0, 0),
            "fn ${1:name}($2) {$0}",
            cx,
        );
    });
    let sel = editor.read_with(cx, |e, _| e.selections().primary().range());
    assert_eq!(sel, 3..7);
    cx.simulate_input("go");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("x: u8");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("1");
    assert_eq!(text(&editor, cx), "fn go(x: u8) {1}\n");
    assert!(!editor.read_with(cx, |e, _| e.snippet_active()));
    // Tab indents again once the snippet is done.
    cx.simulate_keystrokes("tab");
    assert!(text(&editor, cx).starts_with("fn go(x: u8) {1 "));
}

#[gpui::test]
fn highlights_follow_edits_and_scroll_to_line(cx: &mut TestAppContext) {
    let body: String = (0..400).map(|i| format!("line {i}\n")).collect();
    let (editor, cx, _) = open(cx, &body);
    editor.update(cx, |e, cx| {
        let hl = vec![
            BackgroundHighlight::lines(2..4, ColorKey::new("warning").opacity(0.2)),
            BackgroundHighlight::text(Point::new(5, 0)..Point::new(5, 4), "selection".into()),
        ];
        e.set_background_highlights("conflicts", hl, cx);
        e.scroll_to_line(300, ScrollAnchor::Top, cx);
    });
    cx.run_until_parked();
    let visible = editor.read_with(cx, |e, _| e.visible_line_range());
    assert_eq!(visible.start, 300);
    cx.simulate_keystrokes("enter");
    let ranges: Vec<HighlightRange> = editor.read_with(cx, |e, _| {
        e.background_highlights("conflicts")
            .iter()
            .map(|h| h.range.clone())
            .collect()
    });
    assert_eq!(
        ranges,
        vec![
            HighlightRange::Lines(3..5),
            HighlightRange::Text(Point::new(6, 0)..Point::new(6, 4)),
        ]
    );
    editor.update(cx, |e, cx| e.clear_decorations("conflicts", cx));
    assert!(editor.read_with(cx, |e, _| e.background_highlights("conflicts").is_empty()));
}

#[gpui::test]
fn large_files_skip_the_full_tree(cx: &mut TestAppContext) {
    let mut config = Config::default();
    config.editor.large_file_lines = 1000;
    let body: String = (0..5000)
        .map(|i| format!("fn f{i}() {{ let x = {i}; }}\n"))
        .collect();
    let (editor, cx, _) = open_with(cx, &body, config);
    assert!(editor.read_with(cx, |e, _| e.is_large_file()));
    editor.update(cx, |e, cx| e.scroll_to_line(4000, ScrollAnchor::Center, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("secondary-end");
    cx.simulate_input("// end");
    cx.run_until_parked();
    assert!(text(&editor, cx).ends_with("}\n// end"));
}
