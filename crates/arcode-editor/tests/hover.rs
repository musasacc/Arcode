//! Hover popover, keyboard hover and the definition link.

mod common;

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use arcode_buffer::Point;
use arcode_editor::{
    DiagnosticSeverity, Documentation, Editor, EditorDiagnostic, HoverInfo, HoverProvider,
};
use common::*;
use gpui::{Context, Entity, Modifiers, Task, TestAppContext, VisualTestContext, point, px};

/// Answers every hover with the hovered word, records the points asked.
struct FakeHover {
    asked: Rc<RefCell<Vec<Point>>>,
}

impl HoverProvider for FakeHover {
    fn hover(
        &self,
        editor: &Editor,
        at: Point,
        _: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<HoverInfo>>>> {
        self.asked.borrow_mut().push(at);
        let (range, word) = editor.word_at(at)?;
        Some(Task::ready(Ok(Some(HoverInfo {
            contents: vec![
                Documentation::Markdown(format!("```rust\nfn {word}()\n```")),
                Documentation::Markdown(format!("Docs of *{word}*.")),
            ],
            range: Some(range),
        }))))
    }

    fn definition_link(
        &self,
        editor: &Editor,
        at: Point,
        _: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<Range<Point>>>>> {
        let (range, word) = editor.word_at(at)?;
        Some(Task::ready(Ok((word != "nodef").then_some(range))))
    }
}

fn install_hover(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Rc<RefCell<Vec<Point>>> {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let provider = Rc::new(FakeHover {
        asked: asked.clone(),
    });
    editor.update(cx, |e, _| e.set_hover_provider(Some(provider)));
    asked
}

/// Window position inside the char at `p`.
fn pos_of(
    editor: &Entity<Editor>,
    p: Point,
    cx: &mut VisualTestContext,
) -> gpui::Point<gpui::Pixels> {
    let (origin, height) = editor
        .read_with(cx, |e, _| e.position_for_point(p))
        .expect("line laid out");
    point(origin.x + px(3.), origin.y + height / 2.)
}

fn docs(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<String> {
    editor.read_with(cx, |e, _| e.hover_docs_text())
}

#[gpui::test]
fn resting_mouse_shows_docs_after_the_delay(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "fn main() {\n    helper(1);\n}\n");
    let asked = install_hover(&editor, cx);
    let at = pos_of(&editor, Point::new(1, 6), cx);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(docs(&editor, cx), None, "nothing before the delay");
    cx.executor().advance_clock(Duration::from_millis(350));
    cx.run_until_parked();
    let text = docs(&editor, cx).expect("hover shown");
    assert!(
        text.contains("fn helper()") && text.contains("Docs of helper."),
        "{text}"
    );
    assert_eq!(asked.borrow().len(), 1);

    // Moving inside the same word keeps it without asking again.
    let inside = pos_of(&editor, Point::new(1, 8), cx);
    cx.simulate_mouse_move(inside, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(350));
    cx.run_until_parked();
    assert!(docs(&editor, cx).is_some());
    assert_eq!(asked.borrow().len(), 1);

    // Leaving the word hides it.
    let space = pos_of(&editor, Point::new(1, 1), cx);
    cx.simulate_mouse_move(space, None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(docs(&editor, cx), None);
}

#[gpui::test]
fn diagnostics_come_first_and_docs_follow(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "fn main() {\n    helper(1);\n}\n");
    install_hover(&editor, cx);
    editor.update(cx, |e, cx| {
        e.set_diagnostics(
            vec![EditorDiagnostic {
                range: Point::new(1, 4)..Point::new(1, 10),
                severity: DiagnosticSeverity::Error,
                message: "cannot find function `helper`".into(),
                source: "rustc".into(),
            }],
            cx,
        )
    });
    let at = pos_of(&editor, Point::new(1, 5), cx);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    let diags = editor.read_with(cx, |e, _| e.hover_diagnostics());
    assert_eq!(diags, vec!["cannot find function `helper`".to_string()]);
    assert_eq!(docs(&editor, cx), None);
    cx.executor().advance_clock(Duration::from_millis(350));
    cx.run_until_parked();
    let diags = editor.read_with(cx, |e, _| e.hover_diagnostics());
    assert_eq!(diags.len(), 1, "diagnostics stay above the docs");
    assert!(docs(&editor, cx).is_some());
}

#[gpui::test]
fn keyboard_hover_shows_at_once_and_closes_on_cursor_move(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "let value = other;");
    install_hover(&editor, cx);
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(0, 14)..Point::new(0, 14)], cx)
    });
    cx.simulate_keystrokes("ctrl-k ctrl-i");
    cx.run_until_parked();
    let text = docs(&editor, cx).expect("keyboard hover");
    assert!(text.contains("Docs of other."), "{text}");
    // Mouse moves elsewhere do not close it.
    cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::none());
    cx.run_until_parked();
    assert!(docs(&editor, cx).is_some());
    cx.simulate_keystrokes("left");
    assert_eq!(docs(&editor, cx), None);
}

