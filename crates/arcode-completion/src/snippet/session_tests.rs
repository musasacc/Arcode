#![allow(clippy::single_range_in_vec_init)]

use arcode_buffer::{Buffer, Selection};

use super::*;
use crate::snippet::expand::expand;
use crate::snippet::variables::VariableContext;

/// Inserts `snippet` at `offset` into `buffer` and starts a session.
fn start(buffer: &mut Buffer, offset: usize, snippet: &str) -> SnippetSession {
    let e = expand(snippet, &VariableContext::default(), "", "    ");
    buffer.edit(vec![(offset..offset, e.text.clone())]);
    SnippetSession::new(&e, offset)
}

/// Replaces `range` with `text` and feeds the edits (and mirror updates).
fn type_at(buffer: &mut Buffer, s: &mut SnippetSession, range: Range<usize>, text: &str) {
    let edits = buffer.edit(vec![(range, text.to_string())]);
    let mut updates = s.apply_edits(&edits);
    while !updates.is_empty() {
        let edits = buffer.edit(updates);
        updates = s.apply_edits(&edits);
    }
}

fn sel(s: &SnippetSession) -> Vec<Range<usize>> {
    s.selections().iter().map(Selection::range).collect()
}

#[test]
fn navigation_and_finish() {
    let mut b = Buffer::new("// ");
    let mut s = start(&mut b, 3, "fn ${1:name}(${2:arg}) {$0}");
    assert_eq!(b.to_string(), "// fn name(arg) {}");
    assert_eq!(sel(&s), [6..10]);
    assert_eq!(s.active_index(), 1);
    assert!(s.prev().is_none());
    assert_eq!(s.next().map(|x| x.primary().range()), Some(11..14));
    assert!(!s.is_finished());
    assert_eq!(s.prev().map(|x| x.primary().range()), Some(6..10));
    s.next();
    let last = s.next().expect("final");
    assert_eq!(last.primary().range(), 17..17);
    assert!(s.is_finished());
    assert!(s.next().is_none());
    assert!(s.prev().is_none());
}

#[test]
fn only_final_stop_is_finished_immediately() {
    let mut b = Buffer::new("");
    let s = start(&mut b, 0, "plain");
    assert!(s.is_finished());
    assert_eq!(sel(&s), [5..5]);
}

#[test]
fn typing_replaces_and_grows_placeholder() {
    let mut b = Buffer::new("");
    let mut s = start(&mut b, 0, "${1:name}: ${2:Type}$0");
    // Selected placeholder replaced by first char, then more typing at end.
    type_at(&mut b, &mut s, 0..4, "f");
    type_at(&mut b, &mut s, 1..1, "o");
    type_at(&mut b, &mut s, 2..2, "o");
    assert_eq!(b.to_string(), "foo: Type");
    assert_eq!(sel(&s), [0..3]);
    // Next stop shifted accordingly.
    assert_eq!(s.next().map(|x| x.primary().range()), Some(5..9));
    // Typing into the empty-after-delete stop keeps growing it.
    type_at(&mut b, &mut s, 5..9, "");
    type_at(&mut b, &mut s, 5..5, "u8");
    assert_eq!(b.to_string(), "foo: u8");
    assert_eq!(sel(&s), [5..7]);
    assert_eq!(s.next().map(|x| x.primary().range()), Some(7..7));
}

#[test]
fn empty_tabstop_grows_while_neighbors_shift() {
    let mut b = Buffer::new("");
    let mut s = start(&mut b, 0, "($1)$2;");
    assert_eq!(sel(&s), [1..1]);
    type_at(&mut b, &mut s, 1..1, "a");
    type_at(&mut b, &mut s, 2..2, "b");
    assert_eq!(b.to_string(), "(ab);");
    assert_eq!(sel(&s), [1..3]);
    assert_eq!(s.next().map(|x| x.primary().range()), Some(4..4));
}

#[test]
fn mirrors_are_updated() {
    let mut b = Buffer::new("");
    let mut s = start(&mut b, 0, "let ${1:x} = 1; f($1, $1);");
    assert_eq!(b.to_string(), "let x = 1; f(x, x);");
    // All mirrors are selected; the first is primary.
    assert_eq!(sel(&s), [4..5, 13..14, 16..17]);
    type_at(&mut b, &mut s, 4..5, "value");
    assert_eq!(b.to_string(), "let value = 1; f(value, value);");
    assert_eq!(s.active_ranges(), [4..9, 17..22, 24..29]);
    type_at(&mut b, &mut s, 9..9, "s");
    assert_eq!(b.to_string(), "let values = 1; f(values, values);");
}

#[test]
fn multi_cursor_typing_needs_no_mirror_updates() {
    let mut b = Buffer::new("");
    let mut s = start(&mut b, 0, "$1-$1");
    let edits = b.edit(vec![(0..0, "z".into()), (1..1, "z".into())]);
    assert!(s.apply_edits(&edits).is_empty());
    assert_eq!(b.to_string(), "z-z");
    assert_eq!(s.active_ranges(), [0..1, 2..3]);
}

#[test]
fn edits_outside_shift_ranges() {
    let mut b = Buffer::new("xx\n;");
    let mut s = start(&mut b, 3, "${1:a} ${2:b}");
    type_at(&mut b, &mut s, 0..0, "123");
    assert_eq!(s.all_ranges(), [(1, 6..7), (2, 8..9), (0, 9..9)]);
    assert_eq!(s.extent(), 6..9);
    assert!(s.contains(7));
    assert!(!s.contains(2));
    // Edit after the snippet leaves ranges alone.
    let end = b.len_chars();
    type_at(&mut b, &mut s, end..end, "zz");
    assert_eq!(s.all_ranges(), [(1, 6..7), (2, 8..9), (0, 9..9)]);
}

#[test]
fn nested_parent_grows_with_child() {
    let mut b = Buffer::new("");
    let mut s = start(&mut b, 0, "${1:f(${2:x})}$0");
    assert_eq!(b.to_string(), "f(x)");
    s.next();
    assert_eq!(sel(&s), [2..3]);
    type_at(&mut b, &mut s, 2..3, "");
    type_at(&mut b, &mut s, 2..2, "yy");
    assert_eq!(b.to_string(), "f(yy)");
    assert_eq!(s.all_ranges()[..2], [(1, 0..5), (2, 2..4)]);
}

#[test]
fn straddling_delete_collapses() {
    let mut b = Buffer::new("");
    let mut s = start(&mut b, 0, "${1:abc}${2:def}");
    type_at(&mut b, &mut s, 2..4, "");
    assert_eq!(b.to_string(), "abef");
    assert_eq!(s.all_ranges()[..2], [(1, 0..2), (2, 2..4)]);
    s.finish();
    assert!(s.is_finished());
}

#[test]
fn choices_are_exposed() {
    let mut b = Buffer::new("");
    let mut s = start(&mut b, 0, "${1|a,b|} $2");
    assert_eq!(s.current_choices().map(<[String]>::len), Some(2));
    s.next();
    assert!(s.current_choices().is_none());
}

#[test]
fn adjacent_empty_stop_does_not_swallow_neighbor() {
    let mut b = Buffer::new("");
    let mut s = start(&mut b, 0, "$1${2:x}");
    type_at(&mut b, &mut s, 0..0, "ab");
    assert_eq!(b.to_string(), "abx");
    assert_eq!(s.all_ranges()[..2], [(1, 0..2), (2, 2..3)]);
}
