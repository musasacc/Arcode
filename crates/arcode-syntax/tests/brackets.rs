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

#[test]
fn toml_table_array_headers_match_byte_by_byte() {
    let src = "[package]\nname = \"a\"\n\n[[bin]]\nname = \"b\"\n\n[[bin]]\nname = \"c\"\n";
    let (text, syntax) = parse("toml", src);
    let header = offset(src, "[package]");
    let header_end = offset(src, "]\nname");
    assert_eq!(syntax.matching_bracket(&text, header), Some(header_end));
    assert_eq!(syntax.matching_bracket(&text, header_end), Some(header));
    let second = src.rfind("[[bin]]").expect("header");
    for at in [offset(src, "[[bin]]"), second] {
        assert_eq!(syntax.matching_bracket(&text, at), Some(at + 6));
        assert_eq!(syntax.matching_bracket(&text, at + 1), Some(at + 5));
        assert_eq!(syntax.matching_bracket(&text, at + 5), Some(at + 1));
        assert_eq!(syntax.matching_bracket(&text, at + 6), Some(at));
        assert_eq!(syntax.matching_bracket(&text, at + 2), None, "'b'");
    }
}

#[test]
fn nested_toml_arrays_use_single_brackets() {
    let src = "a = [[1, 2], [3]]\n";
    let (text, syntax) = parse("toml", src);
    let outer = offset(src, "[[");
    let outer_end = offset(src, "]\n");
    assert_eq!(syntax.matching_bracket(&text, outer), Some(outer_end));
    assert_eq!(syntax.matching_bracket(&text, outer_end), Some(outer));
    assert_eq!(
        syntax.matching_bracket(&text, outer + 1),
        Some(offset(src, "], ["))
    );
    assert_eq!(
        syntax.matching_bracket(&text, offset(src, "[3")),
        Some(offset(src, "]]"))
    );
}

/// Pairs that do not delimit their parent use the sibling scan.
#[test]
fn pairs_inside_larger_nodes_match() {
    // `a[i]`: the `[` is not the first child of the index expression.
    // `pub(crate)`: the parens sit inside a visibility modifier.
    let src = "pub(crate) fn f() { a[i]; }\n";
    let (text, syntax) = parse("rust", src);
    for (open, close) in [
        (offset(src, "(crate"), offset(src, ") fn")),
        (offset(src, "[i"), offset(src, "];")),
    ] {
        assert_eq!(syntax.matching_bracket(&text, open), Some(close));
        assert_eq!(syntax.matching_bracket(&text, close), Some(open));
    }
}

/// Error recovery can leave several brackets as siblings; the scan must count
/// nesting rather than trust the parent's first and last children.
#[test]
fn brackets_in_error_trees_match_by_nesting() {
    let src = "[1, [2, 3], ";
    let (text, syntax) = parse("json", src);
    assert!(syntax.tree().root_node().has_error());
    let inner = offset(src, "[2");
    let inner_end = offset(src, "],");
    assert_eq!(syntax.matching_bracket(&text, inner), Some(inner_end));
    assert_eq!(syntax.matching_bracket(&text, inner_end), Some(inner));
    assert_eq!(syntax.matching_bracket(&text, 0), None);

    let src = "{\"a\": [1, 2]] }";
    let (text, syntax) = parse("json", src);
    assert!(syntax.tree().root_node().has_error());
    let open = offset(src, "[");
    for at in 0..src.len() {
        if let Some(partner) = syntax.matching_bracket(&text, at) {
            assert_eq!(syntax.matching_bracket(&text, partner), Some(at), "{at}");
        }
    }
    assert_eq!(
        syntax.matching_bracket(&text, open),
        Some(offset(src, "]]"))
    );
}

#[test]
fn large_object_matches_without_scanning() {
    let mut src = String::from("{\n");
    for i in 0..50_000 {
        src.push_str(&format!("  \"k{i}\": [{i}, {{\"x\": ({i})}}],\n"));
    }
    src.push_str("  \"end\": 0\n}\n");
    let (text, syntax) = parse("json", &src);
    let close = src.rfind('}').expect("brace");
    assert_eq!(syntax.matching_bracket(&text, 0), Some(close));
    assert_eq!(syntax.matching_bracket(&text, close), Some(0));
    let last_array = src.rfind('[').expect("bracket");
    let last_array_end = src.rfind(']').expect("bracket");
    assert_eq!(
        syntax.matching_bracket(&text, last_array),
        Some(last_array_end)
    );
}
