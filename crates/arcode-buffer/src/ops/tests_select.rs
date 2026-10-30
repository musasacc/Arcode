use super::test_util::{setup, show};
use super::*;

#[test]
fn select_all_and_line() {
    let (b, mut s) = setup("a|b\n|cd\nef");
    select_all(&b, &mut s);
    assert_eq!(show(&b, &s), "«ab\ncd\nef»");
    let (b, mut s) = setup("a|b\ncd\nef");
    select_line(&b, &mut s);
    assert_eq!(show(&b, &s), "«ab\n»cd\nef");
    select_line(&b, &mut s);
    assert_eq!(show(&b, &s), "«ab\ncd\n»ef");
    select_line(&b, &mut s);
    assert_eq!(show(&b, &s), "«ab\ncd\nef»");
    select_line(&b, &mut s);
    assert_eq!(show(&b, &s), "«ab\ncd\nef»");
}

#[test]
fn select_line_multi_cursor_merges() {
    let (b, mut s) = setup("a|b|\ncd\ne|f");
    select_line(&b, &mut s);
    assert_eq!(show(&b, &s), "«ab\n»cd\n«ef»");
}

#[test]
fn selected_text_joins() {
    let (b, s) = setup("«ab»c\n«中»x|");
    assert_eq!(selected_text(&b, &s), "ab\n中\n");
    let (b, s) = setup("«a\nb»");
    assert_eq!(selected_text(&b, &s), "a\nb");
}

#[test]
fn word_at() {
    let (b, _) = setup("|");
    assert_eq!(word_range_at(&b, 0), None);
    let b = crate::Buffer::new("foo_1 bar.baz");
    assert_eq!(word_range_at(&b, 0), Some(0..5));
    assert_eq!(word_range_at(&b, 3), Some(0..5));
    assert_eq!(word_range_at(&b, 5), Some(0..5));
    assert_eq!(word_range_at(&b, 6), Some(6..9));
    assert_eq!(word_range_at(&b, 9), Some(6..9));
    assert_eq!(word_range_at(&b, 13), Some(10..13));
    let b = crate::Buffer::new("a  b");
    assert_eq!(word_range_at(&b, 2), None);
}

#[test]
fn ctrl_d_flow() {
    let (b, mut s) = setup("fo|o bar foo\nfoo");
    assert!(select_next_occurrence(&b, &mut s));
    assert_eq!(show(&b, &s), "«foo» bar foo\nfoo");
    assert!(select_next_occurrence(&b, &mut s));
    assert_eq!(show(&b, &s), "«foo» bar «foo»\nfoo");
    assert_eq!(s.primary().range(), 8..11);
    assert!(select_next_occurrence(&b, &mut s));
    assert_eq!(s.len(), 3);
    assert!(!select_next_occurrence(&b, &mut s)); // all selected
    assert_eq!(s.len(), 3);
}

#[test]
fn ctrl_d_wraps_and_is_case_sensitive() {
    let (b, mut s) = setup("Foo foo «foo»");
    assert!(select_next_occurrence(&b, &mut s));
    assert_eq!(show(&b, &s), "Foo «foo» «foo»");
    assert!(!select_next_occurrence(&b, &mut s));
}

#[test]
fn ctrl_d_adjacent_and_overlapping() {
    let (b, mut s) = setup("«aa»aa");
    assert!(select_next_occurrence(&b, &mut s));
    assert_eq!(show(&b, &s), "«aa»«aa»");
    let (b, mut s) = setup("«aa»a");
    assert!(!select_next_occurrence(&b, &mut s));
    let (b, mut s) = setup("|  ");
    assert!(!select_next_occurrence(&b, &mut s));
}

#[test]
fn ctrl_d_then_type_edits_all() {
    let (mut b, mut s) = setup("x|y = xy + xy");
    select_next_occurrence(&b, &mut s);
    select_next_occurrence(&b, &mut s);
    select_next_occurrence(&b, &mut s);
    insert_text(&mut b, &mut s, "zz");
    assert_eq!(show(&b, &s), "zz| = zz| + zz|");
}
