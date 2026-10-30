use super::test_util::{setup, show};
use super::*;

fn cfg() -> OpsConfig {
    OpsConfig::default()
}

fn typed(marked: &str, text: &str) -> String {
    let (mut b, mut s) = setup(marked);
    type_text(&mut b, &mut s, text, &cfg());
    show(&b, &s)
}

#[test]
fn insert_at_multiple_cursors() {
    let (mut b, mut s) = setup("a|b|c\nd|");
    let edits = insert_text(&mut b, &mut s, "XY");
    assert_eq!(show(&b, &s), "aXY|bXY|c\ndXY|");
    assert_eq!(edits.len(), 3);
}

#[test]
fn insert_replaces_selections() {
    let (mut b, mut s) = setup("«foo» bar «baz»");
    insert_text(&mut b, &mut s, "中😀");
    assert_eq!(show(&b, &s), "中😀| bar 中😀|");
}

#[test]
fn insert_normalizes_crlf() {
    let (mut b, mut s) = setup("a|");
    insert_text(&mut b, &mut s, "1\r\n2");
    assert_eq!(show(&b, &s), "a1\n2|");
}

#[test]
fn typing_merges_into_one_undo_step() {
    let (mut b, mut s) = setup("x|\ny|");
    for c in ["a", "b", "c"] {
        type_text(&mut b, &mut s, c, &cfg());
    }
    assert_eq!(show(&b, &s), "xabc|\nyabc|");
    let undo = b.undo().expect("undo");
    assert_eq!(b.to_string(), "x\ny");
    assert_eq!(show(&b, &undo.selections), "x|\ny|");
    assert!(b.undo().is_none());
    let redo = b.redo().expect("redo");
    assert_eq!(show(&b, &redo.selections), "xabc|\nyabc|");
}

#[test]
fn newline_breaks_typing_group() {
    let (mut b, mut s) = setup("|");
    type_text(&mut b, &mut s, "a", &cfg());
    type_text(&mut b, &mut s, "\n", &cfg());
    type_text(&mut b, &mut s, "b", &cfg());
    assert_eq!(b.to_string(), "a\nb");
    b.undo();
    assert_eq!(b.to_string(), "a\n");
    b.undo();
    assert_eq!(b.to_string(), "a");
}

#[test]
fn paste_distributes_lines() {
    let (mut b, mut s) = setup("|a\n|b");
    paste(&mut b, &mut s, "1\n2");
    assert_eq!(show(&b, &s), "1|a\n2|b");
    let (mut b, mut s) = setup("|a\n|b");
    paste(&mut b, &mut s, "1\n2\n3");
    assert_eq!(show(&b, &s), "1\n2\n3|a\n1\n2\n3|b");
    let (mut b, mut s) = setup("|");
    paste(&mut b, &mut s, "x");
    type_text(&mut b, &mut s, "y", &cfg());
    b.undo();
    assert_eq!(b.to_string(), "x"); // paste is its own undo step
}

#[test]
fn auto_close_pairs() {
    assert_eq!(typed("|", "("), "(|)");
    assert_eq!(typed("a |", "["), "a [|]");
    assert_eq!(typed("|", "{"), "{|}");
    assert_eq!(typed("|", "\""), "\"|\"");
    assert_eq!(typed("x = |;", "'"), "x = '|';");
}

#[test]
fn auto_close_skips_before_word_and_after_word_quote() {
    assert_eq!(typed("|foo", "("), "(|foo");
    assert_eq!(typed("don|", "'"), "don'|");
    let mut c = cfg();
    c.auto_close.skip_before_word = false;
    let (mut b, mut s) = setup("|foo");
    type_text(&mut b, &mut s, "(", &c);
    assert_eq!(show(&b, &s), "(|)foo");
}

#[test]
fn closing_char_moves_over() {
    assert_eq!(typed("(|)", ")"), "()|");
    assert_eq!(typed("\"|\"", "\""), "\"\"|");
    assert_eq!(typed("|x", ")"), ")|x");
    // Typing "()" in sequence ends after the pair.
    let (mut b, mut s) = setup("|");
    type_text(&mut b, &mut s, "(", &cfg());
    type_text(&mut b, &mut s, ")", &cfg());
    assert_eq!(show(&b, &s), "()|");
}

