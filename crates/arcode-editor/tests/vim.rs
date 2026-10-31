//! Vim mode in the editor, driven by real keystrokes.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use arcode_buffer::Point;
use arcode_completion::{CandidateKind, CandidateSource, CompletionCandidate};
use arcode_config::Config;
use arcode_editor::{CompletionItem, CompletionList, CursorShape, Editor, EditorEvent, VimRequest};
use arcode_vim::Mode;
use common::{Fake, install, labels, open_at, text, visible};
use gpui::{
    AppContext, ClipboardItem, Entity, Focusable, IntoElement, ParentElement, Styled,
    TestAppContext, VisualTestContext, div,
};

fn vim_config() -> Config {
    let mut config = common::config();
    config.editor.vim_mode = true;
    config
}

/// Opens `text` in Vim mode with the cursor at the start.
fn open<'a>(cx: &'a mut TestAppContext, text: &str) -> (Entity<Editor>, &'a mut VisualTestContext) {
    let (editor, cx) = open_at(cx, text, "test.rs", vim_config());
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(0, 0)..Point::new(0, 0)], cx)
    });
    cx.run_until_parked();
    (editor, cx)
}

fn mode(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<Mode> {
    editor.read_with(cx, |e, _| e.vim_mode())
}

fn cursor(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Point {
    editor.read_with(cx, |e, _| e.cursor_position())
}

#[gpui::test]
fn vim_mode_follows_the_setting_and_the_toggle(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "abc");
    assert_eq!(mode(&editor, cx), Some(Mode::Normal));
    assert_eq!(
        editor.read_with(cx, |e, _| e.cursor_shape()),
        CursorShape::Block
    );
    cx.simulate_keystrokes("i");
    assert_eq!(mode(&editor, cx), Some(Mode::Insert));
    assert_eq!(
        editor.read_with(cx, |e, _| e.cursor_shape()),
        CursorShape::Bar
    );
    cx.simulate_keystrokes("escape");
    editor.update(cx, |e, cx| e.toggle_vim_mode(cx));
    cx.run_until_parked();
    assert_eq!(mode(&editor, cx), None);
    assert_eq!(
        editor.read_with(cx, |e, _| e.cursor_shape()),
        CursorShape::Bar
    );
    cx.simulate_keystrokes("x");
    assert_eq!(text(&editor, cx), "xabc", "plain typing again");
    editor.update(cx, |e, cx| e.toggle_vim_mode(cx));
    cx.run_until_parked();
    assert_eq!(mode(&editor, cx), Some(Mode::Normal));
}

#[gpui::test]
fn operators_counts_dot_and_undo(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "one two three four five");
    cx.simulate_keystrokes("d w");
    assert_eq!(text(&editor, cx), "two three four five");
    cx.simulate_keystrokes(".");
    assert_eq!(text(&editor, cx), "three four five");
    cx.simulate_keystrokes("2 d w");
    assert_eq!(text(&editor, cx), "five");
    cx.simulate_keystrokes("u");
    assert_eq!(text(&editor, cx), "three four five");
    cx.simulate_keystrokes("ctrl-r");
    assert_eq!(text(&editor, cx), "five");
    cx.simulate_keystrokes("c i w 5 escape");
    assert_eq!(text(&editor, cx), "5");
    assert_eq!(mode(&editor, cx), Some(Mode::Normal));
}

#[gpui::test]
fn insert_visual_and_registers(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "alpha\nbeta\n");
    cx.simulate_keystrokes("A space x escape");
    assert_eq!(text(&editor, cx), "alpha x\nbeta\n");
    cx.simulate_keystrokes("shift-v y j p");
    assert_eq!(text(&editor, cx), "alpha x\nbeta\nalpha x\n");
    cx.simulate_keystrokes("g g v e");
    assert_eq!(mode(&editor, cx), Some(Mode::Visual));
    cx.simulate_keystrokes("\" a d");
    assert_eq!(text(&editor, cx), " x\nbeta\nalpha x\n");
    cx.simulate_keystrokes("j \" a shift-p");
    assert_eq!(text(&editor, cx), " x\nalphabeta\nalpha x\n");
    // The system clipboard register.
    cx.simulate_keystrokes("\" + y y");
    let clip = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(clip.as_deref(), Some("alphabeta\n"));
    cx.write_to_clipboard(ClipboardItem::new_string("clip".into()));
    cx.simulate_keystrokes("0 \" + shift-p");
    assert_eq!(text(&editor, cx), " x\nclipalphabeta\nalpha x\n");
}

