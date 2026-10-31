//! Undo/redo, grouping, version and dirty tracking.

use arcode_buffer::ops::{self, OpsConfig};
use arcode_buffer::{Buffer, Selection, SelectionSet, UndoGroup};

#[test]
fn version_is_monotonic() {
    let mut b = Buffer::new("x");
    let mut last = b.version();
    b.edit(vec![(1..1, "y".into())]);
    assert!(b.version() > last);
    last = b.version();
    b.undo();
    assert!(b.version() > last);
    last = b.version();
    b.redo();
    assert!(b.version() > last);
    last = b.version();
    assert!(b.undo().is_some());
    assert!(b.undo().is_none());
    assert_eq!(b.version(), last + 1);
}

#[test]
fn dirty_tracks_saved_state_through_undo() {
    let mut b = Buffer::new("abc");
    assert!(!b.is_dirty());
    b.edit(vec![(0..0, "1".into())]);
    assert!(b.is_dirty());
    b.undo();
    assert!(!b.is_dirty(), "undo back to the loaded state is clean");
    b.redo();
    assert!(b.is_dirty());
    b.mark_saved();
    assert!(!b.is_dirty());
    b.edit(vec![(0..0, "2".into())]);
    assert!(b.is_dirty());
    b.undo();
    assert!(!b.is_dirty());
    b.undo();
    assert!(b.is_dirty(), "undo past the save point is dirty");
    b.redo();
    assert!(!b.is_dirty());
}

#[test]
fn diverging_after_save_stays_dirty() {
    let mut b = Buffer::new("a");
    b.edit(vec![(1..1, "b".into())]);
    b.mark_saved();
    b.undo();
    b.edit(vec![(1..1, "c".into())]); // drops the redo branch
    assert!(b.is_dirty());
    assert!(!b.can_redo());
    b.undo();
    assert!(b.is_dirty(), "\"a\" was not the saved text");
}

#[test]
fn merge_groups_and_breaks() {
    let mut b = Buffer::new("");
    b.edit_grouped(vec![(0..0, "a".into())], UndoGroup::Merge);
    b.edit_grouped(vec![(1..1, "b".into())], UndoGroup::Merge);
    b.break_undo_group();
    b.edit_grouped(vec![(2..2, "c".into())], UndoGroup::Merge);
    b.edit_grouped(vec![(3..3, "d".into())], UndoGroup::New);
    b.edit_grouped(vec![(4..4, "e".into())], UndoGroup::Merge);
    assert_eq!(b.to_string(), "abcde");
    b.undo();
    assert_eq!(b.to_string(), "abcd");
    b.undo();
    assert_eq!(b.to_string(), "abc");
    b.undo();
    assert_eq!(b.to_string(), "ab");
    b.undo();
    assert_eq!(b.to_string(), "");
    assert!(!b.can_undo());
}

#[test]
fn merge_does_not_cross_undo_or_save() {
    let mut b = Buffer::new("");
    b.edit_grouped(vec![(0..0, "a".into())], UndoGroup::Merge);
    b.edit_grouped(vec![(1..1, "b".into())], UndoGroup::Merge);
    b.undo();
    b.redo();
    b.edit_grouped(vec![(2..2, "c".into())], UndoGroup::Merge);
    b.undo();
    assert_eq!(b.to_string(), "ab");
    b.mark_saved();
    b.edit_grouped(vec![(2..2, "x".into())], UndoGroup::Merge);
    b.undo();
    assert_eq!(b.to_string(), "ab");
    assert!(!b.is_dirty());
}

#[test]
fn merged_typing_after_save_point() {
    let mut b = Buffer::new("");
    b.edit_grouped(vec![(0..0, "a".into())], UndoGroup::Merge);
    let saved = b.to_string();
    b.mark_saved();
    b.edit_grouped(vec![(1..1, "b".into())], UndoGroup::Merge);
    assert!(b.is_dirty());
    b.undo();
    assert_eq!(b.to_string(), saved);
    assert!(!b.is_dirty());
}

#[test]
fn edit_with_selections_restores_them() {
    let mut b = Buffer::new("one two");
    let before =
        SelectionSet::from_vec(vec![Selection::new(0, 3), Selection::new(4, 7)], 1).expect("set");
    let after =
        SelectionSet::from_vec(vec![Selection::cursor(1), Selection::cursor(3)], 1).expect("set");
    b.edit_with_selections(
        vec![(0..3, "1".into()), (4..7, "2".into())],
        UndoGroup::New,
        &before,
        &after,
    );
    assert_eq!(b.to_string(), "1 2");
    let u = b.undo().expect("undo");
    assert_eq!(u.selections, before);
    assert_eq!(u.selections.primary_index(), 1);
    let r = b.redo().expect("redo");
    assert_eq!(r.selections, after);
}

#[test]
fn undo_without_recorded_selections_places_cursors() {
    let mut b = Buffer::new("abc def");
    b.edit(vec![(0..3, "X".into()), (4..7, "YY".into())]);
    assert_eq!(b.to_string(), "X YY");
    let u = b.undo().expect("undo");
    assert_eq!(b.to_string(), "abc def");
    let heads: Vec<usize> = u.selections.iter().map(|s| s.head).collect();
    assert_eq!(heads, vec![3, 7]);
    let r = b.redo().expect("redo");
    let heads: Vec<usize> = r.selections.iter().map(|s| s.head).collect();
    assert_eq!(heads, vec![1, 4]);
}

#[test]
fn multi_cursor_session_undo_redo() {
    let cfg = OpsConfig::default();
    let mut b = Buffer::new("fn a() {}\nfn b() {}\n");
    let mut sels =
        SelectionSet::from_vec(vec![Selection::cursor(8), Selection::cursor(18)], 0).expect("set");
    let s0 = sels.clone();
    ops::newline_and_indent(&mut b, &mut sels, &cfg);
    let s1 = sels.clone();
    for c in ["x", "(", "1"] {
        ops::type_text(&mut b, &mut sels, c, &cfg);
    }
    let after_typing = b.to_string();
    assert_eq!(
        after_typing,
        "fn a() {\n    x(1)\n}\nfn b() {\n    x(1)\n}\n"
    );
    ops::backspace(&mut b, &mut sels, &cfg);
    assert_eq!(
        b.to_string(),
        "fn a() {\n    x()\n}\nfn b() {\n    x()\n}\n"
    );

    b.undo().expect("undo backspace");
    assert_eq!(b.to_string(), after_typing);
    let u = b.undo().expect("undo typing").selections;
    assert_eq!(u, s1);
    assert_eq!(b.to_string(), "fn a() {\n    \n}\nfn b() {\n    \n}\n");
    let u0 = b.undo().expect("undo newline").selections;
    assert_eq!(u0, s0);
    assert_eq!(b.to_string(), "fn a() {}\nfn b() {}\n");
    assert!(!b.is_dirty());
    b.redo();
    b.redo();
    let r = b.redo().expect("redo backspace");
    assert_eq!(
        b.to_string(),
        "fn a() {\n    x()\n}\nfn b() {\n    x()\n}\n"
    );
    assert_eq!(r.selections.len(), 2);
}
