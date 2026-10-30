//! Adversarial review tests: issues found while reviewing arcode-search.
mod common;

use arcode_search::{
    CancelToken, SearchQuery, SkipReason, parse_todo_line, replace_all, replace_in_text,
    scan_todos_in_text, search_text,
};
use common::{paths, run, tree};

/// A file with a NUL byte after its matches must never be reported, even
/// when `max_results` stops the searcher before it reaches the NUL.
#[test]
fn review_binary_file_not_delivered_when_budget_runs_out() {
    let mut late_nul = b"needle\n".repeat(10);
    late_nul.extend(std::iter::repeat_n(b'a', 300_000));
    late_nul.extend(b"\0\n");
    let dir = tree(&[("bin.dat", &late_nul)]);
    let mut q = SearchQuery::new("needle");
    q.max_results = 3;
    let (files, _) = run(dir.path(), &q);
    assert!(
        files.is_empty(),
        "binary file delivered: {:?}",
        paths(&files)
    );
}

/// The `SkipReason::NotUtf8` docs say UTF-16 files are reported as
/// `NotUtf8`; the search finds them (BOM sniffing), so replace_all must
/// report them with that reason.
#[test]
fn review_utf16_file_skipped_as_not_utf8() {
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "needle here\n".encode_utf16() {
        utf16.extend(unit.to_le_bytes());
    }
    let dir = tree(&[("wide.txt", &utf16)]);
    let summary = replace_all(
        dir.path(),
        &SearchQuery::new("needle"),
        "pin",
        &CancelToken::new(),
    )
    .expect("replace_all");
    assert_eq!(summary.skipped.len(), 1);
    assert_eq!(summary.skipped[0].reason, SkipReason::NotUtf8);
}

/// Sanity checks that passed during review (kept as regression guards).
#[test]
fn review_passing_edge_cases() {
    // Literal escaping of `&`, `~`, `-`, `#`, `<`, `>`.
    let q = SearchQuery::new("a&b~c-d#e<f>");
    let m = search_text("x a&b~c-d#e<f> y", &q).expect("search");
    assert_eq!(m[0].ranges, vec![2..14]);

    // Smart case with whole word.
    let mut q = SearchQuery::new("foo");
    q.smart_case = true;
    q.whole_word = true;
    assert_eq!(search_text("FOO", &q).expect("search").len(), 1);
    q.pattern = "Foo".into();
    assert!(search_text("foo", &q).expect("search").is_empty());

    // Replace with a regex anchored at `$` on CRLF, unicode replacement.
    let mut q = SearchQuery::new(r"(\w+)$");
    q.is_regex = true;
    let (out, n) = replace_in_text("é a\r\nb\r\n", &q, "<$1>", None).expect("replace");
    assert_eq!((out.as_str(), n), ("é <a>\r\n<b>\r\n", 2));

    // TODO edge cases do not panic and parse sensibly.
    let t = parse_todo_line(0, "// todo(İ): x", &["TODO"], false).expect("todo");
    assert_eq!(t.author.as_deref(), Some("İ"));
    assert!(scan_todos_in_text("TODO", &[]).len() == 1);
    assert!(scan_todos_in_text("", &[]).is_empty());
}

/// Matches in a file that later turns out to be binary are charged to the
/// `max_results` budget. Whichever file the walker visits first, the single
/// real match in `z.txt` must be the only result and nothing is truncated.
#[test]
fn review_binary_file_does_not_consume_result_budget() {
    let mut late_nul = b"needle\n".repeat(5);
    late_nul.extend(std::iter::repeat_n(b'a', 300_000));
    late_nul.extend(b"\0\n");
    let dir = tree(&[("a_bin.dat", &late_nul), ("z.txt", b"needle\n")]);
    let mut q = SearchQuery::new("needle");
    q.max_results = 5;
    let (files, stats) = run(dir.path(), &q);
    assert_eq!(paths(&files), ["z.txt"]);
    assert!(!stats.truncated, "only one real match exists");
}

/// A bare include `src` only covers a top-level `src`, while the same bare
/// exclude `src` removes `src` directories at any depth (gitignore
/// semantics). The two lists should agree on what `src` means.
#[test]
fn review_bare_dir_include_matches_nested_like_exclude() {
    let dir = tree(&[
        ("src/a.rs", b"needle\n"),
        ("lib/src/b.rs", b"needle\n"),
        ("lib/x.rs", b"needle\n"),
    ]);
    let mut q = SearchQuery::new("needle");
    q.exclude_globs = vec!["src".into()];
    let (files, _) = run(dir.path(), &q);
    assert_eq!(paths(&files), ["lib/x.rs"], "exclude removes nested src");
    let mut q = SearchQuery::new("needle");
    q.include_globs = vec!["src".into()];
    let (files, _) = run(dir.path(), &q);
    assert_eq!(paths(&files), ["lib/src/b.rs", "src/a.rs"]);
}

/// Custom tags that start with a non-word character (`@todo`, as used by
/// JSDoc/PHPDoc) can never be found: the parser only tries tags at the
/// start of a word.
#[test]
fn review_custom_tag_with_leading_symbol() {
    let items = scan_todos_in_text("/** @todo fix this */", &["@todo"]);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].text, "fix this");
}
