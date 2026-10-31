//! Rust highlighting: tokens, multi-line nodes, unicode, viewports.

mod common;

use common::{highlight_all, line_text, name_of, parse, pieces};

const SNIPPET: &str = r#"// a comment
/// Docs.
#[derive(Debug)]
pub struct Point { x: f64 }

fn main() {
    let count: u32 = 42;
    let s = "hi\n";
    println!("{}", s);
    point.scale(1.5);
    Some(true);
    if count > 0 { return; }
    std::mem::drop(s);
}
"#;

#[test]
fn rust_snippet_tokens() {
    let (text, syntax) = parse("rust", SNIPPET);
    let lines = highlight_all(&syntax, &text);
    let at = |line, needle| name_of(&text, &lines, line, needle);

    assert_eq!(at(0, "// a comment"), Some("comment"));
    assert_eq!(at(1, "/// Docs."), Some("comment.documentation"));
    assert_eq!(at(2, "derive"), Some("attribute"));
    assert_eq!(at(2, "("), Some("punctuation.bracket"));
    assert_eq!(at(3, "pub"), Some("keyword"));
    assert_eq!(at(3, "struct"), Some("keyword"));
    assert_eq!(at(3, "Point"), Some("type"));
    assert_eq!(at(3, "x"), Some("property"));
    assert_eq!(at(3, "f64"), Some("type.builtin"));
    assert_eq!(at(5, "fn"), Some("keyword"));
    assert_eq!(at(5, "main"), Some("function"));
    assert_eq!(at(5, "{"), Some("punctuation.bracket"));
    assert_eq!(at(6, "let"), Some("keyword"));
    assert_eq!(at(6, "u32"), Some("type.builtin"));
    assert_eq!(at(6, "42"), Some("number"));
    assert_eq!(at(6, ";"), Some("punctuation.delimiter"));
    assert_eq!(at(7, "\"hi"), Some("string"));
    assert_eq!(at(7, "\\n"), Some("string.escape"));
    assert_eq!(at(8, "println!"), Some("function.macro"));
    assert_eq!(at(8, "\"{}\""), Some("string"));
    assert_eq!(at(9, "scale"), Some("function.method"));
    assert_eq!(at(9, "1.5"), Some("number"));
    assert_eq!(at(10, "Some"), Some("constructor"));
    assert_eq!(at(10, "true"), Some("boolean"));
    assert_eq!(at(11, "if"), Some("keyword.control"));
    assert_eq!(at(11, ">"), Some("operator"));
    assert_eq!(at(11, "return"), Some("keyword.control"));
    assert_eq!(at(12, "std"), Some("module"));
    assert_eq!(at(12, "drop"), Some("function"));
}

#[test]
fn string_is_split_around_escape() {
    let (text, syntax) = parse("rust", "let s = \"a\\tb\";\n");
    let lines = highlight_all(&syntax, &text);
    let got = pieces(&text, 0, &lines[0]);
    let string_parts: Vec<_> = got
        .iter()
        .filter(|(_, name)| name.starts_with("string"))
        .cloned()
        .collect();
    assert_eq!(
        string_parts,
        vec![
            ("\"a".to_string(), "string"),
            ("\\t".to_string(), "string.escape"),
            ("b\"".to_string(), "string"),
        ]
    );
}

#[test]
fn block_comment_split_across_lines() {
    let src = "let a = 1; /* start\nmiddle ( line\nend */ let b = 2;\n";
    let (text, syntax) = parse("rust", src);
    let lines = highlight_all(&syntax, &text);

    let line0 = pieces(&text, 0, &lines[0]);
    assert_eq!(line0.last(), Some(&("/* start".to_string(), "comment")));
    assert_eq!(
        pieces(&text, 1, &lines[1]),
        vec![("middle ( line".to_string(), "comment")]
    );
    let line2 = pieces(&text, 2, &lines[2]);
    assert_eq!(line2[0], ("end */".to_string(), "comment"));
    assert_eq!(line2[1], ("let".to_string(), "keyword"));
    assert_eq!(name_of(&text, &lines, 2, "2"), Some("number"));
}

