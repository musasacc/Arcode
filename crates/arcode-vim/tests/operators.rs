//! Operators with motions, doubled forms and shortcuts.

mod common;

use common::check_table;

#[test]
fn delete() {
    check_table(&[
        ("|foo bar", "dw", "|bar"),
        ("|a b c", "d2w", "|c"),
        ("|a b c", "2dw", "|c"),
        ("|a b c d e", "2d2w", "|e"),
        ("foo |bar\nbaz", "dw", "foo| \nbaz"),
        ("|foo   \n  bar", "dw", "|\n  bar"),
        ("a\n|\nb", "dw", "a\n|b"),
        ("|foo bar", "de", "| bar"),
        ("foo |bar", "db", "|bar"),
        ("a|bc", "d$", "|a"),
        ("a|bc", "D", "|a"),
        ("a|bc\nde", "2D", "|a"),
        ("ab|c", "d0", "|c"),
        ("  ab|c", "d^", "  |c"),
        ("a\n|b\nc", "dd", "a\n|c"),
        ("a\n|b", "dd", "|a"),
        ("|a\nb\nc", "2dd", "|c"),
        ("|a\nb", "5dd", "|"),
        ("|a\nb\nc", "dj", "|c"),
        ("a\n|b\nc", "dk", "|c"),
        ("a\n|b\nc", "dG", "|a"),
        ("a\n|b\nc", "dgg", "|c"),
        ("|ab,c", "dt,", "|,c"),
        ("|ab,c", "df,", "|c"),
        ("a,b|c", "dF,", "a|c"),
        ("a,b|c", "dT,", "a,|c"),
        ("|a,b,c", "df,.", "|c"),
        ("|(a) b", "d%", "| b"),
        ("|a\nb\n\nc", "d}", "|\nc"),
        ("a\n\nb\n|c", "d{", "a\n|c"),
        ("|foo bar baz", "d/baz<cr>", "|baz"),
        ("foo bar |baz", "d?foo<cr>", "|baz"),
        ("|a b", "d/zz<cr>", "|a b"),
        ("|a b", "d/b<esc>", "|a b"),
        ("|a\nb\nc", "d2j", "|"),
        ("|a\nb\nc", "dL", "|"),
        ("ab|c", "dh", "a|c"),
        ("|abc", "dl", "|bc"),
    ]);
}

#[test]
fn delete_shortcuts() {
    check_table(&[
        ("|abc", "x", "|bc"),
        ("|abcd", "3x", "|d"),
        ("ab|c", "x", "a|b"),
        ("a|bc", "9x", "|a"),
        ("|", "x", "|"),
        ("a\n|\nb", "x", "a\n|\nb"),
        ("ab|c", "X", "a|c"),
        ("abc|d", "2X", "a|d"),
        ("|abc", "X", "|abc"),
        ("|abc", "<del>", "|bc"),
    ]);
}

#[test]
fn change() {
    check_table(&[
        ("|foo bar", "cwxy<esc>", "x|y bar"),
        ("fo|o bar", "cwX<esc>", "fo|X bar"),
        ("|a b c", "c2wX<esc>", "|X c"),
        ("|a b c", "2cwX<esc>", "|X c"),
        ("|foo bar", "cWX<esc>", "|X bar"),
        ("foo|  bar", "cwX<esc>", "foo|Xbar"),
        ("|foo bar", "ceX<esc>", "|X bar"),
        ("foo |bar", "cbX<esc>", "|Xbar"),
        ("  |foo\nbar", "ccX<esc>", "  |X\nbar"),
        ("  |foo\nbar", "SX<esc>", "  |X\nbar"),
        ("|a\nb\nc", "2ccX<esc>", "|X\nc"),
        ("a|bc", "CX<esc>", "a|X"),
        ("a|bc", "c$X<esc>", "a|X"),
        ("|abc", "sX<esc>", "|Xbc"),
        ("|abc", "2sX<esc>", "|Xc"),
        ("|a\nb", "cjX<esc>", "|X"),
        ("|ab,c", "ct,X<esc>", "|X,c"),
    ]);
}

