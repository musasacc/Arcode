mod common;

use std::ops::ControlFlow;

use arcode_search::{CancelToken, SearchError, SearchQuery, search, search_text};
use common::{chars, paths, run, tree, write};

fn q(pattern: &str) -> SearchQuery {
    SearchQuery::new(pattern)
}

#[test]
fn nested_files_sorted_with_paths() {
    let dir = tree(&[
        ("b.txt", b"needle\n"),
        ("a/z.txt", b"x\nneedle here\n"),
        ("a/b/c.txt", b"needle needle\n"),
        ("a/none.txt", b"nothing\n"),
    ]);
    let (files, stats) = run(dir.path(), &q("needle"));
    assert_eq!(paths(&files), ["a/b/c.txt", "a/z.txt", "b.txt"]);
    assert_eq!(stats.files_searched, 4);
    assert_eq!(stats.files_matched, 3);
    assert_eq!(stats.matches, 4);
    assert!(!stats.truncated && !stats.cancelled);
    let z = &files[1];
    assert!(z.path.is_absolute());
    assert!(z.path.ends_with(common::rel("a/z.txt")));
    assert_eq!(z.matches[0].line, 1);
    assert_eq!(z.matches[0].line_text, "needle here");
    assert_eq!(z.matches[0].ranges, vec![0..6]);
    assert_eq!(files[0].matches[0].ranges, vec![0..6, 7..13]);
    assert_eq!(
        run(dir.path(), &q("needle")).0,
        files,
        "deterministic order"
    );
}

#[test]
fn gitignore_and_ignore_files_respected() {
    let dir = tree(&[
        (".gitignore", b"target/\n*.log\n"),
        ("sub/.ignore", b"skip.txt\n"),
        ("src/main.rs", b"needle\n"),
        ("target/out.rs", b"needle\n"),
        ("debug.log", b"needle\n"),
        ("sub/skip.txt", b"needle\n"),
        ("sub/keep.txt", b"needle\n"),
    ]);
    let (files, _) = run(dir.path(), &q("needle"));
    assert_eq!(paths(&files), ["src/main.rs", "sub/keep.txt"]);

    let mut all = q("needle");
    all.respect_gitignore = false;
    let (files, _) = run(dir.path(), &all);
    assert_eq!(files.len(), 5);
}

#[test]
fn hidden_files_and_git_dir() {
    let dir = tree(&[
        (".env", b"needle\n"),
        (".config/x.toml", b"needle\n"),
        (".git/config", b"needle\n"),
        ("visible.txt", b"needle\n"),
    ]);
    let (files, _) = run(dir.path(), &q("needle"));
    assert_eq!(paths(&files), ["visible.txt"]);
    let mut hidden = q("needle");
    hidden.include_hidden = true;
    let (files, _) = run(dir.path(), &hidden);
    assert_eq!(paths(&files), [".config/x.toml", ".env", "visible.txt"]);
}

#[test]
fn binary_files_skipped() {
    let mut late_nul = b"needle at start\n".to_vec();
    late_nul.extend(std::iter::repeat_n(b'a', 300_000));
    late_nul.extend(b"\0\n");
    let dir = tree(&[
        ("bin.dat", b"\0\x01needle\x02"),
        ("late.dat", &late_nul),
        ("text.txt", b"needle\n"),
    ]);
    let (files, _) = run(dir.path(), &q("needle"));
    assert_eq!(paths(&files), ["text.txt"]);
}