#[test]
fn auto_close_multi_cursor_mixed() {
    // One cursor before a word, one free, one before the closer.
    assert_eq!(typed("|ab |\n(|)", "("), "(|ab (|)\n((|))");
    assert_eq!(typed("(|) (|)", ")"), "()| ()|");
}

#[test]
fn auto_close_surrounds_selection() {
    assert_eq!(typed("«foo» »bar«", "("), "(«foo») (»bar«)");
    let mut c = cfg();
    c.auto_close.surround = false;
    let (mut b, mut s) = setup("«foo»");
    type_text(&mut b, &mut s, "(", &c);
    assert_eq!(show(&b, &s), "(|");
}

#[test]
fn auto_close_disabled() {
    let c = OpsConfig {
        auto_close: AutoClose::disabled(),
        ..cfg()
    };
    let (mut b, mut s) = setup("(|)");
    type_text(&mut b, &mut s, "(", &c);
    type_text(&mut b, &mut s, ")", &c);
    assert_eq!(show(&b, &s), "(()|)");
}

#[test]
fn multi_char_type_is_plain_insert() {
    assert_eq!(typed("|", "(x"), "(x|");
}

#[test]
fn newline_copies_indent() {
    let (mut b, mut s) = setup("    foo|\n\tbar|");
    newline_and_indent(&mut b, &mut s, &cfg());
    assert_eq!(show(&b, &s), "    foo\n    |\n\tbar\n\t|");
}

#[test]
fn newline_indents_after_opener() {
    for opener in ["{", "(", "[", ":"] {
        let (mut b, mut s) = setup(&format!("  if x{opener}|"));
        newline_and_indent(&mut b, &mut s, &cfg());
        assert_eq!(show(&b, &s), format!("  if x{opener}\n      |"));
    }
    let (mut b, mut s) = setup("fn x() {  |");
    newline_and_indent(&mut b, &mut s, &cfg());
    assert_eq!(show(&b, &s), "fn x() {  \n    |");
}

#[test]
fn newline_between_brackets() {
    let (mut b, mut s) = setup("  fn x() {|}");
    newline_and_indent(&mut b, &mut s, &cfg());
    assert_eq!(show(&b, &s), "  fn x() {\n      |\n  }");
    let c = OpsConfig {
        indent: IndentUnit::Tab,
        ..cfg()
    };
    let (mut b, mut s) = setup("[|]\n(|)");
    newline_and_indent(&mut b, &mut s, &c);
    assert_eq!(show(&b, &s), "[\n\t|\n]\n(\n\t|\n)");
}

#[test]
fn newline_replaces_selection_and_cursor_in_indent() {
    let (mut b, mut s) = setup("a«bc»d");
    newline_and_indent(&mut b, &mut s, &cfg());
    assert_eq!(show(&b, &s), "a\n|d");
    let (mut b, mut s) = setup("  |  x");
    newline_and_indent(&mut b, &mut s, &cfg());
    assert_eq!(show(&b, &s), "  \n  |  x");
}

#[test]
fn tab_inserts_to_next_stop() {
    let (mut b, mut s) = setup("|x\nab|c");
    insert_tab(&mut b, &mut s, &cfg());
    assert_eq!(show(&b, &s), "    |x\nab  |c");
    let c = OpsConfig {
        indent: IndentUnit::Tab,
        ..cfg()
    };
    let (mut b, mut s) = setup("a|");
    insert_tab(&mut b, &mut s, &c);
    assert_eq!(show(&b, &s), "a\t|");
    let (mut b, mut s) = setup("«a\nb»");
    insert_tab(&mut b, &mut s, &cfg());
    assert_eq!(show(&b, &s), "«    a\n    b»");
}

#[test]
fn typing_after_cursor_jump_starts_new_step() {
    let (mut b, mut s) = setup("|ab");
    type_text(&mut b, &mut s, "x", &cfg());
    s = crate::SelectionSet::single(3);
    type_text(&mut b, &mut s, "y", &cfg());
    type_text(&mut b, &mut s, "z", &cfg());
    assert_eq!(b.to_string(), "xabyz");
    let u = b.undo().expect("undo");
    assert_eq!(show(&b, &u.selections), "xab|");
    let u = b.undo().expect("undo");
    assert_eq!(show(&b, &u.selections), "|ab");
}

#[test]
fn crlf_typed_is_newline() {
    let (mut b, mut s) = setup("  x|");
    type_text(&mut b, &mut s, "\r\n", &cfg());
    assert_eq!(show(&b, &s), "  x\n  |");
}
