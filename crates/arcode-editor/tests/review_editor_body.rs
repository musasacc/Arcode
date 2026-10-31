//! Review tests for the editor body redesign (DESIGN.md 6.7).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use arcode_buffer::Point;
use arcode_editor::{ColorKey, EditorEvent, GutterDecoration, GutterLane, GutterMark};
use arcode_ui_kit::IconName;
use common::lang_fake::*;
use common::*;
use gpui::{Modifiers, TestAppContext, point, px};

/// A run button and the light bulb share one column now and the run button
/// is the one drawn on a line that has both. Clicking that visible run
/// triangle must run, not open the (hidden) bulb's code-action menu.
#[gpui::test]
fn clicking_a_run_button_on_the_bulb_line_runs(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, CODE);
    install_services(&editor, false, cx);
    move_to(&editor, Point::new(1, 6), cx);
    settle(cx);
    assert_eq!(editor.read_with(cx, |e, _| e.lightbulb_line()), Some(1));
    editor.update(cx, |e, cx| {
        let play = GutterDecoration {
            line: 1,
            lane: GutterLane::Runnables,
            mark: GutterMark::Icon {
                icon: IconName::Play,
                color: ColorKey::new("success"),
            },
            tag: 7,
        };
        e.set_gutter_decorations("run", vec![play], cx);
    });
    let events: Rc<RefCell<Vec<EditorEvent>>> = Rc::default();
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&editor, move |_, e: &EditorEvent, _| {
            sink.borrow_mut().push(e.clone())
        })
        .detach();
    });
    cx.run_until_parked();
    let (origin, height) = editor
        .read_with(cx, |e, _| e.position_for_point(Point::new(1, 0)))
        .expect("laid out");
    // No breakpoint lane: the run / bulb column starts at the editor's left.
    let at = point(px(8.), origin.y + height / 2.);
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    let runs = events
        .borrow()
        .iter()
        .filter(|e| {
            matches!(
                e,
                EditorEvent::GutterClicked {
                    lane: GutterLane::Runnables,
                    tag: Some(7),
                    ..
                }
            )
        })
        .count();
    assert_eq!(menu(&editor, cx), None, "the hidden bulb opened its menu");
    assert_eq!(runs, 1, "the run button click was not emitted");
}