#[gpui::test]
fn search_highlights_and_jumps(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "foo bar\nbar foo\nfoo");
    cx.simulate_keystrokes("/ f o o enter");
    assert_eq!(cursor(&editor, cx), Point::new(1, 4));
    let shown = editor.read_with(cx, |e, _| e.background_highlights("vim-search").len());
    assert_eq!(shown, 3);
    cx.simulate_keystrokes("n");
    assert_eq!(cursor(&editor, cx), Point::new(2, 0));
    cx.simulate_keystrokes("shift-n");
    assert_eq!(cursor(&editor, cx), Point::new(1, 4));
    cx.simulate_keystrokes("? b a r enter");
    assert_eq!(cursor(&editor, cx), Point::new(1, 0));
    cx.simulate_keystrokes(": n o h enter");
    let shown = editor.read_with(cx, |e, _| e.background_highlights("vim-search").len());
    assert_eq!(shown, 0);
    cx.simulate_keystrokes(": 3 enter");
    assert_eq!(cursor(&editor, cx), Point::new(2, 0));
    cx.simulate_keystrokes(": % s / f o o / x / g enter");
    assert_eq!(text(&editor, cx), "x bar\nbar x\nx");
}

#[gpui::test]
fn marks_jump_back(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "one\ntwo\nthree\n");
    cx.simulate_keystrokes("j l m a shift-g ` a");
    assert_eq!(cursor(&editor, cx), Point::new(1, 1));
    cx.simulate_keystrokes("shift-g ' a");
    assert_eq!(cursor(&editor, cx), Point::new(1, 0));
}

#[gpui::test]
fn ex_commands_reach_the_application(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    std::fs::write(&path, "abc\n").unwrap();
    let (editor, cx) = open(cx, "");
    editor.update(cx, |e, cx| e.set_path(&path, cx));
    let requests = Rc::new(RefCell::new(Vec::new()));
    let sink = requests.clone();
    cx.update(|_, cx| {
        arcode_editor::set_vim_request_handler(
            Rc::new(move |_, request, _, _| sink.borrow_mut().push(request)),
            cx,
        )
    });
    cx.simulate_keystrokes(": e space b . r s enter");
    cx.simulate_keystrokes(": q enter");
    assert_eq!(
        *requests.borrow(),
        vec![
            VimRequest::Open("b.rs".into()),
            VimRequest::Close { force: false }
        ]
    );
    requests.borrow_mut().clear();
    cx.simulate_keystrokes("i x escape : q enter");
    assert!(requests.borrow().is_empty(), "a dirty buffer is not closed");
    let message = editor.read_with(cx, |e, _| e.vim_status().unwrap().message);
    assert!(message.unwrap().starts_with("E37"));
    cx.simulate_keystrokes(": q ! enter");
    assert_eq!(*requests.borrow(), vec![VimRequest::Close { force: true }]);
    requests.borrow_mut().clear();
    cx.simulate_keystrokes(": w q enter");
    assert_eq!(*requests.borrow(), vec![VimRequest::Close { force: false }]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "x");
    cx.simulate_keystrokes(": b o g u s enter");
    let message = editor.read_with(cx, |e, _| e.vim_status().unwrap().message);
    assert_eq!(
        message.as_deref(),
        Some("E492: Not an editor command: bogus")
    );
}

#[gpui::test]
fn status_reports_mode_and_pending_keys(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "abc");
    let events = Rc::new(RefCell::new(0));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&editor, move |_, e: &EditorEvent, _| {
            if *e == EditorEvent::VimStatusChanged {
                *sink.borrow_mut() += 1;
            }
        })
        .detach()
    });
    cx.simulate_keystrokes("2 d");
    let status = editor.read_with(cx, |e, _| e.vim_status().unwrap());
    assert_eq!(status.pending, "2d");
    assert_eq!(status.mode, "O-PENDING");
    assert!(*events.borrow() >= 2);
    cx.simulate_keystrokes("escape :");
    let status = editor.read_with(cx, |e, _| e.vim_status().unwrap());
    assert_eq!(status.command_line.as_deref(), Some(":"));
    let before = *events.borrow();
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("l");
    let after = *events.borrow();
    cx.simulate_keystrokes("l");
    assert_eq!(*events.borrow(), after, "a plain motion changes no status");
    assert!(after > before);
}