#[test]
fn yank_and_put() {
    check_table(&[
        ("|foo bar", "ywP", "foo| foo bar"),
        ("|foo bar", "yiwP", "fo|ofoo bar"),
        ("|a\nb", "yyp", "a\n|a\nb"),
        ("a\n|b", "yyP", "a\n|b\nb"),
        ("|a\nb", "Yp", "a\n|a\nb"),
        ("|a\nb", "yjGp", "a\nb\n|a\nb"),
        ("a\n|b", "ykp", "a\n|a\nb\nb"),
        ("|ab", "xp", "b|a"),
        ("|a\nb", "ddp", "b\n|a"),
        ("|x", "yl3p", "xxx|x"),
        ("|x\ny", "yy2p", "x\n|x\nx\ny"),
        ("|ab", "ylgp", "aa|b"),
        ("|a\nb", "yygp", "a\na\n|b"),
        ("|a\nb", "yygP", "a\n|a\nb"),
        ("|  a\nb", "yyjp", "  a\nb\n  |a"),
        ("|ab\ncd", "vjyGp", "ab\nc|ab\ncd"),
        ("|", "p", "|"),
        ("|abc", "yiwA <esc>p", "abc ab|c"),
        ("a|b", "y$P", "a|bb"),
    ]);
}

#[test]
fn shift_case_join_replace() {
    check_table(&[
        ("|a", ">>", "    |a"),
        ("      |a", "<<", "  |a"),
        ("|\ta", "<<", "|a"),
        ("|a\nb\nc", "2>>", "    |a\n    b\nc"),
        ("|a\nb", ">j", "    |a\n    b"),
        ("|a\n\nb", ">2j", "    |a\n\n    b"),
        ("    |a\nb", "<ip", "|a\nb"),
        ("|abC d", "g~w", "|ABc d"),
        ("a|Bc", "gUU", "A|BC"),
        ("a|Bc", "gUgU", "A|BC"),
        ("A|Bc", "guu", "a|bc"),
        ("a|Bc", "g~~", "A|bC"),
        ("|ab cd", "gUiw", "|AB cd"),
        ("|ab cd", "gUe", "|AB cd"),
        ("|abc", "~", "A|bc"),
        ("|abc", "2~", "AB|c"),
        ("|abc", "5~", "AB|C"),
        ("|abc", "rx", "|xbc"),
        ("|abcd", "3rx", "xx|xd"),
        ("|ab", "3rx", "|ab"),
        ("a|bc", "r<cr>", "a\n|c"),
        ("|a\n  b", "J", "a| b"),
        ("|a\nb\nc", "3J", "a b| c"),
        ("|a\nb\nc", "J", "a| b\nc"),
        ("|f(\n)", "J", "f(|)"),
        ("|a \nb", "J", "a |b"),
        ("|\nb", "J", "|b"),
        ("|a\n  b", "gJ", "a|  b"),
        ("|a", "J", "|a"),
    ]);
}

#[test]
fn reindent_and_comment() {
    check_table(&[
        ("|{\nx\n}", "=G", "|{\n    x\n}"),
        (
            "|fn f() {\n        a;\n  b;\n}",
            "=G",
            "|fn f() {\n    a;\n    b;\n}",
        ),
        ("|a\n   b", "=j", "|a\nb"),
        ("|a", "gcc", "|// a"),
        ("|// a", "gcc", "|a"),
        ("  |a\n    b", "gcj", "  |// a\n  //   b"),
        ("|a\n\nb", "gcap", "|// a\n\nb"),
        ("|a\n\nb", "VGgc", "|// a\n\n// b"),
        ("|a", "gcc.", "|a"),
    ]);
    let mut s = common::Session::new("|a");
    s.keys("gcc");
    assert_eq!(s.state(), "|// a");
}

#[test]
fn increment() {
    check_table(&[
        ("x |9 y", "<c-a>", "x 1|0 y"),
        ("|x -1", "<c-a>", "x |0"),
        ("|10", "5<c-x>", "|5"),
        ("|a7b", "<c-a><c-a>", "a|9b"),
        ("|abc", "<c-a>", "|abc"),
        ("|x 0", "<c-x>", "x -|1"),
    ]);
}

#[test]
fn multiple_cursors_operate_together() {
    check_table(&[
        ("|a b\n|c d", "dw", "|b\n|d"),
        ("|ab\n|cd", "x", "|b\n|d"),
        ("|a\n|b", "A!<esc>", "a|!\nb|!"),
    ]);
}
