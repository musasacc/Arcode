//! Insert entries, Insert-mode keys and Replace mode.

mod common;

use arcode_vim::{Mode, VimAction};
use common::{Session, check_table};

#[test]
fn entries() {
    check_table(&[
        ("|bc", "ia<esc>", "|abc"),
        ("|ac", "ab<esc>", "a|bc"),
        ("|", "aab<esc>", "a|b"),
        ("  b|c", "Ia<esc>", "  |abc"),
        ("  b|c", "gIa<esc>", "|a  bc"),
        ("a|b", "Ac<esc>", "ab|c"),
        ("|a", "ob<esc>", "a\n|b"),
        ("|a", "Ob<esc>", "|b\na"),
        ("  |a", "ob<esc>", "  a\n  |b"),
        ("  |a", "Ob<esc>", "  |b\n  a"),
        ("|fn {", "ox<esc>", "fn {\n    |x"),
        ("|b", "3ia<esc>", "aa|ab"),
        ("|a", "3ox<esc>", "a\nx\nx\n|x"),
        ("|a", "2Ax<esc>", "ax|x"),
        ("|ab", "ix<esc>lgiy<esc>", "x|yab"),
    ]);
}

#[test]
fn editing_keys() {
    check_table(&[
        ("|ac", "ab<bs><bs>X<esc>", "|Xc"),
        ("a|", "a<bs><bs>x<esc>", "|x"),
        ("a\n|b", "i<bs><esc>", "|ab"),
        ("|ab", "a<cr><esc>", "a\n|b"),
        ("  |a", "A<cr>b<esc>", "  a\n  |b"),
        ("|f(", "A<cr>x<esc>", "f(\n    |x"),
        ("|{}", "a<cr>x<esc>", "{\n    |x\n}"),
        ("|a", "i<tab><esc>", "   | a"),
        ("|", "a  <tab>x<esc>", "    |x"),
        ("|", "afoo bar<c-w>X<esc>", "foo |X"),
        ("|", "a    foo<c-u>x<esc>", "    |x"),
        ("|ab", "a<del><esc>", "|a"),
        ("|", "a        <bs>x<esc>", "    |x"),
        ("|a", "yli<c-r>\"<esc>", "|aa"),
        ("|ab", "ix<right>y<esc>", "xa|yb"),
        ("|ab", "A<left><left>x<esc>", "|xab"),
        ("|a\nbc", "A<down>x<esc>", "a\nb|xc"),
    ]);
}

#[test]
fn replace_mode() {
    check_table(&[
        ("|abc", "Rxy<esc>", "x|yc"),
        ("|abc", "Rxy<bs><esc>", "|xbc"),
        ("|a", "Rxyz<esc>", "xy|z"),
        ("|a", "Rxy<bs><bs><esc>", "|a"),
        // A count repeats the replacement, as in Vim.
        ("|ab", "2Rx<esc>", "x|x"),
    ]);
    let mut s = Session::new("|abc");
    s.keys("R");
    assert_eq!(s.mode(), Mode::Replace);
    s.keys("x<esc>");
    assert_eq!(s.mode(), Mode::Normal);
}

#[test]
fn passthrough_and_mode() {
    let mut s = Session::new("|a");
    s.keys("i");
    assert_eq!(s.mode(), Mode::Insert);
    assert_eq!(s.last_action(), Some(&VimAction::ModeChanged(Mode::Insert)));
    s.keys("<c-s>");
    assert_eq!(s.last_action(), Some(&VimAction::PassThrough));
    s.keys("<a-x>");
    assert_eq!(s.last_action(), Some(&VimAction::PassThrough));
    s.keys("<esc>");
    assert_eq!(s.last_action(), Some(&VimAction::ModeChanged(Mode::Normal)));
    s.keys("<c-p>");
    assert_eq!(s.state(), "|a");
    s.keys("<c-s>");
    assert_eq!(s.last_action(), Some(&VimAction::PassThrough));
}

#[test]
fn insert_text_api_is_recorded() {
    let ctx = common::ctx();
    let mut s = Session::new("|x\ny");
    s.keys("i");
    s.vim
        .insert_text("ab\ncd", &mut s.buffer, &mut s.sels, &ctx);
    s.keys("<esc>j0.");
    assert_eq!(s.buffer.to_string(), "ab\ncdx\nab\ncdy");
    s.keys("u");
    assert_eq!(s.buffer.to_string(), "ab\ncdx\ny");
}

#[test]
fn dot_register_holds_last_insert() {
    let mut s = Session::new("|");
    s.keys("ihello<esc>");
    assert_eq!(s.reg('.').as_deref(), Some("hello"));
    s.keys("\".p");
    assert_eq!(s.buffer.to_string(), "hellohello");
}
