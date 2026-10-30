//! Adversarial review tests. Each test demonstrates one reported issue and
//! is expected to FAIL until the implementation is fixed.

use std::time::{Duration, Instant};

use arcode_buffer::ops::{self, OpsConfig};
use arcode_buffer::{Buffer, PositionEncoding, Selection, SelectionSet, TsPoint, movement};

/// Tree-sitter and LSP count only `\n` as a line break. A form feed (common
/// in GNU C sources, Emacs Lisp, Python stdlib files) must not start a new
/// row, otherwise every Edit sent to tree-sitter / LSP below it is wrong.
#[test]
fn review_form_feed_is_not_a_line_break_for_ts_and_lsp() {
    let b = Buffer::new("a\u{000C}b\nc");
    // 'c' is char 4, on the second '\n'-delimited line.
    assert_eq!(b.char_to_ts_point(4), TsPoint::new(1, 0));
    assert_eq!(b.to_lsp_position(4, PositionEncoding::Utf16), (1, 0));
    assert_eq!(b.from_lsp_position(1, 0, PositionEncoding::Utf16), 4);
}

/// The same root cause makes a single `Edit` internally inconsistent: the
/// start/old-end rows come from ropey (which counts U+2028, U+0085, VT, FF
/// as breaks) while `new_end_position` only counts `\n`.
#[test]
fn review_edit_new_end_position_agrees_with_buffer() {
    let mut b = Buffer::new("x");
    let edits = b.edit(vec![(0..0, "a\u{2028}b".to_string())]);
    let e = &edits[0];
    // After the edit, the buffer's own conversion of the new end must
    // match the new_end_position reported for that edit.
    assert_eq!(b.char_to_ts_point(e.new_end_char()), e.new_end_position);
}

/// Word movement works on chars, so a combining mark (not alphanumeric) is
/// treated as punctuation and the cursor lands inside a grapheme cluster.
#[test]
fn review_word_right_does_not_split_grapheme() {
    let b = Buffer::new("cafe\u{301} x");
    assert_eq!(
        movement::word_right(&b, 0),
        5,
        "must not stop between e and U+0301"
    );
    assert_eq!(
        movement::word_left(&b, 5),
        0,
        "must not stop between e and U+0301"
    );
}

/// `page_down` casts `lines` to `isize`; a huge count wraps negative and the
/// cursor moves UP instead of down.
#[test]
fn review_page_down_huge_count_moves_down() {
    let b = Buffer::new("a\nb\nc");
    let (pos, _) = movement::page_down(&b, 2, None, usize::MAX, 4);
    assert_eq!(pos, b.len_chars());
}

/// Ops panic (slice out of bounds) when a selection is past the end of the
/// buffer, e.g. a stale selection set after the file was reloaded/shrunk.
/// The spec asks for clamping; library code should not panic.
#[test]
fn review_backspace_with_stale_selection_does_not_panic() {
    let mut b = Buffer::new("abc");
    let mut sels = SelectionSet::single(100);
    let cfg = OpsConfig::default();
    ops::backspace(&mut b, &mut sels, &cfg);
    assert_eq!(b.to_string(), "ab");
}

#[test]
fn review_newline_with_stale_selection_does_not_panic() {
    let mut b = Buffer::new("abc");
    let mut sels = SelectionSet::single(100);
    let cfg = OpsConfig::default();
    ops::newline_and_indent(&mut b, &mut sels, &cfg);
    assert_eq!(b.to_string(), "abc\n");
}

/// Typing with many cursors is O(k^2): every cursor is mapped with
/// `map_pos`, which walks all preceding changes. 40k cursors (e.g. "select
/// all occurrences" in a large file) take many seconds per keystroke.
#[test]
fn review_multi_cursor_typing_is_not_quadratic() {
    let n = 40_000;
    let text = "ab\n".repeat(n);
    let mut b = Buffer::new(&text);
    let sels: Vec<Selection> = (0..n).map(|i| Selection::cursor(i * 3 + 1)).collect();
    let mut sels = SelectionSet::from_vec(sels, 0).expect("non-empty");
    let start = Instant::now();
    ops::insert_text(&mut b, &mut sels, "x");
    let elapsed = start.elapsed();
    assert_eq!(b.len_chars(), text.chars().count() + n);
    assert!(
        elapsed < Duration::from_secs(3),
        "one keystroke took {elapsed:?}"
    );
}

/// Switching the line ending away and back to the on-disk one leaves the
/// buffer dirty forever (saved_state is set to u64::MAX), although the
/// on-disk form is identical again.
#[test]
fn review_line_ending_round_trip_is_clean() {
    use arcode_buffer::LineEnding;
    let mut b = Buffer::new("a\nb\n");
    b.set_line_ending(LineEnding::CrLf);
    assert!(b.is_dirty());
    b.set_line_ending(LineEnding::Lf);
    assert!(!b.is_dirty(), "back to the loaded line ending and text");
}