#[gpui::test]
fn completion_works_in_insert_mode_and_escape_closes_it_first(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    install(
        &editor,
        Fake::new(|_, _, _| {
            let item = |l: &str| {
                let source = CandidateSource::Lsp {
                    server: "fake".into(),
                };
                CompletionItem::new(CompletionCandidate::new(l, CandidateKind::Function, source))
            };
            let list = CompletionList {
                items: vec![item("println"), item("print")],
                is_incomplete: false,
            };
            (list, Duration::ZERO)
        }),
        cx,
    );
    cx.simulate_keystrokes("i p r");
    assert!(visible(&editor, cx));
    assert!(labels(&editor, cx).contains(&"println".to_string()));
    cx.simulate_keystrokes("escape");
    assert!(!visible(&editor, cx));
    assert_eq!(
        mode(&editor, cx),
        Some(Mode::Insert),
        "the first Escape closes the list"
    );
    cx.simulate_keystrokes("i n t l");
    assert!(visible(&editor, cx), "typing opens the list again");
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&editor, cx), "println");
    cx.simulate_keystrokes("escape");
    assert_eq!(mode(&editor, cx), Some(Mode::Normal));
}

#[gpui::test]
fn snippets_take_tab_in_insert_mode(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "");
    cx.simulate_keystrokes("i");
    editor.update(cx, |e, cx| {
        let at = Point::new(0, 0);
        e.insert_snippet(at..at, "f(${1:a}, ${2:b})", cx)
    });
    cx.simulate_keystrokes("x tab y");
    assert_eq!(text(&editor, cx), "f(x, y)");
    cx.simulate_keystrokes("escape");
    assert_eq!(mode(&editor, cx), Some(Mode::Normal));
    assert!(!editor.read_with(cx, |e, _| e.snippet_active()));
}

#[gpui::test]
fn read_only_editors_take_motions_but_no_edits(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "abc def");
    editor.update(cx, |e, cx| e.set_read_only(true, cx));
    cx.simulate_keystrokes("w");
    assert_eq!(cursor(&editor, cx), Point::new(0, 4));
    cx.simulate_keystrokes("d w");
    assert_eq!(text(&editor, cx), "abc def");
    assert!(!editor.read_with(cx, |e, _| e.is_dirty()));
}

#[gpui::test]
fn escape_in_normal_mode_leaves_one_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "a\nb\nc");
    cx.simulate_keystrokes("secondary-alt-down secondary-alt-down");
    assert_eq!(editor.read_with(cx, |e, _| e.selections().len()), 3);
    cx.simulate_keystrokes("x");
    assert_eq!(text(&editor, cx), "\n\n");
    cx.simulate_keystrokes("escape");
    assert_eq!(editor.read_with(cx, |e, _| e.selections().len()), 1);
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

#[gpui::test]
fn registers_and_search_are_shared_between_editors(cx: &mut TestAppContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(vim_config(), cx);
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(std::sync::Arc::new(theme), cx);
        arcode_editor::init(cx);
    });
    let (two, cx) = cx.add_window_view(|_, cx| {
        let editor = |text: &str, cx: &mut gpui::Context<Two>| {
            let text = text.to_string();
            cx.new(|cx| Editor::new(arcode_buffer::Buffer::new(&text), cx))
        };
        Two {
            a: editor("alpha beta", cx),
            b: editor("x beta", cx),
        }
    });
    let (a, b) = two.read_with(cx, |t, _| (t.a.clone(), t.b.clone()));
    let focus = |e: &Entity<Editor>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.focus(&e.focus_handle(cx)));
        cx.run_until_parked();
    };
    focus(&a, cx);
    cx.simulate_keystrokes("y w");
    focus(&b, cx);
    cx.simulate_keystrokes("shift-p");
    assert_eq!(text(&b, cx), "alpha x beta");
    focus(&a, cx);
    cx.simulate_keystrokes("/ b e t a enter");
    focus(&b, cx);
    cx.simulate_keystrokes("0 n");
    assert_eq!(
        cursor(&b, cx),
        Point::new(0, 8),
        "`n` repeats the other editor's search"
    );
}

#[gpui::test]
fn visual_block_and_replace_modes(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "abcd\nabcd\nabcd");
    cx.simulate_keystrokes("l ctrl-v j j l");
    assert_eq!(mode(&editor, cx), Some(Mode::VisualBlock));
    assert_eq!(editor.read_with(cx, |e, _| e.selections().len()), 3);
    cx.simulate_keystrokes("d");
    assert_eq!(text(&editor, cx), "ad\nad\nad");
    cx.simulate_keystrokes("shift-r");
    assert_eq!(mode(&editor, cx), Some(Mode::Replace));
    assert_eq!(
        editor.read_with(cx, |e, _| e.cursor_shape()),
        CursorShape::Underline
    );
    cx.simulate_keystrokes("x y escape");
    assert_eq!(text(&editor, cx), "axy\nad\nad");
    cx.simulate_keystrokes("j 0 shift-v j >");
    assert_eq!(text(&editor, cx), "axy\n    ad\n    ad");
}
