use super::*;

fn b(text: &str) -> Buffer {
    Buffer::new(text)
}

#[test]
fn left_right_graphemes() {
    let buf = b("ae\u{301}😀\n中");
    assert_eq!(right(&buf, 0), 1);
    assert_eq!(right(&buf, 1), 3); // e + combining accent
    assert_eq!(right(&buf, 3), 4);
    assert_eq!(right(&buf, 4), 5); // newline
    assert_eq!(right(&buf, 6), 6);
    assert_eq!(left(&buf, 6), 5);
    assert_eq!(left(&buf, 3), 1);
    assert_eq!(left(&buf, 0), 0);
    assert_eq!(right(&buf, 99), 6);
}

#[test]
fn zwj_and_flag_are_single_steps() {
    let family = "👨\u{200d}👩\u{200d}👧";
    let buf = b(&format!("{family}🇯🇵x"));
    let after_family = family.chars().count();
    assert_eq!(right(&buf, 0), after_family);
    assert_eq!(right(&buf, after_family), after_family + 2);
    assert_eq!(left(&buf, after_family + 2), after_family);
    assert_eq!(left(&buf, after_family), 0);
}

#[test]
fn line_and_buffer_edges() {
    let buf = b("  foo\n\tbar\n   \nx");
    assert_eq!(line_start(&buf, 4), 0);
    assert_eq!(line_end(&buf, 1), 5);
    assert_eq!(buffer_start(&buf), 0);
    assert_eq!(buffer_end(&buf), buf.len_chars());
    assert_eq!(first_non_whitespace(&buf, 0), 2);
    assert_eq!(first_non_whitespace(&buf, 1), 7);
    assert_eq!(first_non_whitespace(&buf, 2), 14); // blank line -> line end
    assert_eq!(line_start_smart(&buf, 4), 2);
    assert_eq!(line_start_smart(&buf, 2), 0);
    assert_eq!(line_start_smart(&buf, 0), 2);
    assert_eq!(line_start_smart(&buf, 9), 7);
}

#[test]
fn display_columns_with_tabs() {
    let buf = b("\tab\tc\n12\t3");
    assert_eq!(display_column(&buf, 0, 4), 0);
    assert_eq!(display_column(&buf, 1, 4), 4);
    assert_eq!(display_column(&buf, 3, 4), 6);
    assert_eq!(display_column(&buf, 4, 4), 8);
    assert_eq!(display_column(&buf, 8, 4), 2);
    assert_eq!(display_column(&buf, 9, 4), 4);
    assert_eq!(display_column(&buf, 10, 4), 5);
    assert_eq!(display_column(&buf, 9, 0), 3); // tab width 0 behaves as 1
    assert_eq!(char_for_display_column(&buf, 0, 0, 4), 0);
    assert_eq!(char_for_display_column(&buf, 0, 2, 4), 0); // inside tab
    assert_eq!(char_for_display_column(&buf, 0, 4, 4), 1);
    assert_eq!(char_for_display_column(&buf, 0, 5, 4), 2);
    assert_eq!(char_for_display_column(&buf, 0, 99, 4), 5);
    assert_eq!(char_for_display_column(&buf, 1, 3, 4), 8);
    assert_eq!(char_for_display_column(&buf, 9, 1, 4), 7); // line clamped
}

#[test]
fn display_column_snaps_to_grapheme() {
    let buf = b("ae\u{301}x\nabcd");
    // Column 2 is between 'e' and its accent; snap to the cluster start.
    assert_eq!(char_for_display_column(&buf, 0, 2, 4), 1);
    assert_eq!(char_for_display_column(&buf, 0, 3, 4), 3);
}

#[test]
fn up_down_keep_goal() {
    let buf = b("hello world\nhi\n\tlonger line");
    let (p, g) = down(&buf, 8, None, 4);
    assert_eq!((p, g), (14, 8)); // clamped to end of "hi"
    let (p, g) = down(&buf, p, Some(g), 4);
    assert_eq!(p, 15 + 5); // tab=4, then 4 chars to column 8
    assert_eq!(g, 8);
    let (p, _) = up(&buf, p, Some(g), 4);
    assert_eq!(p, 14);
    let (p, _) = up(&buf, p, Some(g), 4);
    assert_eq!(p, 8);
    assert_eq!(up(&buf, 8, Some(8), 4), (0, 8)); // first line -> start
    let end = buf.len_chars();
    assert_eq!(down(&buf, 17, None, 4).0, end); // last line -> end
}

