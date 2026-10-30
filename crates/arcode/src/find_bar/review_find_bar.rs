//! Review tests for the find bar (failing tests that reproduce bugs found
//! in review; see each test's doc comment).

use std::time::{Duration, Instant};

use arcode_buffer::Buffer;
use gpui::TestAppContext;

use super::*;
use crate::test_support::{init, settle};

fn open_bar(text: &str, query: &str, cx: &mut TestAppContext) -> (Entity<Editor>, Entity<FindBar>) {
    init(cx);
    let editor = cx.new(|cx| Editor::new(Buffer::new(text), cx));
    let bar = cx.new(FindBar::new);
    bar.update(cx, |bar, cx| {
        bar.open(Some(editor.clone()), true, cx);
        bar.set_query(query, cx);
    });
    settle(cx);
    (editor, bar)
}

fn selected_text(editor: &Entity<Editor>, cx: &mut TestAppContext) -> String {
    editor.read_with(cx, |e, _| {
        let sel = e.selections().primary();
        let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
        e.text_in_range(e.offset_to_point(a)..e.offset_to_point(b))
    })
}

/// F3 after the bar was closed and the buffer edited (pane `find_step`:
/// `set_editor` + `reshow` + `go`) steps through the matches computed
/// before the edit: `go` runs before the re-search finishes, so it selects
/// a stale range that is no longer a match.
#[gpui::test]
fn f3_after_reopening_selects_a_real_match_not_a_stale_range(cx: &mut TestAppContext) {
    let (editor, bar) = open_bar("count\ncount\n", "count", cx);
    bar.update(cx, |bar, cx| bar.close(cx));
    // While closed, line 1 changes: its match moves two columns right.
    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(1, 0)..Point::new(1, 0), "zz".into())], cx);
    });
    settle(cx);
    // What Pane::find_step does for F3 on a closed bar.
    bar.update(cx, |bar, cx| {
        bar.set_editor(Some(editor.clone()), cx);
        bar.reshow(cx);
        bar.go(true, cx);
    });
    settle(cx);
    assert_eq!(
        selected_text(&editor, cx),
        "count",
        "F3 must select a match of the current text"
    );
}

/// F3 on a closed bar while another tab is active (pane `find_step` calls
/// `set_editor(new)`, which clears the matches, then `go`): the first press
/// shows the bar but selects nothing, because `go` runs on an empty match
/// list and the re-search finishes without selecting.
#[gpui::test]
fn f3_on_another_tab_selects_the_next_match_on_the_first_press(cx: &mut TestAppContext) {
    let (_first, bar) = open_bar("count\n", "count", cx);
    bar.update(cx, |bar, cx| bar.close(cx));
    let other = cx.new(|cx| Editor::new(Buffer::new("x\ncount here\n"), cx));
    bar.update(cx, |bar, cx| {
        bar.set_editor(Some(other.clone()), cx);
        bar.reshow(cx);
        bar.go(true, cx);
    });
    settle(cx);
    bar.read_with(cx, |bar, _| {
        assert_eq!(bar.matches().len(), 1);
        assert_eq!(bar.current(), Some(0), "the first F3 moves to a match");
    });
    assert_eq!(selected_text(&other, cx), "count");
}

/// Replace All in the find bar searches and applies every edit on the main
/// thread in one go. On a 200k-line file (one match per line) the debug
/// app froze for about 11 minutes; this smaller case (20k matches) should
/// still stay well under a frame budget's order of magnitude.
#[gpui::test]
fn replace_all_on_a_large_buffer_does_not_freeze_the_ui(cx: &mut TestAppContext) {
    let text: String = (0..20_000)
        .map(|i| format!("row {i} needle filler\n"))
        .collect();
    let (editor, bar) = open_bar(&text, "needle", cx);
    bar.update(cx, |bar, cx| bar.set_replacement("pin", cx));
    let start = Instant::now();
    let n = bar.update(cx, |bar, cx| bar.replace_all(cx));
    let took = start.elapsed();
    assert_eq!(n, 20_000);
    assert!(
        editor
            .read_with(cx, |e, _| e.buffer().text().to_string())
            .contains("row 0 pin")
    );
    assert!(
        took < Duration::from_millis(1500),
        "replace_all blocked the main thread for {took:?}"
    );
}

/// The "Replaced N occurrences" notice of a Replace All survives switching
/// the bar to another editor (tab switch, `set_editor`) and reshowing it,
/// so the other file shows "Replaced 200000 occurrences" instead of its
/// own "1 of 2" match status (seen in the app after switching tabs).
#[gpui::test]
fn replace_all_notice_does_not_follow_the_bar_to_another_file(cx: &mut TestAppContext) {
    let (_first, bar) = open_bar("count count\n", "count", cx);
    bar.update(cx, |bar, cx| {
        bar.set_replacement("n", cx);
        bar.replace_all(cx);
    });
    settle(cx);
    let other = cx.new(|cx| Editor::new(Buffer::new("count\n"), cx));
    bar.update(cx, |bar, cx| bar.set_editor(Some(other.clone()), cx));
    settle(cx);
    bar.read_with(cx, |bar, _| {
        assert_eq!(bar.matches().len(), 1);
        assert_eq!(
            bar.notice, None,
            "the other file's status must not show the old replace notice"
        );
    });
}
