//! Tree-based bracket matching.

mod common;

use common::{apply_edit, parse};

fn offset(src: &str, needle: &str) -> usize {
    src.find(needle).expect("needle")
}

#[test]
fn rust_brackets_match_both_ways() {
    let src = "fn f(a: [u8; 3]) -> u8 { g((1, 2)); a[0] }\n";
    let (text, syntax) = parse("rust", src);
    let pairs = [
        (offset(src, "(a"), offset(src, ") ->")),
        (offset(src, "[u8"), offset(src, "]) ->")),
        (offset(src, "{ g"), offset(src, "}\n")),
        (offset(src, "((1"), offset(src, ");")),
        (offset(src, "(1,"), offset(src, "));")),
        (offset(src, "[0]"), offset(src, "] }")),
    ];
    for (open, close) in pairs {
        assert_eq!(
            syntax.matching_bracket(&text, open),
            Some(close),
            "open {open}"
        );
        assert_eq!(
            syntax.matching_bracket(&text, close),
            Some(open),
            "close {close}"
        );
    }
}

#[test]
fn brackets_in_strings_and_comments_are_ignored() {
    let src = "fn f() { let s = \"(\"; /* [ */ }\n";
    let (text, syntax) = parse("rust", src);
    assert_eq!(syntax.matching_bracket(&text, offset(src, "(\"")), None);
    assert_eq!(syntax.matching_bracket(&text, offset(src, "[ */")), None);
    assert_eq!(
        syntax.matching_bracket(&text, offset(src, "{ let")),
        Some(offset(src, "}\n"))
    );
}

#[test]
fn non_brackets_and_out_of_range() {
    let src = "fn f() {}\n";
    let (text, syntax) = parse("rust", src);
    assert_eq!(syntax.matching_bracket(&text, 0), None);
    assert_eq!(syntax.matching_bracket(&text, src.len()), None);
    assert_eq!(syntax.matching_bracket(&text, 10_000), None);
}

#[test]
fn unmatched_bracket_has_no_partner() {
    let src = "fn f() {\n";
    let (text, syntax) = parse("rust", src);
    assert_eq!(syntax.matching_bracket(&text, offset(src, "{")), None);
}

#[test]
fn macro_token_trees() {
    let src = "fn f() { vec![(1), [2]]; }\n";
    let (text, syntax) = parse("rust", src);
    assert_eq!(
        syntax.matching_bracket(&text, offset(src, "![")),
        None,
        "'!' is not a bracket"
    );
    assert_eq!(
        syntax.matching_bracket(&text, offset(src, "[(1")),
        Some(offset(src, "];"))
    );
    assert_eq!(
        syntax.matching_bracket(&text, offset(src, "(1)")),
        Some(offset(src, "), ["))
    );
}

#[test]
fn json_and_toml_brackets() {
    let src = "{\"a\": [1, {\"b\": 2}]}";
    let (text, syntax) = parse("json", src);
    assert_eq!(syntax.matching_bracket(&text, 0), Some(src.len() - 1));
    assert_eq!(
        syntax.matching_bracket(&text, offset(src, "[")),
        Some(offset(src, "]"))
    );
    assert_eq!(
        syntax.matching_bracket(&text, offset(src, "{\"b")),
        Some(offset(src, "}]"))
    );

    let src = "x = [1, [2]]\ny = { a = 1 }\n";
    let (text, syntax) = parse("toml", src);
    assert_eq!(
        syntax.matching_bracket(&text, offset(src, "[1")),
        Some(offset(src, "]\n"))
    );
    assert_eq!(
        syntax.matching_bracket(&text, offset(src, "{")),
        Some(offset(src, "}"))
    );
}

#[test]
fn matching_after_incremental_edit() {
    let src = "fn f() { g(); }\n";
    let (mut text, mut syntax) = parse("rust", src);
    let at = offset(src, "g();");
    let edit = apply_edit(&mut text, at..at, "if x { y(); } ");
    syntax.edit(&edit);
    syntax.reparse(&text);
    let new = text.to_string();
    assert_eq!(
        syntax.matching_bracket(&text, offset(&new, "{ y")),
        Some(offset(&new, "} g"))
    );
    assert_eq!(
        syntax.matching_bracket(&text, offset(&new, "{ if")),
        Some(new.rfind('}').expect("brace"))
    );
}
