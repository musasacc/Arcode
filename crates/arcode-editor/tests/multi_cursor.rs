//! Multi-cursor commands: select all occurrences, cursors above / below,
//! cursor undo, Alt+click, column selection and editing with every cursor.

mod common;

use arcode_buffer::Point;
use arcode_editor::Editor;
use common::{open, text};
use gpui::{
    ClipboardItem, Entity, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, px,
};

fn at(editor: &Entity<Editor>, cx: &mut VisualTestContext, line: usize, col: usize) {
    editor.update(cx, |e, cx| {
        let p = Point::new(line, col);
        e.select_ranges(vec![p..p], cx)
    });
    cx.run_until_parked();
}

fn ranges(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<(usize, usize)> {
    editor.read_with(cx, |e, _| {
        e.selections().iter().map(|s| (s.anchor, s.head)).collect()
    })
}

/// Window position of the middle of the cell at `(line, col)`.
fn pos(
    editor: &Entity<Editor>,
    cx: &mut VisualTestContext,
    line: usize,
    col: usize,
) -> gpui::Point<Pixels> {
    let (p, h) = editor
        .read_with(cx, |e, _| e.position_for_point(Point::new(line, col)))
        .expect("line laid out");
    gpui::point(p.x + px(1.), p.y + h / 2.)
}

#[gpui::test]
fn select_all_occurrences_and_type(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "foo bar foo\nfood foo");
    at(&editor, cx, 0, 1);
    cx.simulate_keystrokes("secondary-shift-l");
    assert_eq!(ranges(&editor, cx), vec![(0, 3), (8, 11), (17, 20)]);
    cx.simulate_input("x");
    assert_eq!(text(&editor, cx), "x bar x\nfood x");
    cx.simulate_keystrokes("backspace");
    assert_eq!(text(&editor, cx), " bar \nfood ");
    cx.simulate_keystrokes("escape");
    assert_eq!(
        ranges(&editor, cx).len(),
        1,
        "Escape goes back to one cursor"
    );
}

#[gpui::test]
fn cursors_above_and_below_and_cursor_undo(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "abc\nabc\nabc\nabc");
    at(&editor, cx, 1, 1);
    cx.simulate_keystrokes("secondary-alt-down");
    cx.simulate_keystrokes("secondary-alt-down");
    cx.simulate_keystrokes("secondary-alt-up");
    assert_eq!(ranges(&editor, cx), vec![(1, 1), (5, 5), (9, 9), (13, 13)]);
    cx.simulate_keystrokes("secondary-u");
    assert_eq!(ranges(&editor, cx), vec![(5, 5), (9, 9), (13, 13)]);
    cx.simulate_keystrokes("secondary-u");
    assert_eq!(ranges(&editor, cx), vec![(5, 5), (9, 9)]);
    cx.simulate_keystrokes("delete");
    assert_eq!(text(&editor, cx), "abc\nac\nac\nabc");
    cx.simulate_keystrokes("secondary-u");
    assert_eq!(
        ranges(&editor, cx).len(),
        2,
        "edits clear the cursor history"
    );
}

#[gpui::test]
fn paste_gives_each_cursor_one_line(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "a\nb\nc");
    at(&editor, cx, 0, 1);
    cx.simulate_keystrokes("secondary-alt-down secondary-alt-down");
    cx.write_to_clipboard(ClipboardItem::new_string("1\n2\n3".into()));
    cx.simulate_keystrokes("secondary-v");
    assert_eq!(text(&editor, cx), "a1\nb2\nc3");
    // Whole lines copied from two cursors go back one per cursor.
    let (editor, cx) = (editor, cx);
    at(&editor, cx, 0, 0);
    cx.simulate_keystrokes("secondary-alt-down");
    cx.simulate_keystrokes("secondary-c");
    at(&editor, cx, 1, 0);
    cx.simulate_keystrokes("secondary-alt-down");
    cx.simulate_keystrokes("secondary-v");
    assert_eq!(text(&editor, cx), "a1\na1\nb2\nb2\nc3");
    // Counts that do not match paste everything at every cursor.
    cx.write_to_clipboard(ClipboardItem::new_string("x\ny\nz".into()));
    at(&editor, cx, 4, 2);
    cx.simulate_keystrokes("secondary-alt-up");
    cx.simulate_keystrokes("secondary-v");
    assert_eq!(text(&editor, cx), "a1\na1\nb2\nb2x\ny\nz\nc3x\ny\nz");
}

#[gpui::test]
fn alt_click_adds_and_removes_cursors(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "abcdef\nabcdef\nabcdef");
    at(&editor, cx, 0, 0);
    let alt = Modifiers {
        alt: true,
        ..Default::default()
    };
    let p = pos(&editor, cx, 1, 2);
    cx.simulate_click(p, alt);
    let p = pos(&editor, cx, 2, 4);
    cx.simulate_click(p, alt);
    assert_eq!(ranges(&editor, cx), vec![(0, 0), (9, 9), (18, 18)]);
    let p = pos(&editor, cx, 1, 2);
    cx.simulate_click(p, alt);
    assert_eq!(ranges(&editor, cx), vec![(0, 0), (18, 18)]);
    cx.simulate_input("-");
    assert_eq!(text(&editor, cx), "-abcdef\nabcdef\nabcd-ef");
}

#[gpui::test]
fn column_selection_with_the_middle_button_and_alt_shift(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, "abcdef\nab\nabcdef\nabcdef");
    let none = Modifiers::default();
    let (from, to) = (pos(&editor, cx, 0, 1), pos(&editor, cx, 2, 4));
    cx.simulate_mouse_down(from, MouseButton::Middle, none);
    cx.simulate_mouse_move(to, MouseButton::Middle, none);
    cx.simulate_mouse_up(to, MouseButton::Middle, none);
    // Line 1 ends inside the box: its part is selected.
    assert_eq!(ranges(&editor, cx), vec![(1, 4), (8, 9), (11, 14)]);
    cx.simulate_input("X");
    assert_eq!(text(&editor, cx), "aXef\naX\naXef\nabcdef");

    let alt_shift = Modifiers {
        alt: true,
        shift: true,
        ..Default::default()
    };
    let (from, to) = (pos(&editor, cx, 2, 2), pos(&editor, cx, 3, 2));
    cx.simulate_mouse_down(from, MouseButton::Left, alt_shift);
    cx.simulate_mouse_move(to, MouseButton::Left, alt_shift);
    cx.simulate_mouse_up(to, MouseButton::Left, alt_shift);
    assert_eq!(ranges(&editor, cx), vec![(10, 10), (15, 15)]);
    cx.simulate_input("_");
    assert_eq!(text(&editor, cx), "aXef\naX\naX_ef\nab_cdef");
    // A box right of a short line skips it.
    let (from, to) = (pos(&editor, cx, 2, 4), pos(&editor, cx, 3, 5));
    let from = gpui::point(from.x, pos(&editor, cx, 1, 0).y);
    cx.simulate_mouse_down(from, MouseButton::Middle, none);
    cx.simulate_mouse_move(to, MouseButton::Middle, none);
    cx.simulate_mouse_up(to, MouseButton::Middle, none);
    assert_eq!(ranges(&editor, cx), vec![(12, 13), (18, 19)]);
}