#[test]
fn regex_literal_word_and_case() {
    let dir = tree(&[("f.txt", b"foo_bar Foo\nfoo.bar foobar\nFOO(1)\n")]);
    let lines = |query: &SearchQuery| {
        let (files, _) = run(dir.path(), query);
        files
            .first()
            .map(|f| f.matches.iter().map(|m| m.line).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let mut regex = q(r"foo\.bar|FOO\(\d\)");
    regex.is_regex = true;
    assert_eq!(lines(&regex), [1, 2]);
    assert_eq!(lines(&q("foo.bar")), [1]);
    assert_eq!(lines(&q("FOO(1)")), [2]);

    let mut word = q("foo");
    word.whole_word = true;
    word.case_sensitive = true;
    assert_eq!(lines(&word), [1]);
    word.case_sensitive = false;
    assert_eq!(lines(&word), [0, 1, 2]);

    let mut smart = q("Foo");
    smart.smart_case = true;
    assert_eq!(lines(&smart), [0]);
    smart.pattern = "foo".into();
    assert_eq!(lines(&smart), [0, 1, 2]);
}

#[test]
fn invalid_regex_is_an_error() {
    let dir = tree(&[("f.txt", b"x\n")]);
    let mut query = q("(unclosed");
    query.is_regex = true;
    let err = search(dir.path(), &query, &CancelToken::new(), |_| {
        ControlFlow::Continue(())
    })
    .unwrap_err();
    assert!(matches!(err, SearchError::InvalidPattern(_)), "{err}");
    let err = search(
        dir.path().join("missing").as_path(),
        &q("x"),
        &CancelToken::new(),
        |_| ControlFlow::Continue(()),
    )
    .unwrap_err();
    assert!(matches!(err, SearchError::InvalidRoot(_)));
}

#[test]
fn cancellation_stops_search() {
    let files: Vec<(String, Vec<u8>)> = (0..300)
        .map(|i| (format!("d{}/f{i}.txt", i % 7), b"needle\n".to_vec()))
        .collect();
    let dir = tree(&[]);
    for (rel, body) in &files {
        write(dir.path(), rel, body);
    }

    let token = CancelToken::new();
    token.cancel();
    let mut seen = 0;
    let stats = search(dir.path(), &q("needle"), &token, |_| {
        seen += 1;
        ControlFlow::Continue(())
    })
    .unwrap();
    assert_eq!(seen, 0);
    assert!(stats.cancelled);

    let token = CancelToken::new();
    let mut seen = 0;
    let stats = search(dir.path(), &q("needle"), &token, |_| {
        seen += 1;
        token.cancel();
        ControlFlow::Continue(())
    })
    .unwrap();
    assert_eq!(seen, 1);
    assert!(stats.cancelled);
    assert_eq!(stats.files_matched, 1);
}

#[test]
fn break_stops_delivery() {
    let dir = tree(&[("a.txt", b"x\n"), ("b.txt", b"x\n"), ("c.txt", b"x\n")]);
    let mut seen = 0;
    let stats = search(dir.path(), &q("x"), &CancelToken::new(), |_| {
        seen += 1;
        ControlFlow::Break(())
    })
    .unwrap();
    assert_eq!(seen, 1);
    assert_eq!(stats.files_matched, 1);
    assert!(!stats.cancelled);
}

#[test]
fn max_results_truncates() {
    let dir = tree(&[]);
    for i in 0..10 {
        write(dir.path(), &format!("f{i}.txt"), b"x x\nx\nx x\n");
    }
    let mut query = q("x");
    query.max_results = 7;
    let (files, stats) = run(dir.path(), &query);
    assert!(stats.truncated);
    assert_eq!(stats.matches, 7);
    let total: usize = files.iter().map(|f| f.match_count()).sum();
    assert_eq!(total, 7);

    query.max_results = 50;
    let (_, stats) = run(dir.path(), &query);
    assert!(!stats.truncated);
    assert_eq!(stats.matches, 50);
}

#[test]
fn max_results_cuts_inside_one_line() {
    let dir = tree(&[(
        "a.txt",
        b"x x x x x
",
    )]);
    let mut query = q("x");
    query.max_results = 3;
    let (files, stats) = run(dir.path(), &query);
    assert!(stats.truncated);
    assert_eq!(files[0].matches[0].ranges, vec![0..1, 2..3, 4..5]);
    assert_eq!(files[0].matches[0].columns.len(), 3);

    // Exactly at the cap is not truncation.
    query.max_results = 5;
    let (files, stats) = run(dir.path(), &query);
    assert!(!stats.truncated);
    assert_eq!(files[0].match_count(), 5);

    // search_text applies the same cap.
    query.max_results = 2;
    let lines = search_text(
        "x x
x
", &query,
    )
    .unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].ranges, vec![0..1, 2..3]);
}

