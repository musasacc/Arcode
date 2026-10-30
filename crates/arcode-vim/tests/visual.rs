//! Visual, Visual Line and Visual Block modes.

mod common;

use arcode_vim::Mode;
use common::{Session, check_table};

#[test]
fn selection_rendering() {
    check_table(&[
        ("|abc", "v", "«a»bc"),
        ("|abc", "vl", "«ab»c"),
        ("ab|c", "vh", "a«bc»"),
        ("|ab\ncd", "Vj", "«ab\ncd»"),
        ("|ab\ncd\nef", "V", "«ab\n»cd\nef"),
        ("|ab\ncd", "v$", "«ab\n»cd"),
        ("|abc\ndef", "<c-v>j", "«a»bc\n«d»ef"),
        ("|abc\ndef", "<c-v>jl", "«ab»c\n«de»f"),
        ("|abc\nd\nefg", "<c-v>jjl", "«ab»c\n«d»\n«ef»g"),
        ("|abc\n\nefg", "<c-v>jjl", "«ab»c\n\n«ef»g"),
        ("|abc\nde", "<c-v>j$", "«abc»\n«de»"),
        ("|abc def", "viw", "«abc» def"),
        ("|abc def", "vaw", "«abc »def"),
        ("|abc def", "vee", "«abc def»"),
        ("|abc", "vl<esc>", "a|bc"),
        ("|abc", "vlv", "a|bc"),
        ("|abc", "vV", "«abc»"),
        ("|abc\nd", "Vv", "«a»bc\nd"),
        ("a|b\nc\n\nd", "vip", "«ab\nc\n»\nd"),
        ("a|bcd", "vlo", "a«bc»d"),
    ]);
}

#[test]
fn operators() {
    check_table(&[
        ("|abc", "vld", "|c"),
        ("a|bc", "vhd", "|c"),
        ("|ab\ncd", "Vd", "|cd"),
        ("|ab\ncd\nef", "Vjd", "|ef"),
        ("|ab\ncd", "v$d", "|cd"),
        ("|abc def", "viwd", "| def"),
        ("|abc def", "vawd", "|def"),
        ("|abc", "vlx", "|c"),
        ("|abc\nd", "vX", "|d"),
        ("|abc\nd", "vD", "|d"),
        ("|abc", "vlcX<esc>", "|Xc"),
        ("|abc", "vlsX<esc>", "|Xc"),
        ("|ab\ncd", "vSX<esc>", "|X\ncd"),
        ("|abc", "vl~", "|ABc"),
        ("|ABC", "vlu", "|abC"),
        ("|abc", "vlU", "|ABc"),
        ("|abc", "vlgU", "|ABc"),
        ("|a\nb", "Vj>", "    |a\n    b"),
        ("|a\nb", "Vj2>", "        |a\n        b"),
        ("    |a\nb", "V<", "|a\nb"),
        ("|a\nb\nc", "VjjJ", "a b| c"),
        ("|a\nb", "vJ", "a| b"),
        ("|abc", "vlrx", "|xxc"),
        ("|ab\ncd", "Vrx", "|xx\ncd"),
        ("a|bcd", "vlohd", "|d"),
        ("|abc", "vl<esc>gvd", "|c"),
        ("|a\nb", "Vjgc", "|// a\n// b"),
        ("|a\n b", "Vj=", "|a\nb"),
        ("a|bcd", "vly", "a|bcd"),
        ("ab|cd", "vhyP", "ab|cbcd"),
        ("|ab\ncd", "VyjP", "ab\n|ab\ncd"),
    ]);
    let mut s = Session::new("a|bcd");
    s.keys("vly");
    assert_eq!(s.reg('"').as_deref(), Some("bc"));
    assert_eq!(s.mode(), Mode::Normal);
}

#[test]
fn put_over_selection() {
    check_table(&[
        ("|foo bar", "yiwwviwp", "foo fo|o"),
        ("|foo bar", "yiwwviwpu", "foo |bar"),
        ("|a\nb", "yyjVp", "a\n|a"),
        ("|ab\ncd", "ylVjp", "|a"),
        ("|ab cd", "yyjwviwp", "ab \n|ab cd\n"),
    ]);
    let mut s = Session::new("|foo bar");
    s.keys("yiwwviwp");
    assert_eq!(s.reg('"').as_deref(), Some("bar"));
    let mut s = Session::new("|foo bar");
    s.keys("yiwwviwP");
    assert_eq!(s.reg('"').as_deref(), Some("foo"));
}

#[test]
fn block_mode() {
    check_table(&[
        ("|abc\ndef", "<c-v>jld", "|c\nf"),
        ("|abc\ndef", "<c-v>jIX<esc>", "|Xabc\nXdef"),
        ("|abc\ndef", "<c-v>jAX<esc>", "a|Xbc\ndXef"),
        ("|ab\nabcd", "<c-v>j$AX<esc>", "ab|X\nabcdX"),
        ("|d\nabc", "<c-v>jllAX<esc>", "d  |X\nabcX"),
        ("|abc\ndef", "<c-v>jcX<esc>", "|Xbc\nXef"),
        ("|abc\ndef", "<c-v>jlyP", "|ababc\ndedef"),
        ("|abc\ndef", "<c-v>jly$p", "abc|ab\ndefde"),
        ("|abc\ndef", "<c-v>j~", "|Abc\nDef"),
        ("|abc\ndef", "<c-v>jrx", "|xbc\nxef"),
        ("|abc\ndef", "<c-v>j>", "    |abc\n    def"),
        ("|ab\ncd", "<c-v>jlo", "«ab»\n«cd»"),
        ("|abc\ndef", "<c-v>jlx", "|c\nf"),
        ("|ab\ncd", "<c-v>jyjp", "ab\nc|ad\n c"),
        ("|ab\ncd", "<c-v>jy$p", "ab|a\ncdc"),
    ]);
    let mut s = Session::new("|abc\ndef");
    s.keys("<c-v>j");
    assert_eq!(s.mode(), Mode::VisualBlock);
    assert_eq!(s.sels.len(), 2);
    assert_eq!(s.vim.cursor_positions(&s.sels), vec![4]);
    s.keys("l");
    assert_eq!(s.vim.cursor_positions(&s.sels), vec![5]);
}

#[test]
fn modes_and_mouse_selection() {
    let mut s = Session::new("|abc");
    s.keys("v");
    assert_eq!(s.mode(), Mode::Visual);
    s.keys("V");
    assert_eq!(s.mode(), Mode::VisualLine);
    s.keys("<esc>");
    assert_eq!(s.mode(), Mode::Normal);
    // The editor selected text with the mouse: Vim enters Visual mode.
    s.sels = arcode_buffer::SelectionSet::from_selection(arcode_buffer::Selection::new(0, 2));
    s.keys("d");
    assert_eq!(s.state(), "|c");
    assert_eq!(s.mode(), Mode::Normal);
}

#[test]
fn visual_command_line_range() {
    check_table(&[
        ("|a\na\na", "Vj:s/a/b/<cr>", "b\n|b\na"),
        ("|a a\na a\na", "vj:s/a/b/g<cr>", "b b\n|b b\na"),
    ]);
}
