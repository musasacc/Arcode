//! Incremental edit + reparse must match a fresh parse.

mod common;

use arcode_syntax::{Syntax, language_by_id};
use common::{apply_edit, highlight_all, name_of, parse};
use ropey::Rope;

const SOURCE: &str = r#"use std::collections::HashMap;

/// A counter.
pub struct Counter {
    counts: HashMap<String, usize>,
}

impl Counter {
    pub fn add(&mut self, word: &str) {
        *self.counts.entry(word.to_string()).or_insert(0) += 1;
    }

    fn total(&self) -> usize {
        let s = "text with ( brackets";
        self.counts.values().sum()
    }
}
"#;

/// Asserts the incrementally updated syntax equals a fresh parse of `text`.
fn assert_matches_fresh(syntax: &Syntax, text: &Rope) {
    let fresh = Syntax::new(syntax.language(), text).expect("parse");
    assert_eq!(
        syntax.tree().root_node().to_sexp(),
        fresh.tree().root_node().to_sexp(),
        "tree differs for:\n{text}"
    );
    assert_eq!(
        highlight_all(syntax, text),
        highlight_all(&fresh, text),
        "highlights differ for:\n{text}"
    );
}

fn byte_of(text: &Rope, needle: &str) -> usize {
    text.to_string().find(needle).expect("needle present")
}

#[test]
fn rename_function_keeps_highlights() {
    let (mut text, mut syntax) = parse("rust", SOURCE);
    let at = byte_of(&text, "total");
    let edit = apply_edit(&mut text, at..at + "total".len(), "grand_total");
    syntax.edit(&edit);
    syntax.reparse(&text);
    assert_matches_fresh(&syntax, &text);

    let lines = highlight_all(&syntax, &text);
    let line = text.byte_to_line(at);
    assert_eq!(
        name_of(&text, &lines, line, "grand_total"),
        Some("function")
    );
}

#[test]
fn opening_block_comment_changes_following_lines() {
    let (mut text, mut syntax) = parse("rust", SOURCE);
    let at = byte_of(&text, "impl Counter");
    let edit = apply_edit(&mut text, at..at, "/* ");
    syntax.edit(&edit);
    let changed = syntax.reparse_with_changed_ranges(&text);
    assert!(!changed.is_empty(), "structure changed");
    assert!(changed.iter().any(|range| range.end > at), "{changed:?}");
    assert_matches_fresh(&syntax, &text);

    let lines = highlight_all(&syntax, &text);
    let line = text.byte_to_line(byte_of(&text, "fn total"));
    assert_eq!(name_of(&text, &lines, line, "fn total"), Some("comment"));

    // Close it again.
    let at = byte_of(&text, "/* ");
    let edit = apply_edit(&mut text, at..at + 3, "");
    syntax.edit(&edit);
    syntax.reparse(&text);
    assert_matches_fresh(&syntax, &text);
    assert_eq!(text.to_string(), SOURCE);
}

#[test]
fn newline_and_unicode_edits() {
    let (mut text, mut syntax) = parse("rust", SOURCE);
    let at = byte_of(&text, "let s");
    let edit = apply_edit(&mut text, at..at, "let ü = \"日本\";\n        ");
    syntax.edit(&edit);
    syntax.reparse(&text);
    assert_matches_fresh(&syntax, &text);

    // Delete across a line break.
    let start = byte_of(&text, "fn total");
    let end = byte_of(&text, "let s");
    let edit = apply_edit(&mut text, start..end, "");
    syntax.edit(&edit);
    syntax.reparse(&text);
    assert_matches_fresh(&syntax, &text);
}

#[test]
fn several_edits_then_one_reparse() {
    let (mut text, mut syntax) = parse("rust", SOURCE);
    for (needle, replacement) in [("Counter {", "Tally {"), ("usize>", "u64>"), ("0)", "1)")] {
        let at = byte_of(&text, needle);
        let edit = apply_edit(&mut text, at..at + needle.len(), replacement);
        syntax.edit(&edit);
    }
    syntax.reparse(&text);
    assert_matches_fresh(&syntax, &text);
}