#[test]
fn up_down_unicode() {
    let buf = b("中文字\n😀😀😀x");
    let (p, g) = down(&buf, 2, None, 4);
    assert_eq!((p, g), (6, 2));
    let (p, _) = up(&buf, 8, None, 4);
    assert_eq!(p, 3); // column 4 clamps to end of line 0
}

#[test]
fn paging() {
    let text: String = (0..50).map(|i| format!("line{i}\n")).collect();
    let buf = b(&text);
    let start = buf.line_start(10) + 2;
    let (p, g) = page_down(&buf, start, None, 20, 4);
    assert_eq!(buf.char_to_point(p).line, 30);
    assert_eq!(g, 2);
    let (p, _) = page_up(&buf, p, Some(g), 25, 4);
    assert_eq!(buf.char_to_point(p).line, 5);
    let (p, _) = page_up(&buf, p, Some(g), 25, 4);
    assert_eq!(buf.char_to_point(p).line, 0);
    assert_eq!(buf.char_to_point(p).column, 2);
    assert_eq!(page_up(&buf, p, Some(g), 25, 4).0, 0);
    let (p, _) = page_down(&buf, p, Some(g), 1000, 4);
    assert_eq!(p, buf.len_chars()); // last line is empty
}

#[test]
fn word_moves() {
    let buf = b("foo.bar  baz_1(x)\n  next");
    assert_eq!(word_right(&buf, 0), 3);
    assert_eq!(word_right(&buf, 3), 4);
    assert_eq!(word_right(&buf, 4), 7);
    assert_eq!(word_right(&buf, 7), 14);
    assert_eq!(word_right(&buf, 14), 15);
    assert_eq!(word_right(&buf, 17), 24); // across newline + indent
    assert_eq!(word_right(&buf, 24), 24);
    assert_eq!(word_left(&buf, 24), 20);
    assert_eq!(word_left(&buf, 20), 16);
    assert_eq!(word_left(&buf, 14), 9);
    assert_eq!(word_left(&buf, 9), 4);
    assert_eq!(word_left(&buf, 4), 3);
    assert_eq!(word_left(&buf, 3), 0);
    assert_eq!(word_left(&buf, 0), 0);
}

#[test]
fn word_moves_unicode() {
    let buf = b("héllo 中文 😀😀 x");
    assert_eq!(word_right(&buf, 0), 5);
    assert_eq!(word_right(&buf, 5), 8);
    assert_eq!(word_right(&buf, 8), 11); // emoji are punctuation-class
    assert_eq!(word_left(&buf, 11), 9);
    assert_eq!(word_left(&buf, 9), 6);
}

#[test]
fn selection_helpers() {
    let buf = b("abc def\nxyz");
    let mut sels =
        SelectionSet::from_vec(vec![Selection::cursor(1), Selection::cursor(9)], 0).expect("set");
    move_selections(&buf, &mut sels, true, word_right);
    let ranges: Vec<_> = sels.iter().map(|s| (s.anchor, s.head)).collect();
    assert_eq!(ranges, vec![(1, 3), (9, 11)]);
    move_left(&buf, &mut sels, false);
    let heads: Vec<_> = sels.iter().map(|s| s.head).collect();
    assert_eq!(heads, vec![1, 9]);
    move_right(&buf, &mut sels, false);
    let heads: Vec<_> = sels.iter().map(|s| s.head).collect();
    assert_eq!(heads, vec![2, 10]);
    move_right(&buf, &mut sels, true);
    move_right(&buf, &mut sels, false);
    let heads: Vec<_> = sels.iter().map(|s| s.head).collect();
    assert_eq!(heads, vec![3, 11]);
    move_selections_vertically(&buf, &mut sels, false, |b, p, g| up(b, p, g, 4));
    let heads: Vec<_> = sels.iter().map(|s| (s.head, s.goal_column)).collect();
    assert_eq!(heads, vec![(0, Some(3)), (3, Some(3))]);
    move_left(&buf, &mut sels, true);
    assert_eq!(sels.len(), 2);
}