#[test]
fn long_lines_truncated_with_correct_ranges() {
    let line = format!(
        "{}Ünïcödé{}needle{}\n",
        "x".repeat(700),
        "y".repeat(10),
        "z".repeat(700)
    );
    let dir = tree(&[("long.txt", line.as_bytes())]);
    let (files, _) = run(dir.path(), &q("needle"));
    let m = &files[0].matches[0];
    assert_eq!(m.columns, vec![717..723]);
    assert!(m.line_text.chars().count() <= arcode_search::MAX_LINE_CHARS + 2);
    assert_eq!(chars(&m.line_text, &m.ranges[0]), "needle");
    assert!(m.line_text.contains("Ünïcödé"));
}

#[test]
fn unicode_columns_and_crlf() {
    let dir = tree(&[("u.txt", "日本語 needle 😀 needle\r\nnext\r\n".as_bytes())]);
    let (files, _) = run(dir.path(), &q("needle"));
    let m = &files[0].matches[0];
    assert_eq!(m.line_text, "日本語 needle 😀 needle");
    assert_eq!(m.ranges, vec![4..10, 13..19]);
    let mut eol = q("needle$");
    eol.is_regex = true;
    let (files, _) = run(dir.path(), &eol);
    assert_eq!(files[0].matches[0].ranges, vec![13..19]);
}

#[test]
fn globs_and_file_size() {
    let dir = tree(&[
        ("src/a.rs", b"needle\n"),
        ("src/gen/b.rs", b"needle\n"),
        ("docs/c.md", b"needle\n"),
        ("big.txt", &[b'n'; 5000]),
        ("big2.txt", format!("needle{}", " ".repeat(5000)).as_bytes()),
    ]);
    let mut query = q("needle");
    query.include_globs = vec!["*.rs".into()];
    assert_eq!(
        paths(&run(dir.path(), &query).0),
        ["src/a.rs", "src/gen/b.rs"]
    );
    query.exclude_globs = vec!["gen".into()];
    assert_eq!(paths(&run(dir.path(), &query).0), ["src/a.rs"]);
    query.include_globs = vec!["docs/".into()];
    query.exclude_globs.clear();
    assert_eq!(paths(&run(dir.path(), &query).0), ["docs/c.md"]);

    let mut small = q("needle");
    small.max_file_size = 1000;
    assert!(!paths(&run(dir.path(), &small).0).contains(&"big2.txt".to_owned()));
    small.max_file_size = 0;
    assert!(paths(&run(dir.path(), &small).0).contains(&"big2.txt".to_owned()));
}

#[test]
fn single_file_root_and_utf16() {
    let dir = tree(&[("one.txt", b"a needle\n")]);
    let (files, _) = run(&dir.path().join("one.txt"), &q("needle"));
    assert_eq!(paths(&files), ["one.txt"]);

    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "x needle\n".encode_utf16() {
        utf16.extend(unit.to_le_bytes());
    }
    write(dir.path(), "wide.txt", &utf16);
    let (files, _) = run(dir.path(), &q("needle"));
    assert_eq!(paths(&files), ["one.txt", "wide.txt"]);
    assert_eq!(files[1].matches[0].ranges, vec![2..8]);
}

#[test]
fn in_memory_text_search() {
    let text = "\u{feff}alpha\r\nbeta Alpha\n";
    let lines = search_text(text, &q("alpha")).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].line, 1);
    assert_eq!(lines[1].ranges, vec![5..10]);
    assert_eq!(lines[0].line_text, "alpha");
    assert!(search_text("x", &q("")).is_err());
}
