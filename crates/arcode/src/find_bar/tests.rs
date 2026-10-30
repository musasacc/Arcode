use arcode_buffer::Buffer;
use gpui::TestAppContext;

use super::*;
use crate::test_support::{init, settle};

const TEXT: &str = "let count = 1;\nprint(count);\nlet total = count + 2;\n";

fn setup(cx: &mut TestAppContext) -> (Entity<Editor>, Entity<FindBar>) {
    init(cx);
    let editor = cx.new(|cx| Editor::new(Buffer::new(TEXT), cx));
    let bar = cx.new(FindBar::new);
    bar.update(cx, |bar, cx| {
        bar.open(Some(editor.clone()), true, cx);
        bar.set_query("count", cx);
    });
    settle(cx);
    (editor, bar)
}

fn text(editor: &Entity<Editor>, cx: &mut TestAppContext) -> String {
    editor.read_with(cx, |e, _| e.buffer().text().to_string())
}

#[gpui::test]
fn finds_highlights_and_steps_through_matches(cx: &mut TestAppContext) {
    let (editor, bar) = setup(cx);
    bar.read_with(cx, |bar, _| {
        assert_eq!(bar.matches().len(), 3);
        assert_eq!(bar.current(), Some(0), "the first match is selected");
    });
    editor.read_with(cx, |e, _| {
        assert_eq!(e.background_highlights(MATCHES_SET).len(), 3);
        assert_eq!(e.background_highlights(CURRENT_SET).len(), 1);
        assert_eq!(e.cursor_position(), Point::new(0, 9));
    });
    bar.update(cx, |bar, cx| bar.go(true, cx));
    editor.read_with(cx, |e, _| {
        assert_eq!(e.cursor_position(), Point::new(1, 11))
    });
    bar.update(cx, |bar, cx| {
        bar.go(false, cx);
        bar.go(false, cx);
    });
    bar.read_with(cx, |bar, _| {
        assert_eq!(bar.current(), Some(2), "wraps around")
    });

    bar.update(cx, |bar, cx| bar.toggle_option(SearchOption::Case, cx));
    bar.update(cx, |bar, cx| bar.set_query("COUNT", cx));
    settle(cx);
    bar.read_with(cx, |bar, _| assert!(bar.matches().is_empty()));

    bar.update(cx, |bar, cx| {
        bar.toggle_option(SearchOption::Regex, cx);
        bar.set_query("(", cx);
    });
    settle(cx);
    bar.read_with(cx, |bar, _| {
        assert!(bar.error.is_some(), "invalid regex is reported")
    });
}

#[gpui::test]
fn replace_next_and_replace_all_are_undoable(cx: &mut TestAppContext) {
    let (editor, bar) = setup(cx);
    bar.update(cx, |bar, cx| {
        bar.set_replacement("n", cx);
        bar.replace_next(cx);
    });
    settle(cx);
    assert_eq!(
        text(&editor, cx),
        "let n = 1;\nprint(count);\nlet total = count + 2;\n"
    );
    bar.read_with(cx, |bar, _| {
        assert_eq!(bar.matches().len(), 2);
        assert_eq!(bar.current(), Some(0), "moved on to the next match");
    });
    let replaced = bar.update(cx, |bar, cx| bar.replace_all(cx));
    assert_eq!(replaced, 2);
    assert_eq!(
        text(&editor, cx),
        "let n = 1;\nprint(n);\nlet total = n + 2;\n"
    );
    editor.update(cx, |e, cx| e.undo(cx));
    assert_eq!(
        text(&editor, cx),
        "let n = 1;\nprint(count);\nlet total = count + 2;\n",
        "replace all is one undo step"
    );
}

#[gpui::test]
fn regex_groups_expand_in_replacements(cx: &mut TestAppContext) {
    let (editor, bar) = setup(cx);
    bar.update(cx, |bar, cx| {
        bar.toggle_option(SearchOption::Regex, cx);
        bar.set_query(r"let (\w+)", cx);
        bar.set_replacement("const $1", cx);
    });
    settle(cx);
    bar.update(cx, |bar, cx| bar.replace_all(cx));
    assert_eq!(
        text(&editor, cx),
        "const count = 1;\nprint(count);\nconst total = count + 2;\n"
    );
}

#[gpui::test]
fn a_closed_bar_leaves_no_highlights_behind(cx: &mut TestAppContext) {
    let (editor, bar) = setup(cx);
    bar.update(cx, |bar, cx| bar.close(cx));
    editor.update(cx, |e, cx| {
        e.apply_edits(
            vec![(Point::new(0, 0)..Point::new(0, 0), "count ".into())],
            cx,
        );
    });
    settle(cx);
    editor.read_with(cx, |e, _| {
        assert!(e.background_highlights(MATCHES_SET).is_empty());
        assert!(e.background_highlights(CURRENT_SET).is_empty());
    });
    bar.update(cx, |bar, cx| bar.reshow(cx));
    settle(cx);
    editor.read_with(cx, |e, _| {
        assert_eq!(e.background_highlights(MATCHES_SET).len(), 4)
    });
}

#[gpui::test]
fn edits_refresh_the_matches(cx: &mut TestAppContext) {
    let (editor, bar) = setup(cx);
    editor.update(cx, |e, cx| {
        e.apply_edits(
            vec![(Point::new(3, 0)..Point::new(3, 0), "count\n".into())],
            cx,
        );
    });
    settle(cx);
    bar.read_with(cx, |bar, _| assert_eq!(bar.matches().len(), 4));
}
