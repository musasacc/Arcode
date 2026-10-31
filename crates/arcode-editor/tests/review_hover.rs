//! Review tests for hover: bugs found while reviewing the hover / go to
//! definition unit. Each test describes the expected behaviour; they fail
//! on the reviewed code.

mod common;

use std::time::Duration;

use arcode_buffer::Point;
use arcode_editor::{Documentation, Editor, HoverInfo, HoverProvider};
use common::*;
use gpui::{Context, Entity, Modifiers, Task, TestAppContext, VisualTestContext, point, px};

/// Go to the end of the document as bound on this platform (Cmd+Down on
/// macOS, Ctrl+End elsewhere).
const DOC_END: &str = if cfg!(target_os = "macos") {
    "cmd-down"
} else {
    "ctrl-end"
};

/// Answers every hover with the hovered word after `delay` (a real
/// language server never answers in the same frame).
struct SlowHover {
    delay: Duration,
}

impl HoverProvider for SlowHover {
    fn hover(
        &self,
        editor: &Editor,
        at: Point,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<HoverInfo>>>> {
        let (range, word) = editor.word_at(at)?;
        let delay = self.delay;
        Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(delay).await;
            Ok(Some(HoverInfo {
                contents: vec![Documentation::Markdown(format!("Docs of *{word}*."))],
                range: Some(range),
            }))
        }))
    }
}

fn install_hover(editor: &Entity<Editor>, delay: Duration, cx: &mut VisualTestContext) {
    let provider = std::rc::Rc::new(SlowHover { delay });
    editor.update(cx, |e, _| e.set_hover_provider(Some(provider)));
}

fn docs(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<String> {
    editor.read_with(cx, |e, _| e.hover_docs_text())
}

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

/// Ctrl+K Ctrl+I, then the user nudges the mouse (over blank space in the
/// editor, or anywhere outside it) while the server is still answering:
/// `update_mouse_hover` sees no word under the mouse and calls
/// `cancel_hover_request`, which drops the keyboard request. The keyboard
/// hover never appears.
#[gpui::test]
fn keyboard_hover_is_not_cancelled_by_mouse_motion(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "let value = other;\n\n\n");
    install_hover(&editor, Duration::from_millis(50), cx);
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(0, 14)..Point::new(0, 14)], cx)
    });
    cx.simulate_keystrokes("secondary-k secondary-i");
    cx.run_until_parked();
    // The mouse moves over an empty line of the editor before the answer.
    let blank = pos_of(&editor, Point::new(2, 0), cx);
    cx.simulate_mouse_move(blank, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(100));
    cx.run_until_parked();
    assert!(
        docs(&editor, cx).is_some_and(|t| t.contains("Docs of other.")),
        "keyboard hover was dropped by a mouse move"
    );
}

/// A keyboard hover (and a mouse hover) stays visible after the editor
/// loses focus, e.g. when the document-symbol picker, the Ctrl+P finder or
/// the terminal takes focus. The popover is deferred with priority 1, so it
/// is painted above the workspace's modal overlays, covers their rows and,
/// being `occlude`d, eats clicks on them (seen in the GUI with Ctrl+K
/// Ctrl+I then Ctrl+Shift+O).
#[gpui::test]
fn hover_hides_when_the_editor_loses_focus(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "let value = other;");
    install_hover(&editor, Duration::ZERO, cx);
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(0, 14)..Point::new(0, 14)], cx)
    });
    cx.simulate_keystrokes("secondary-k secondary-i");
    cx.run_until_parked();
    assert!(docs(&editor, cx).is_some(), "keyboard hover shown");
    // Something else (a picker's text field) takes focus.
    let other = cx.update(|_, cx| cx.focus_handle());
    cx.update(|window, _| window.focus(&other));
    cx.run_until_parked();
    assert_eq!(
        docs(&editor, cx),
        None,
        "the hover outlives the editor's focus and floats over overlays"
    );
}

/// Go to definition opens library sources (e.g. the Rust std library)
/// read-only, but accepting a plain (non-snippet) completion still edits
/// a read-only buffer: `apply_completion` calls `after_edits` without the
/// `read_only` check that `edit_with`, typing and snippets have. Seen in
/// the GUI: F12 on `Option` opened core/option.rs read-only, Ctrl+Space
/// after `Opt` and Enter turned `Option` into `Optionion` and marked the
/// tab dirty; Ctrl+S (no read-only check in `Editor::save`) would then
/// write into the toolchain.
#[gpui::test]
fn completion_does_not_edit_a_read_only_buffer(cx: &mut TestAppContext) {
    use arcode_completion::{CandidateKind, CandidateSource, CompletionCandidate};
    use arcode_editor::CompletionList;
    let (editor, cx) = open(cx, "impl Opt");
    let fake = Fake::new(|_, _, _| {
        let item = arcode_editor::CompletionItem::new(CompletionCandidate::new(
            "Option",
            CandidateKind::Enum,
            CandidateSource::Lsp {
                server: "fake".into(),
            },
        ));
        (
            CompletionList {
                items: vec![item],
                is_incomplete: false,
            },
            Duration::ZERO,
        )
    });
    install(&editor, fake, cx);
    editor.update(cx, |e, cx| e.set_read_only(true, cx));
    cx.simulate_keystrokes("ctrl-space");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "impl Opt", "read-only buffer was edited");
    assert!(!editor.read_with(cx, |e, _| e.is_dirty()));
}

/// A mouse hover stays after the cursor moves far away and the view
/// scrolls (PageDown, Ctrl+End, a go-to-definition in the same file). The
/// popover keeps the window position computed when it opened, so it now
/// floats over unrelated text until the mouse moves.
#[gpui::test]
fn mouse_hover_hides_when_the_view_scrolls_away(cx: &mut TestAppContext) {
    let mut text = String::from("helper();\n");
    for i in 0..300 {
        text.push_str(&format!("let line_{i} = {i};\n"));
    }
    let (editor, cx) = open(cx, &text);
    install_hover(&editor, Duration::ZERO, cx);
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(0, 0)..Point::new(0, 0)], cx)
    });
    cx.run_until_parked();
    let at = pos_of(&editor, Point::new(0, 2), cx);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    assert!(docs(&editor, cx).is_some(), "mouse hover shown");
    cx.simulate_keystrokes(DOC_END);
    cx.run_until_parked();
    let visible = editor.read_with(cx, |e, _| e.visible_line_range());
    assert!(!visible.contains(&0), "the view scrolled: {visible:?}");
    assert_eq!(
        docs(&editor, cx),
        None,
        "hover for line 0 is still shown after scrolling to the end"
    );
}

/// A mouse hover closes when the cursor moves (arrow keys, Vim motions),
/// so it does not keep covering the code being worked on.
#[gpui::test]
fn mouse_hover_hides_when_the_cursor_moves(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "helper();\nlet x = 1;\n");
    install_hover(&editor, Duration::ZERO, cx);
    editor.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(1, 0)..Point::new(1, 0)], cx)
    });
    cx.run_until_parked();
    let at = pos_of(&editor, Point::new(0, 2), cx);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    assert!(docs(&editor, cx).is_some(), "mouse hover shown");
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert_eq!(
        docs(&editor, cx),
        None,
        "hover still shown after the cursor moved"
    );
}
