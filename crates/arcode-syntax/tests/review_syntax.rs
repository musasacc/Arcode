//! Adversarial review tests. Each test documents one confirmed issue and
//! fails against the current implementation.

mod common;

use std::time::{Duration, Instant};

use arcode_syntax::point_for_byte;
use common::parse;

/// `matching_bracket` collects every sibling of the bracket token into a
/// `Vec` and scans them, so its cost grows with the number of siblings. On a
/// large JSON array (lock files, data dumps) the opening `[` has hundreds of
/// thousands of siblings, which makes a single cursor move cost tens of
/// milliseconds. The partner of a well-formed pair is the parent's first or
/// last child, so this should take microseconds.
#[test]
fn matching_bracket_cost_does_not_grow_with_sibling_count() {
    let elements = 300_000;
    let mut src = String::with_capacity(elements * 12);
    src.push_str("[\n");
    for i in 0..elements {
        src.push_str(&format!("  {i},\n"));
    }
    src.push_str("  0\n]\n");
    let (text, syntax) = parse("json", &src);
    let close = src.rfind(']').expect("closing bracket");

    let mut best = Duration::MAX;
    for _ in 0..3 {
        let started = Instant::now();
        assert_eq!(syntax.matching_bracket(&text, 0), Some(close));
        assert_eq!(syntax.matching_bracket(&text, close), Some(0));
        best = best.min(started.elapsed());
    }
    println!("matching_bracket on a {elements}-element array: {best:?}");
    assert!(
        best < Duration::from_millis(5),
        "matching_bracket took {best:?}; it should not be linear in the sibling count"
    );
}

/// TOML array-of-tables headers (`[[bin]]`) use the two-byte tokens `[[`
/// and `]]`, which `bracket_kind` ignores, so the brackets of a header never
/// match even though `[` and `]` are in the supported set.
#[test]
fn toml_table_array_header_brackets_match() {
    let src = "[[bin]]\nname = \"x\"\n";
    let (text, syntax) = parse("toml", src);
    assert_eq!(syntax.matching_bracket(&text, 0), Some(6));
    assert_eq!(syntax.matching_bracket(&text, 6), Some(0));
    assert_eq!(syntax.matching_bracket(&text, 1), Some(5));
    assert_eq!(syntax.matching_bracket(&text, 5), Some(1));
}

/// `point_for_byte` is documented as a tree-sitter style `(row, byte column)`
/// for building `InputEditData`, but it uses ropey's Unicode line breaks
/// (lone `\r`, U+2028, U+0085, VT, FF) while tree-sitter only counts `\n`.
#[test]
fn point_for_byte_matches_tree_sitter_rows() {
    for src in ["fn a() {}\rfn c() {}\n", "fn a() {}\u{2028}fn c() {}\n"] {
        let (text, syntax) = parse("rust", src);
        let at = src.find("fn c").expect("needle");
        let node = syntax
            .tree()
            .root_node()
            .descendant_for_byte_range(at, at + 2)
            .expect("node");
        let ts = node.start_position();
        assert_eq!(
            point_for_byte(&text, at),
            (ts.row, ts.column),
            "{src:?}: point_for_byte disagrees with tree-sitter"
        );
    }
}