#[test]
fn pseudo_random_edits_match_fresh_parse() {
    let fragments = [
        "",
        "x",
        "(",
        ")",
        "{",
        "}",
        "\"",
        "/*",
        "*/",
        "//",
        "\n",
        "fn f() {}\n",
        "é",
        "r#\"",
        "\"#",
        "let a = 1;",
        "'a",
        "::",
    ];
    let (mut text, mut syntax) = parse("rust", SOURCE);
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % bound.max(1) as u64) as usize
    };
    for _ in 0..200 {
        let chars = text.len_chars();
        let start_char = next(chars + 1);
        let end_char = (start_char + next(6)).min(chars);
        let start = text.char_to_byte(start_char);
        let end = text.char_to_byte(end_char);
        let insert = fragments[next(fragments.len())];
        let edit = apply_edit(&mut text, start..end, insert);
        syntax.edit(&edit);
        syntax.reparse(&text);
        assert_matches_fresh(&syntax, &text);
    }
}

#[test]
fn json_and_toml_incremental() {
    for (id, src, needle, insert) in [
        (
            "json",
            "{\"a\": [1, 2], \"b\": null}\n",
            "null",
            "{\"c\": true}",
        ),
        ("toml", "[a]\nx = 1\n[b]\ny = \"s\"\n", "1", "[1, 2]"),
    ] {
        let lang = language_by_id(id).expect("lang");
        let mut text = Rope::from_str(src);
        let mut syntax = Syntax::new(lang, &text).expect("parse");
        let at = byte_of(&text, needle);
        let edit = apply_edit(&mut text, at..at + needle.len(), insert);
        syntax.edit(&edit);
        syntax.reparse(&text);
        assert_matches_fresh(&syntax, &text);
    }
}

#[test]
fn stale_tree_never_panics() {
    let (mut text, mut syntax) = parse("rust", SOURCE);
    // Edit with multi-byte chars but do not reparse: the tree is stale.
    let at = byte_of(&text, "pub struct");
    let edit = apply_edit(&mut text, at..at + 10, "ü日本語 (");
    syntax.edit(&edit);
    let lines = syntax.highlight_lines(&text, 0..text.len_lines());
    assert_eq!(lines.len(), text.len_lines());
    for byte in 0..text.len_bytes() + 2 {
        let _ = syntax.matching_bracket(&text, byte);
    }
    // Text shrank a lot without telling the tree.
    let short = Rope::from_str("é");
    let _ = syntax.highlight_lines(&short, 0..5);
    let _ = syntax.matching_bracket(&short, 0);
    syntax.reparse(&text);
    assert_matches_fresh(&syntax, &text);
}

/// Edits on text with breaks tree-sitter does not count as rows (lone CR,
/// U+2028) keep every node position equal to a fresh parse, even before
/// the reparse.
#[test]
fn edit_points_follow_tree_sitter_rows() {
    let src = "fn a() {}\rfn b() {}\u{2028}fn c() {\n    1\r}\n";
    let (mut text, mut syntax) = parse("rust", src);
    let at = byte_of(&text, "fn b");
    let edit = apply_edit(&mut text, at..at, "fn x() {}\n");
    syntax.edit(&edit);
    syntax.reparse(&text);
    let fresh = Syntax::new(syntax.language(), &text).expect("parse");
    let mut ours = syntax.tree().walk();
    let mut theirs = fresh.tree().walk();
    loop {
        let (a, b) = (ours.node(), theirs.node());
        assert_eq!(a.kind(), b.kind());
        assert_eq!(a.byte_range(), b.byte_range());
        assert_eq!(a.start_position(), b.start_position(), "{}", a.kind());
        assert_eq!(a.end_position(), b.end_position(), "{}", a.kind());
        let (stepped, _) = (ours.goto_first_child(), theirs.goto_first_child());
        if stepped {
            continue;
        }
        let mut done = false;
        while !ours.goto_next_sibling() {
            theirs.goto_parent();
            if !ours.goto_parent() {
                done = true;
                break;
            }
        }
        if done {
            break;
        }
        theirs.goto_next_sibling();
    }
    assert_matches_fresh(&syntax, &text);
}
