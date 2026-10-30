//! Review tests for the editor extension points. Each test reproduces a
//! bug found in review and fails until it is fixed.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use arcode_buffer::{Buffer, Point};
use arcode_config::{Config, ThemeRegistry};
use arcode_editor::{
    BackgroundHighlight, CodeLens, ColorKey, Editor, EditorEvent, EditorPopover, GutterLane,
    LensItem, LensPlacement, ScrollAnchor,
};
use gpui::{
    Entity, Focusable, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, div, point, prelude::*, px,
};

type Events = Rc<RefCell<Vec<EditorEvent>>>;

/// Line height with default settings (14px * 1.5).
const LH: f32 = 21.;

fn open<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<Editor>, &'a mut VisualTestContext, Events) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(Config::default(), cx);
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

fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.buffer().to_string())
}

/// An end-of-line code lens that was scrolled horizontally under the
/// gutter is invisible (text is clipped to the text area) but its hit box
/// is not clipped: a click on the breakpoint lane "presses" the hidden
/// lens (e.g. "Accept Current" on a merge conflict) instead of emitting a
/// gutter click.
#[gpui::test]
fn hidden_lens_under_gutter_does_not_steal_gutter_clicks(cx: &mut TestAppContext) {
    let long = "x".repeat(400);
    let (editor, cx, events) = open(cx, &format!("{long}\nabc\n"));
    editor.update(cx, |e, cx| {
        e.set_gutter_lane_visible(GutterLane::Breakpoints, true, cx);
        e.set_code_lenses(
            "conflict",
            vec![CodeLens {
                line: 1,
                placement: LensPlacement::EndOfLine,
                items: vec![LensItem {
                    id: 1,
                    label: "Accept Current".into(),
                }],
            }],
            cx,
        );
    });
    cx.run_until_parked();
    let x_of = |cx: &mut VisualTestContext, col| {
        editor
            .read_with(cx, |e, _| e.position_for_point(Point::new(1, col)))
            .expect("line 1 laid out")
            .0
            .x
    };
    let gutter_w = x_of(cx, 0);
    let cw = x_of(cx, 1) - gutter_w;
    // The lens starts after "abc" plus a two-char gap: text_left + 5 cw.
    // Scroll right so it starts 4px from the editor's left edge, i.e. in
    // the breakpoint lane.
    let dx = gutter_w + cw * 5. - px(4.);
    cx.simulate_event(ScrollWheelEvent {
        position: point(gutter_w + px(100.), px(10.)),
        delta: ScrollDelta::Pixels(point(-dx, px(0.))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
    events.borrow_mut().clear();
    cx.simulate_click(point(px(8.), px(LH + 10.)), Modifiers::default());
    let got: Vec<String> = events
        .borrow()
        .iter()
        .filter_map(|e| match e {
            EditorEvent::GutterClicked { line, lane, .. } => {
                Some(format!("gutter {line} {lane:?}"))
            }
            EditorEvent::CodeLensClicked { id, .. } => Some(format!("lens {id}")),
            _ => None,
        })
        .collect();
    assert_eq!(got, vec!["gutter 1 Breakpoints".to_string()]);
}

/// Undo while a snippet with mirrors is active must leave redo working.
#[gpui::test]
fn snippet_mirror_undo_then_redo(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "\n");
    editor.update(cx, |e, cx| {
        e.insert_snippet(Point::new(0, 0)..Point::new(0, 0), "${1:a}-${1:a}", cx);
    });
    cx.simulate_input("x");
    assert_eq!(text(&editor, cx), "x-x\n");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&editor, cx), "a-a\n");
    cx.simulate_keystrokes("secondary-shift-z");
    assert_eq!(text(&editor, cx), "x-x\n", "redo after undo in a snippet");
}

/// Docs: "items inside deleted text are dropped". Whole-line background
/// highlights are never dropped: deleting every highlighted line moves the
/// highlight (e.g. the debugger's current line) onto an unrelated line.
#[gpui::test]
fn line_highlight_inside_deleted_text_is_dropped(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "a\nb\nc\nd\ne\nf\n");
    editor.update(cx, |e, cx| {
        e.set_background_highlights(
            "debugger",
            vec![BackgroundHighlight::lines(
                2..4,
                ColorKey::new("warning").opacity(0.3),
            )],
            cx,
        );
        // Delete lines 1..5 ("b".."e"), which contain the whole highlight.
        assert!(e.apply_edits(vec![(Point::new(1, 0)..Point::new(5, 0), String::new())], cx));
    });
    assert_eq!(text(&editor, cx), "a\nf\n");
    let left: Vec<_> = editor.read_with(cx, |e, _| {
        e.background_highlights("debugger")
            .iter()
            .map(|h| h.range.clone())
            .collect()
    });
    assert!(left.is_empty(), "highlight moved onto line \"f\": {left:?}");
}

/// A popover whose anchor is scrolled out of view is not drawn, but its
/// key handler still swallows keys: Enter does nothing visible (an
/// invisible completion item would be accepted) instead of inserting a
/// newline.
#[gpui::test]
fn hidden_popover_does_not_swallow_keys(cx: &mut TestAppContext) {
    let body: String = (0..600).map(|i| format!("line {i}\n")).collect();
    let (editor, cx, _) = open(cx, &body);
    let accepted: Rc<RefCell<u32>> = Rc::default();
    let sink = accepted.clone();
    editor.update(cx, |e, cx| {
        let content = cx.new(|_| Popup).into();
        let popover = EditorPopover::new(Point::new(0, 0), content).on_key(move |_, ks, _, _| {
            if ks.key == "enter" {
                *sink.borrow_mut() += 1;
                return true;
            }
            false
        });
        e.show_popover("completion", popover, cx);
        e.scroll_to_line(500, ScrollAnchor::Top, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        editor.read_with(cx, |e, _| e.visible_line_range().start),
        500
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        *accepted.borrow(),
        0,
        "an off-screen (undrawn) popover consumed Enter"
    );
}

struct Popup;

impl gpui::Render for Popup {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div().w(px(80.)).h(px(40.)).child("items")
    }
}

/// Undoing the snippet insertion itself must end the snippet session;
/// otherwise Tab jumps to stale tabstops instead of indenting.
#[gpui::test]
fn undoing_a_snippet_ends_its_session(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "x\n");
    editor.update(cx, |e, cx| {
        e.insert_snippet(Point::new(0, 0)..Point::new(0, 0), "${1:foo} ${2:bar} ", cx);
    });
    assert_eq!(text(&editor, cx), "foo bar x\n");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&editor, cx), "x\n");
    // Tab must indent again; with the stale session it is swallowed.
    cx.simulate_keystrokes("tab");
    assert_ne!(text(&editor, cx), "x\n", "Tab did nothing after the undo");
}
