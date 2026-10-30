//! Text objects with operators.

mod common;

use common::check_table;

#[test]
fn words() {
    check_table(&[
        ("foo b|ar baz", "diw", "foo | baz"),
        ("foo b|ar baz", "daw", "foo |baz"),
        ("foo b|ar", "daw", "fo|o"),
        ("foo |  bar", "diw", "foo|bar"),
        ("foo |  bar", "daw", "fo|o"),
        ("a x.|y b", "diW", "a | b"),
        ("a x.|y b", "daW", "a |b"),
        ("a x.|y b", "diw", "a x.| b"),
        ("foo b|ar", "ciwX<esc>", "foo |X"),
        ("|a b c", "d3iw", "| c"),
        ("|a b c", "d2aw", "|c"),
        ("x |foo y", "yiwP", "x fo|ofoo y"),
    ]);
}

#[test]
fn quotes() {
    check_table(&[
        ("x \"a|b\" y", "di\"", "x \"|\" y"),
        ("x \"a|b\" y", "da\"", "x |y"),
        ("x 'a|b' y", "di'", "x '|' y"),
        ("x `a|b` y", "da`", "x |y"),
        ("|x \"ab\"", "di\"", "x \"|\""),
        ("x \"a|b\"", "ci\"Z<esc>", "x \"|Z\""),
        ("|no quotes", "di\"", "|no quotes"),
        ("x \"a\\\"|b\" y", "di\"", "x \"|\" y"),
    ]);
}

#[test]
fn brackets() {
    check_table(&[
        ("f(a, |b)", "ci(X<esc>", "f(|X)"),
        ("f(a, |b)", "di)", "f(|)"),
        ("f(a|) x", "da(", "f| x"),
        ("f(a|) x", "dib", "f(|) x"),
        ("f(a|) x", "dab", "f| x"),
        ("f(a, (|b))", "d2i(", "f(|)"),
        ("f(a, (|b))", "2di(", "f(|)"),
        ("a[|1]", "di[", "a[|]"),
        ("a[|1] x", "da]", "a| x"),
        ("{ |a }", "di{", "{|}"),
        ("{ |a }", "diB", "{|}"),
        ("x{ |a }", "daB", "|x"),
        ("<|a>", "di<", "<|>"),
        ("<|a>", "da>", "|"),
        ("fn {\n    |x;\n}", "di{", "fn {\n|}"),
        ("fn {\n    |x;\n}", "ci{y<esc>", "fn {\n    |y\n}"),
        ("fn {\n    |x;\n}", "da{", "fn| "),
        ("|(a)", "yi(P", "(|aa)"),
        ("|nothing", "di(", "|nothing"),
    ]);
}

#[test]
fn tags() {
    check_table(&[
        ("<a>|x</a>", "dit", "<a>|</a>"),
        ("y <a>|x</a> z", "dat", "y | z"),
        ("<div><p>h|i</p></div>", "d2it", "<div>|</div>"),
        ("<div><p>h|i</p></div>", "cits<esc>", "<div><p>|s</p></div>"),
        ("|<a x=\"1\">t</a>", "dit", "<a x=\"1\">|</a>"),
    ]);
}

#[test]
fn paragraphs() {
    check_table(&[
        ("a\n|b\n\nc", "dip", "|\nc"),
        ("a\n|b\n\nc", "dap", "|c"),
        ("a\n\n|c", "dap", "|a"),
        ("|a\n\nb\n\nc", "d2ap", "|c"),
        ("a\n|b\n\nc", "yipP", "|a\nb\na\nb\n\nc"),
        ("a\n|b\n\nc", "cipX<esc>", "|X\n\nc"),
    ]);
}
