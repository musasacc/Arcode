//! Normal-mode motions with counts.

mod common;

use arcode_buffer::Buffer;
use common::{assert_vim, check_table, parse, render};

#[test]
fn harness_round_trip() {
    let (text, sels) = parse("a|b«cd»e");
    assert_eq!(text, "abcde");
    assert_eq!(sels.len(), 2);
    assert_eq!(render(&Buffer::new(&text), &sels), "a|b«cd»e");
}

#[test]
fn horizontal_and_vertical() {
    check_table(&[
        ("|hello world", "l", "h|ello world"),
        ("hel|lo", "h", "he|llo"),
        ("|hello", "3l", "hel|lo"),
        ("h|ello", "10l", "hell|o"),
        ("he|llo", "5h", "|hello"),
        ("|hello", "<space><space>", "he|llo"),
        ("he|llo", "<bs>", "h|ello"),
        ("|ab", "<right>", "a|b"),
        ("ab|c\ndef", "j", "abc\nde|f"),
        ("abc\nd|ef", "k", "a|bc\ndef"),
        ("abc|d\nx\nabcd", "j", "abcd\n|x\nabcd"),
        ("abc|d\nx\nabcd", "jj", "abcd\nx\nabc|d"),
        ("|a\nb\nc", "2j", "a\nb\n|c"),
        ("|a\nb\nc", "9j", "a\nb\n|c"),
        ("a\nb\n|c", "2k", "|a\nb\nc"),
        ("|a\nb", "gj", "a\n|b"),
        ("a\n|b", "gk", "|a\nb"),
        ("|a\nb", "<down>", "a\n|b"),
        ("|a", "k", "|a"),
    ]);
}

#[test]
fn word_motions() {
    check_table(&[
        ("|foo bar", "w", "foo |bar"),
        ("|foo.bar baz", "w", "foo|.bar baz"),
        ("|foo.bar baz", "W", "foo.bar |baz"),
        ("|a b c d", "3w", "a b c |d"),
        ("fo|o\n  bar", "w", "foo\n  |bar"),
        ("foo |bar", "w", "foo ba|r"),
        ("|a\n\nb", "w", "a\n|\nb"),
        ("|foo bar", "e", "fo|o bar"),
        ("fo|o bar", "e", "foo ba|r"),
        ("|a.b c.d", "E", "a.|b c.d"),
        ("|a b c", "2e", "a b |c"),
        ("foo ba|r", "b", "foo |bar"),
        ("foo |bar", "b", "|foo bar"),
        ("a.b c.|d", "B", "a.b |c.d"),
        ("a b |c", "2b", "|a b c"),
        ("foo ba|r", "ge", "fo|o bar"),
        ("a.b c.|d", "gE", "a.|b c.d"),
        ("a\n\n|b", "b", "a\n|\nb"),
    ]);
}

#[test]
fn line_motions() {
    check_table(&[
        ("  fo|o", "0", "|  foo"),
        ("  fo|o", "^", "  |foo"),
        ("  fo|o", "<home>", "|  foo"),
        ("|foo\nbar", "$", "fo|o\nbar"),
        ("|foo\nbar", "<end>", "fo|o\nbar"),
        ("|foo\nbar", "2$", "foo\nba|r"),
        ("|abc\nabcdef", "$j", "abc\nabcde|f"),
        ("|ab  ", "g_", "a|b  "),
        ("  a|b", "_", "  |ab"),
        ("|a\n  b", "+", "a\n  |b"),
        ("|a\n  b", "<cr>", "a\n  |b"),
        ("a\n  |b", "-", "|a\n  b"),
        ("|abcdef", "4|", "abc|def"),
        ("|a\nb\n  c", "G", "a\nb\n  |c"),
        ("a\nb\n|c", "gg", "|a\nb\nc"),
        ("|a\nb\nc", "2G", "a\n|b\nc"),
        ("|a\nb\nc", "3gg", "a\nb\n|c"),
    ]);
}

#[test]
fn find_and_repeat() {
    check_table(&[
        ("|a,b,c", "f,", "a|,b,c"),
        ("|a,b,c", "2f,", "a,b|,c"),
        ("|abc,d", "t,", "ab|c,d"),
        ("a,b,|c", "F,", "a,b|,c"),
        ("a,bc|d", "T,", "a,|bcd"),
        ("|a,b,c", "f,;", "a,b|,c"),
        ("|a,b,c", "f,;,", "a|,b,c"),
        ("|a,b,c", "t,;", "a,|b,c"),
        ("|a,b,c", "fz", "|a,b,c"),
        ("|a\n,", "f,", "|a\n,"),
    ]);
}

