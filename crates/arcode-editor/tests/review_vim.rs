//! Review tests for Vim mode and the multi-cursor extras. Each test
//! reproduces one bug found in review; they fail until it is fixed.

mod common;

use std::sync::Arc;

use arcode_buffer::Point;
use arcode_config::Config;
use arcode_editor::Editor;
use common::{open_at, text};
use gpui::{
    AppContext, Entity, Focusable, IntoElement, ParentElement, Styled, TestAppContext,
    VisualTestContext, div,
};

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

fn dirty(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |e, _| e.is_dirty())
}

/// `u` in a read-only output view (the language-server log, filled with
/// `append_output`) removes logged output: Vim's undo removes the last
/// chunk, then the read-only guard calls `buffer.undo()` again instead of
/// taking Vim's undo back, removing the chunk before it too.
#[gpui::test]
fn review_u_in_read_only_log_keeps_the_output(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "log\n");
    editor.update(cx, |e, cx| {
        e.set_read_only(true, cx);
        e.append_output("one\n", cx);
        e.append_output("two\n", cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("u");
    assert_eq!(
        text(&editor, cx),
        "log\none\ntwo\n",
        "a read-only editor must not lose text on `u`"
    );
}

/// A counted insert in a read-only editor: the first typed char is taken
/// back, but `force_normal` sends Escape, which replays the insert
/// `count - 1` times. Those edits are dropped from the edit log
/// (`take_edits`) and never reverted, so the read-only buffer is changed
/// (and the editor is not told: no syntax/LSP update).
#[gpui::test]
fn review_counted_insert_in_read_only_editor(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "abc");
    editor.update(cx, |e, cx| e.set_read_only(true, cx));
    cx.simulate_keystrokes("3 i x");
    assert_eq!(
        text(&editor, cx),
        "abc",
        "3ix must not edit a read-only buffer"
    );
    assert!(!dirty(&editor, cx));
}

/// Same with `3o`: the reverted `o` leaves an Insert session with a count;
/// Escape (from `force_normal`) opens two more lines.
#[gpui::test]
fn review_counted_open_line_in_read_only_editor(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "abc\ndef");
    editor.update(cx, |e, cx| e.set_read_only(true, cx));
    cx.simulate_keystrokes("3 o");
    assert_eq!(
        text(&editor, cx),
        "abc\ndef",
        "3o must not edit a read-only buffer"
    );
    assert!(!dirty(&editor, cx));
}

/// Two editors in one window.
struct Two {
    a: Entity<Editor>,
    b: Entity<Editor>,
}

impl gpui::Render for Two {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(div().w_1_2().h_full().child(self.a.clone()))
            .child(div().w_1_2().h_full().child(self.b.clone()))
    }
}

/// A macro with two changes, recorded in a writable editor and replayed in
/// a read-only one (macros are shared): only the last undo group is taken
/// back, so the first change stays in the read-only buffer.
#[gpui::test]
fn review_macro_replay_in_read_only_editor(cx: &mut TestAppContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(vim_config(), cx);
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        arcode_editor::init(cx);
    });
    let (two, cx) = cx.add_window_view(|_, cx| {
        let editor = |text: &str, cx: &mut gpui::Context<Two>| {
            let text = text.to_string();
            cx.new(|cx| Editor::new(arcode_buffer::Buffer::new(&text), cx))
        };
        Two {
            a: editor("abc\nabc", cx),
            b: editor("xyz\nxyz", cx),
        }
    });
    let (a, b) = two.read_with(cx, |t, _| (t.a.clone(), t.b.clone()));
    b.update(cx, |e, cx| e.set_read_only(true, cx));
    cx.update(|window, cx| window.focus(&a.focus_handle(cx)));
    cx.run_until_parked();
    cx.simulate_keystrokes("q a x j x q");
    assert_eq!(text(&a, cx), "bc\nbc");
    cx.update(|window, cx| window.focus(&b.focus_handle(cx)));
    cx.run_until_parked();
    cx.simulate_keystrokes("@ a");
    assert_eq!(
        text(&b, cx),
        "xyz\nxyz",
        "a read-only buffer must not change"
    );
}

/// `:w` in a read-only editor (e.g. a library file opened by go-to-
/// definition) is refused by `Editor::save`, yet Vim reports
/// `"…" written`. The error is only logged.
#[gpui::test]
fn review_write_in_read_only_editor_reports_success(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lib.rs");
    std::fs::write(&path, "abc\n").unwrap();
    let (editor, cx) = open(cx, "abc\n");
    editor.update(cx, |e, cx| {
        e.set_path(&path, cx);
        e.set_read_only(true, cx);
    });
    cx.simulate_keystrokes(": w enter");
    let message = editor
        .read_with(cx, |e, _| e.vim_status().unwrap().message)
        .unwrap_or_default();
    assert!(
        !message.contains("written"),
        "nothing was written, but the message says: {message}"
    );
}

/// Whole lines copied from two cursors, pasted with two cursors on the
/// same line: each cursor should get one line (counts match), and the
/// cursors should stay on their own text. Both lines are inserted at the
/// same line start and the first cursor ends up inside the second line.
#[gpui::test]
fn review_per_cursor_line_paste_with_cursors_on_one_line(cx: &mut TestAppContext) {
    let (editor, cx) = open_at(cx, "a\nb\nxyz", "t.txt", common::config());
    editor.update(cx, |e, cx| {
        e.select_ranges(
            vec![
                Point::new(0, 0)..Point::new(0, 0),
                Point::new(1, 0)..Point::new(1, 0),
            ],
            cx,
        )
    });
    cx.simulate_keystrokes("secondary-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
        Some("a\nb\n")
    );
    editor.update(cx, |e, cx| {
        e.select_ranges(
            vec![
                Point::new(2, 1)..Point::new(2, 1),
                Point::new(2, 2)..Point::new(2, 2),
            ],
            cx,
        )
    });
    cx.simulate_keystrokes("secondary-v");
    assert_eq!(text(&editor, cx), "a\nb\na\nb\nxyz");
    let heads: Vec<Point> = editor.read_with(cx, |e, _| {
        e.selections()
            .iter()
            .map(|s| e.offset_to_point(s.head))
            .collect()
    });
    assert_eq!(
        heads,
        vec![Point::new(4, 1), Point::new(4, 2)],
        "both cursors stay on the `xyz` line"
    );
}