#[gpui::test]
fn hover_setting_off_never_asks(cx: &mut TestAppContext) {
    let mut config = config();
    config.editor.hover = false;
    let (editor, cx) = open_at(cx, "helper();", "test.rs", config);
    let asked = install_hover(&editor, cx);
    let at = pos_of(&editor, Point::new(0, 2), cx);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    assert!(asked.borrow().is_empty());
    assert_eq!(docs(&editor, cx), None);
}

#[gpui::test]
fn secondary_modifier_underlines_and_click_goes_to_definition(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "call(nodef);");
    install_hover(&editor, cx);
    let jumps = Rc::new(RefCell::new(0));
    let counter = jumps.clone();
    cx.update(|_, cx| {
        cx.on_action(move |_: &arcode_editor::GoToDefinition, _| *counter.borrow_mut() += 1)
    });
    let at = pos_of(&editor, Point::new(0, 1), cx);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    let link = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.hover_link());
    assert_eq!(link(cx), None);
    cx.simulate_modifiers_change(Modifiers::secondary_key());
    cx.run_until_parked();
    assert_eq!(link(cx), Some(Point::new(0, 0)..Point::new(0, 4)));
    // No definition: no underline.
    let nodef = pos_of(&editor, Point::new(0, 6), cx);
    cx.simulate_mouse_move(nodef, None, Modifiers::secondary_key());
    cx.run_until_parked();
    assert_eq!(link(cx), None);
    cx.simulate_mouse_move(at, None, Modifiers::secondary_key());
    cx.run_until_parked();
    assert!(link(cx).is_some());
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert_eq!(link(cx), None);

    cx.simulate_click(at, Modifiers::secondary_key());
    cx.run_until_parked();
    assert_eq!(*jumps.borrow(), 1);
    let cursor = editor.read_with(cx, |e, _| e.cursor_position());
    assert_eq!(cursor, Point::new(0, 1));
}

#[gpui::test]
fn moving_within_one_diagnostic_does_not_redraw(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "fn main() {\n    helper(1);\n}\n");
    editor.update(cx, |e, cx| {
        e.set_diagnostics(
            vec![EditorDiagnostic {
                range: Point::new(1, 4)..Point::new(1, 13),
                severity: DiagnosticSeverity::Error,
                message: "bad call".into(),
                source: String::new(),
            }],
            cx,
        )
    });
    cx.run_until_parked();
    let notified = Rc::new(RefCell::new(0));
    let counter = notified.clone();
    cx.update(|_, cx| {
        cx.observe(&editor, move |_, _| *counter.borrow_mut() += 1)
            .detach()
    });
    let first = pos_of(&editor, Point::new(1, 5), cx);
    cx.simulate_mouse_move(first, None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |e, _| e.hover_diagnostics()).len(), 1);
    let shown = *notified.borrow();
    assert!(shown >= 1, "showing the popover redraws");
    for col in [6, 8, 11] {
        let at = pos_of(&editor, Point::new(1, col), cx);
        cx.simulate_mouse_move(at, None, Modifiers::none());
        cx.run_until_parked();
    }
    assert_eq!(*notified.borrow(), shown, "the same popover is not redrawn");
}

#[gpui::test]
fn keyboard_hover_follows_its_line_and_closes_when_it_leaves_the_view(
    cx: &mut TestAppContext,
) {
    let mut text = String::from("let value = other;\n");
    for i in 0..200 {
        text.push_str(&format!("let line_{i} = {i};\n"));
    }
    let (editor, cx) = open(cx, &text);
    install_hover(&editor, cx);
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(0, 14)..Point::new(0, 14)], cx)
    });
    cx.simulate_keystrokes("ctrl-k ctrl-i");
    cx.run_until_parked();
    assert!(docs(&editor, cx).is_some());
    // Scrolling a little keeps it (attached to its moved line); scrolling
    // the line out of view closes it.
    editor.update(cx, |e, cx| e.scroll_to_line(0, arcode_editor::ScrollAnchor::Top, cx));
    cx.run_until_parked();
    assert!(docs(&editor, cx).is_some());
    editor.update(cx, |e, cx| e.scroll_to_line(150, arcode_editor::ScrollAnchor::Top, cx));
    cx.run_until_parked();
    assert_eq!(docs(&editor, cx), None);
}