#[test]
fn brackets_and_paragraphs() {
    check_table(&[
        ("|f(a[1])", "%", "f(a[1]|)"),
        ("f(a[1]|)", "%", "f|(a[1])"),
        ("f(a|[1])", "%", "f(a[1|])"),
        ("|1\n2\n3\n4", "50%", "1\n|2\n3\n4"),
        ("|a\nb\n\nc", "}", "a\nb\n|\nc"),
        ("a\n\nb\n|c", "{", "a\n|\nb\nc"),
        ("|a\n\nb\n\nc", "2}", "a\n\nb\n|\nc"),
        ("|a\nb", "}", "a\n|b"),
    ]);
}

#[test]
fn screen_and_scroll() {
    let text = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14";
    let at0 = format!("|{text}");
    check_table(&[
        ("a\nb\nc\nd\n|e", "H", "|a\nb\nc\nd\ne"),
        ("|a\nb\nc\nd\ne", "L", "a\nb\nc\nd\n|e"),
        ("|a\nb\nc\nd\ne", "M", "a\nb\n|c\nd\ne"),
        ("|a\nb\nc\nd\ne", "2H", "a\n|b\nc\nd\ne"),
        (
            &at0,
            "<c-d>",
            "1\n2\n3\n4\n5\n|6\n7\n8\n9\n10\n11\n12\n13\n14",
        ),
        (&at0, "<c-d><c-u>", &at0),
        (
            &at0,
            "<c-f>",
            "1\n2\n3\n4\n5\n6\n7\n8\n|9\n10\n11\n12\n13\n14",
        ),
        (&at0, "<c-f><c-b>", &at0),
        (
            &at0,
            "3<c-d>",
            "1\n2\n3\n|4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14",
        ),
    ]);
    let mut s = common::Session::new(&at0);
    s.keys("<c-d>");
    assert_eq!(
        s.last_action(),
        Some(&arcode_vim::VimAction::ScrollLines(5))
    );
    s.keys("<c-e>");
    assert_eq!(
        s.last_action(),
        Some(&arcode_vim::VimAction::ScrollLines(1))
    );
    s.keys("zt");
    assert_eq!(
        s.last_action(),
        Some(&arcode_vim::VimAction::ScrollCursorTop)
    );
    s.keys("zz");
    assert_eq!(
        s.last_action(),
        Some(&arcode_vim::VimAction::ScrollCursorCenter)
    );
    s.keys("zb");
    assert_eq!(
        s.last_action(),
        Some(&arcode_vim::VimAction::ScrollCursorBottom)
    );
}

#[test]
fn search_motions() {
    check_table(&[
        ("|foo bar foo", "*", "foo bar |foo"),
        ("foo bar |foo", "#", "|foo bar foo"),
        ("|foo foobar foo", "*", "foo foobar |foo"),
        ("|foo foobar foo", "g*", "foo |foobar foo"),
        ("|x a x a x", "/a<cr>", "x |a x a x"),
        ("|x a x a x", "/a<cr>n", "x a x |a x"),
        ("|x a x a x", "/a<cr>nN", "x |a x a x"),
        ("x a x |a x", "?a<cr>", "x |a x a x"),
        ("x a x |a x", "?a<cr>n", "x a x |a x"),
        ("x |a x", "/x<cr>n", "|x a x"),
        ("|x A x a", "/a\\c<cr>", "x |A x a"),
        ("|x a x a", "/a<cr>/<cr>", "x a x |a"),
        ("|a b", "/zzz<cr>", "|a b"),
        ("|ab", "/b<bs><bs>l", "a|b"),
    ]);
    let mut s = common::Session::new("|foo foo");
    s.keys("*");
    let q = s.vim.search_query().expect("highlight");
    assert!(q.whole_word && q.pattern == "foo");
    s.keys(":noh<cr>");
    assert!(s.vim.search_query().is_none());
    s.keys("/zz<cr>");
    assert_eq!(
        s.last_action(),
        Some(&arcode_vim::VimAction::StatusMessage(
            "E486: Pattern not found: zz".into()
        ))
    );
}

#[test]
fn marks_and_jumps() {
    check_table(&[
        ("|a\nb\nc", "majj'a", "|a\nb\nc"),
        ("a|bc\nd", "mxj`x", "a|bc\nd"),
        ("|a\nb\nc", "G''", "|a\nb\nc"),
        ("a|b\nc\nd", "G``", "a|b\nc\nd"),
        ("|a\nb", "'z", "|a\nb"),
    ]);
}

#[test]
fn escape_and_invalid_keys_cancel() {
    assert_vim("|abc", "d<esc>l", "a|bc");
    assert_vim("|abc", "2<esc>l", "a|bc");
    assert_vim("|abc", "Ql", "a|bc");
    assert_vim("|abc", "dQl", "a|bc");
}

#[test]
fn multiple_cursors_move_together() {
    assert_vim("|ab\n|cd", "l", "a|b\nc|d");
    assert_vim("|ab cd\n|ef gh", "w", "ab |cd\nef |gh");
}
