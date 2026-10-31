//! Inlay hints, code actions (light bulb and menu), rename and the
//! context menu with a scripted `LanguageServices` provider.

mod common;

use arcode_buffer::Point;
use common::lang_fake::*;
use common::*;
use gpui::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};

#[gpui::test]
fn inlay_hints_follow_edits_and_the_toggle(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, CODE);
    let log = install_services(&editor, false, cx);
    settle(cx);
    let hints = editor.read_with(cx, |e, _| e.inlay_hint_labels());
    assert_eq!(
        hints,
        vec![
            (Point::new(1, 9), ": i32".to_string()),
            (Point::new(2, 9), ": i32".to_string())
        ]
    );
    let asked = log.borrow().hint_requests;
    // Idle: nothing is asked again.
    settle(cx);
    assert_eq!(log.borrow().hint_requests, asked);

    // An edit moves the hints at once and asks again after the debounce.
    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(0, 0)..Point::new(0, 0), "\n".into())], cx)
    });
    cx.run_until_parked();
    let moved = editor.read_with(cx, |e, _| e.inlay_hint_labels());
    assert_eq!(moved[0].0, Point::new(2, 9));
    settle(cx);
    assert_eq!(log.borrow().hint_requests, asked + 1);

    // The toggle hides them in every editor and stops asking.
    editor.update(cx, |e, cx| e.toggle_inlay_hints(cx));
    settle(cx);
    assert!(
        editor
            .read_with(cx, |e, _| e.inlay_hint_labels())
            .is_empty()
    );
    let before = log.borrow().hint_requests;
    editor.update(cx, |e, cx| e.toggle_inlay_hints(cx));
    settle(cx);
    assert_eq!(editor.read_with(cx, |e, _| e.inlay_hint_labels()).len(), 2);
    assert_eq!(log.borrow().hint_requests, before + 1);
}

#[gpui::test]
fn light_bulb_and_code_action_menu(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, CODE);
    let log = install_services(&editor, false, cx);
    move_to(&editor, Point::new(1, 6), cx);
    assert_eq!(editor.read_with(cx, |e, _| e.lightbulb_line()), None);
    settle(cx);
    assert_eq!(editor.read_with(cx, |e, _| e.lightbulb_line()), Some(1));
    // No actions on another line: the bulb goes away.
    move_to(&editor, Point::new(2, 4), cx);
    settle(cx);
    assert_eq!(editor.read_with(cx, |e, _| e.lightbulb_line()), None);

    move_to(&editor, Point::new(1, 6), cx);
    cx.simulate_keystrokes("ctrl-.");
    cx.run_until_parked();
    assert_eq!(
        menu(&editor, cx).unwrap(),
        [
            "[Quick Fix]",
            "> Add missing semicolon",
            "[Extract]",
            "Extract into variable",
            "[Source Action]",
            "Organize imports",
        ]
    );
    cx.simulate_keystrokes("down down up down enter");
    cx.run_until_parked();
    assert_eq!(menu(&editor, cx), None, "picking closes the menu");
    assert_eq!(log.borrow().applied, ["Organize imports"]);

    // Refactor... lists only refactorings; Escape closes it.
    cx.simulate_keystrokes("ctrl-shift-r");
    cx.run_until_parked();
    assert_eq!(
        menu(&editor, cx).unwrap(),
        ["[Extract]", "> Extract into variable"]
    );
    cx.simulate_keystrokes("escape");
    assert_eq!(menu(&editor, cx), None);

    // Typing closes the menu and reaches the buffer.
    cx.simulate_keystrokes("ctrl-.");
    cx.run_until_parked();
    assert!(menu(&editor, cx).is_some());
    cx.simulate_input("z");
    assert_eq!(menu(&editor, cx), None);
    assert!(text(&editor, cx).contains("lezt"));
}

#[gpui::test]
fn no_actions_shows_a_notice(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, CODE);
    install_services(&editor, false, cx);
    move_to(&editor, Point::new(2, 4), cx);
    cx.simulate_keystrokes("ctrl-.");
    cx.run_until_parked();
    let notice = editor.read_with(cx, |e, cx| e.notice_text(cx));
    assert_eq!(notice.as_deref(), Some("No code actions available"));
    cx.simulate_keystrokes("right");
    assert_eq!(editor.read_with(cx, |e, cx| e.notice_text(cx)), None);
}

#[gpui::test]
fn rename_field_edits_and_commits(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, CODE);
    let log = install_services(&editor, false, cx);
    move_to(&editor, Point::new(2, 13), cx);
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    let field = |cx: &mut VisualTestContext| editor.read_with(cx, |e, cx| e.rename_text(cx));
    assert_eq!(field(cx).as_deref(), Some("x"));
    // The name starts selected: typing replaces it; the buffer is untouched.
    cx.simulate_input("total");
    cx.simulate_keystrokes("backspace");
    assert_eq!(field(cx).as_deref(), Some("tota"));
    assert_eq!(text(&editor, cx), CODE);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(field(cx), None);
    assert_eq!(
        log.borrow().renamed,
        [(Point::new(2, 13), "tota".to_string())]
    );

    // Escape cancels; so does moving the cursor.
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert!(field(cx).is_some());
    cx.simulate_keystrokes("escape");
    assert_eq!(field(cx), None);
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    move_to(&editor, Point::new(0, 0), cx);
    assert_eq!(field(cx), None);
    assert_eq!(log.borrow().renamed.len(), 1);
}

#[gpui::test]
fn right_click_opens_the_context_menu(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, CODE);
    install_services(&editor, false, cx);
    let (origin, height) = editor
        .read_with(cx, |e, _| e.position_for_point(Point::new(2, 12)))
        .expect("laid out");
    let at = point(origin.x + px(2.), origin.y + height / 2.);
    cx.simulate_click(at, Modifiers::none());
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        editor.read_with(cx, |e, _| e.cursor_position()),
        Point::new(2, 12)
    );
    let labels = menu(&editor, cx).expect("context menu");
    assert_eq!(
        labels,
        [
            "> Go to Definition",
            "Find All References",
            "-",
            "Rename Symbol",
            "Refactor...",
            "Format Document",
            "-",
            "Cut",
            "Copy",
            "Paste",
        ]
    );
    // Picking "Rename Symbol" runs the action in the editor.
    cx.simulate_keystrokes("down down enter");
    cx.run_until_parked();
    assert_eq!(
        editor.read_with(cx, |e, cx| e.rename_text(cx)).as_deref(),
        Some("x")
    );
}