#[test]
fn raw_string_split_across_lines() {
    let src = "let r = r#\"one\ntwo \"quoted\" // not a comment\nthree\"#;\nlet z = 0;\n";
    let (text, syntax) = parse("rust", src);
    let lines = highlight_all(&syntax, &text);

    assert_eq!(name_of(&text, &lines, 0, "r#\"one"), Some("string"));
    assert_eq!(
        pieces(&text, 1, &lines[1]),
        vec![(line_text(&text, 1), "string")]
    );
    let line2 = pieces(&text, 2, &lines[2]);
    assert_eq!(line2[0], ("three\"#".to_string(), "string"));
    assert_eq!(name_of(&text, &lines, 3, "let"), Some("keyword"));
}

#[test]
fn viewport_inside_multiline_node() {
    let src = "/*\n a\n b\n c\n*/\nfn f() {}\n";
    let (text, syntax) = parse("rust", src);
    let lines = syntax.highlight_lines(&text, 2..3);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        pieces(&text, 2, &lines[0]),
        vec![(" b".to_string(), "comment")]
    );

    // A viewport that starts mid-comment and ends after it.
    let lines = syntax.highlight_lines(&text, 3..6);
    assert_eq!(lines.len(), 3);
    assert_eq!(
        pieces(&text, 4, &lines[1]),
        vec![("*/".to_string(), "comment")]
    );
    assert_eq!(
        pieces(&text, 5, &lines[2])[0],
        ("fn".to_string(), "keyword")
    );
}

#[test]
fn viewport_matches_full_highlight() {
    let (text, syntax) = parse("rust", SNIPPET);
    let full = highlight_all(&syntax, &text);
    for start in 0..text.len_lines() {
        for end in start..=text.len_lines() {
            let part = syntax.highlight_lines(&text, start..end);
            assert_eq!(part, full[start..end], "lines {start}..{end}");
        }
    }
}

#[test]
fn unicode_byte_offsets() {
    let src = "let s = \"héllo wörld\"; // ünïcode ✓\nfn größe() -> u8 { 1 }\n";
    let (text, syntax) = parse("rust", src);
    let lines = highlight_all(&syntax, &text);

    let line0 = pieces(&text, 0, &lines[0]);
    assert!(line0.contains(&("\"héllo wörld\"".to_string(), "string")));
    assert!(line0.contains(&("// ünïcode ✓".to_string(), "comment")));
    let line1 = pieces(&text, 1, &lines[1]);
    assert!(line1.contains(&("größe".to_string(), "function")));
    assert!(line1.contains(&("u8".to_string(), "type.builtin")));
}

#[test]
fn crlf_lines_exclude_carriage_return() {
    let src = "// one\r\nfn a() {}\r\n";
    let (text, syntax) = parse("rust", src);
    let lines = highlight_all(&syntax, &text);
    assert_eq!(
        pieces(&text, 0, &lines[0]),
        vec![("// one".to_string(), "comment")]
    );
    assert_eq!(lines[0][0].range, 0..6);
    assert_eq!(name_of(&text, &lines, 1, "fn"), Some("keyword"));
}

#[test]
fn broken_code_still_highlights() {
    let (text, syntax) = parse("rust", "fn (\nlet x = \"unterminated\n");
    let lines = highlight_all(&syntax, &text);
    assert_eq!(name_of(&text, &lines, 0, "fn"), Some("keyword"));
}

#[test]
fn doc_comment_markers_are_not_operators() {
    let src = "//! inner\n/** block\n doc */\nfn x(p: *const u8) { let a = !b * *c / 2; }\n";
    let (text, syntax) = parse("rust", src);
    let lines = highlight_all(&syntax, &text);
    for (line, spans) in lines.iter().enumerate().take(3) {
        assert_eq!(
            pieces(&text, line, spans),
            vec![(line_text(&text, line), "comment.documentation")]
        );
    }
    let ops: Vec<_> = pieces(&text, 3, &lines[3])
        .into_iter()
        .filter(|(_, name)| *name == "operator")
        .map(|(s, _)| s)
        .collect();
    assert_eq!(ops, ["*", "=", "!", "*", "*", "/"]);
}
