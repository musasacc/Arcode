mod common;

use arcode_search::{
    CancelToken, SearchError, SearchQuery, SkipReason, replace_all, replace_in_file,
    replace_in_text,
};
use common::{slash, tree};

fn regex(pattern: &str) -> SearchQuery {
    SearchQuery {
        is_regex: true,
        case_sensitive: true,
        ..SearchQuery::new(pattern)
    }
}

#[test]
fn regex_groups_and_named_groups() {
    let (out, n) = replace_in_text(
        "let a = f(1, 2);\nlet b = f(3, 4);\n",
        &regex(r"f\((\d), (?P<second>\d)\)"),
        "g(${second}, $1)$$",
        None,
    )
    .unwrap();
    assert_eq!(n, 2);
    assert_eq!(out, "let a = g(2, 1)$;\nlet b = g(4, 3)$;\n");
}

#[test]
fn literal_mode_inserts_replacement_verbatim() {
    let (out, n) = replace_in_text("a.b a.b axb", &SearchQuery::new("a.b"), "$1", None).unwrap();
    assert_eq!(n, 2);
    assert_eq!(out, "$1 $1 axb");
}

#[test]
fn crlf_mixed_endings_and_bom_preserved() {
    let text = "\u{feff}foo\r\nbar foo\nfoo\r\nlast foo";
    let (out, n) = replace_in_text(text, &SearchQuery::new("foo"), "qux", None).unwrap();
    assert_eq!(n, 4);
    assert_eq!(out, "\u{feff}qux\r\nbar qux\nqux\r\nlast qux");
    let (out, _) = replace_in_text("x\r\ny\r\n", &regex("$"), "!", None).unwrap();
    assert_eq!(out, "x\r\ny\r\n", "empty matches are never replaced");
    let (out, n) = replace_in_text("ab\r\nab\r\n", &regex("b$"), "B", None).unwrap();
    assert_eq!((out.as_str(), n), ("aB\r\naB\r\n", 2));
}

#[test]
fn only_selected_lines_and_case_insensitive() {
    let text = "Foo\nfoo\nFOO\nfoo";
    let (out, n) = replace_in_text(text, &SearchQuery::new("foo"), "x", Some(&[3, 0])).unwrap();
    assert_eq!(n, 2);
    assert_eq!(out, "x\nfoo\nFOO\nx");
}

#[test]
fn whole_word_replace() {
    let mut q = SearchQuery::new("id");
    q.whole_word = true;
    let (out, n) = replace_in_text("id idx _id id.x", &q, "key", None).unwrap();
    assert_eq!((out.as_str(), n), ("key idx _id key.x", 2));
}

#[test]
fn file_replace_writes_and_rejects_bad_files() {
    let dir = tree(&[
        ("a.txt", b"one two\r\none\r\n"),
        ("latin1.txt", b"caf\xe9 one\n"),
        ("bin.dat", b"one\0"),
    ]);
    let path = dir.path().join("a.txt");
    let n = replace_in_file(&path, &regex(r"(o)ne"), "${1}NE", None).unwrap();
    assert_eq!(n, 2);
    assert_eq!(std::fs::read(&path).unwrap(), b"oNE two\r\noNE\r\n");
    assert_eq!(replace_in_file(&path, &regex("zzz"), "y", None).unwrap(), 0);

    let latin = dir.path().join("latin1.txt");
    let err = replace_in_file(&latin, &SearchQuery::new("one"), "x", None).unwrap_err();
    assert!(matches!(err, SearchError::NotUtf8(_)));
    assert_eq!(std::fs::read(&latin).unwrap(), b"caf\xe9 one\n");
    let err = replace_in_file(
        &dir.path().join("bin.dat"),
        &SearchQuery::new("one"),
        "x",
        None,
    )
    .unwrap_err();
    assert!(matches!(err, SearchError::Binary(_)));

    // UTF-16 with a BOM is text in a foreign encoding, not binary.
    let mut wide = vec![0xFE, 0xFF];
    for unit in "one\n".encode_utf16() {
        wide.extend(unit.to_be_bytes());
    }
    let wide_path = dir.path().join("wide.txt");
    std::fs::write(&wide_path, &wide).unwrap();
    let err = replace_in_file(&wide_path, &SearchQuery::new("one"), "x", None).unwrap_err();
    assert!(matches!(err, SearchError::NotUtf8(_)));
    assert_eq!(std::fs::read(&wide_path).unwrap(), wide);
}

#[test]
fn replace_all_in_tree() {
    let dir = tree(&[
        (".gitignore", b"ignored/\n"),
        ("src/a.rs", b"old old\n"),
        ("src/b.rs", b"x\r\nold\r\n"),
        ("ignored/c.rs", b"old\n"),
        ("latin1.txt", b"\xe9 old\n"),
        ("none.txt", b"nothing\n"),
    ]);
    let mut q = SearchQuery::new("old");
    q.max_results = 1; // ignored by replace_all
    let summary = replace_all(dir.path(), &q, "new", &CancelToken::new()).unwrap();
    assert_eq!(summary.replacements, 3);
    let changed: Vec<_> = summary.changed.iter().map(|c| slash(&c.relative)).collect();
    assert_eq!(changed, ["src/a.rs", "src/b.rs"]);
    assert_eq!(summary.skipped.len(), 1);
    assert_eq!(slash(&summary.skipped[0].relative), "latin1.txt");
    assert_eq!(summary.skipped[0].reason, SkipReason::NotUtf8);
    let read = |p: &str| std::fs::read(dir.path().join(p)).unwrap();
    assert_eq!(read("src/a.rs"), b"new new\n");
    assert_eq!(read("src/b.rs"), b"x\r\nnew\r\n");
    assert_eq!(read("ignored/c.rs"), b"old\n");
    assert!(!summary.cancelled);
}

#[test]
fn replace_all_cancelled_changes_nothing() {
    let dir = tree(&[("a.txt", b"old\n")]);
    let token = CancelToken::new();
    token.cancel();
    let summary = replace_all(dir.path(), &SearchQuery::new("old"), "new", &token).unwrap();
    assert!(summary.cancelled);
    assert!(summary.changed.is_empty());
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"old\n");
}

#[test]
fn replace_invalid_regex_errors() {
    assert!(matches!(
        replace_in_text("x", &regex("("), "y", None),
        Err(SearchError::InvalidPattern(_))
    ));
}

#[test]
fn line_replacements_expand_each_match() {
    let q = regex(r"(\w+)=(\d)");
    let got = arcode_search::line_replacements("é a=1, bb=2 x", &q, "$2:$1").unwrap();
    assert_eq!(
        got,
        vec![(2..5, "1:a".to_owned()), (7..11, "2:bb".to_owned())]
    );
    let mut lit = SearchQuery::new("$x");
    lit.case_sensitive = true;
    let got = arcode_search::line_replacements("a $x $X", &lit, "$1").unwrap();
    assert_eq!(got, vec![(2..4, "$1".to_owned())]);
    let empty = arcode_search::line_replacements("abc", &regex("x*"), "y").unwrap();
    assert!(empty.is_empty());
}
