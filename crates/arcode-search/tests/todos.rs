mod common;

use arcode_search::{CancelToken, TodoOptions, scan_todos, scan_todos_with};
use common::{slash, tree};

#[test]
fn scans_tree_sorted_with_details() {
    let dir = tree(&[
        (".gitignore", b"vendor/\n"),
        (
            "src/main.rs",
            b"fn main() {}\n// TODO(alice): wire up the UI\n/* FIXME: leak */\n",
        ),
        ("src/lib.rs", b"let TODOS = 1; // HACK workaround\n"),
        ("web/index.html", b"<!-- NOTE: keep in sync -->\r\n"),
        ("vendor/dep.rs", b"// TODO: not ours\n"),
        ("bin.dat", b"// TODO: binary\0"),
        (".hidden/x.rs", b"// TODO: hidden\n"),
    ]);
    let items = scan_todos(dir.path(), &[], &CancelToken::new());
    let summary: Vec<_> = items
        .iter()
        .map(|t| (slash(&t.relative), t.line, t.tag.as_str(), t.text.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("src/lib.rs".to_owned(), 0, "HACK", "workaround"),
            ("src/main.rs".to_owned(), 1, "TODO", "wire up the UI"),
            ("src/main.rs".to_owned(), 2, "FIXME", "leak"),
            ("web/index.html".to_owned(), 0, "NOTE", "keep in sync"),
        ]
    );
    assert_eq!(items[1].author.as_deref(), Some("alice"));
    assert_eq!(items[1].column, 3);
    assert_eq!(items[0].column, 18);
    assert!(items[0].path.is_absolute());
}

#[test]
fn custom_tags_and_case_insensitive() {
    let dir = tree(&[("a.py", b"# perf: slow loop\n# TODO: x\n# PERF(bob) fast\n")]);
    let items = scan_todos(dir.path(), &["PERF"], &CancelToken::new());
    assert_eq!(items.len(), 1);
    assert_eq!(
        (items[0].line, items[0].author.as_deref()),
        (2, Some("bob"))
    );
    assert_eq!(items[0].text, "fast");

    let opts = TodoOptions {
        tags: vec!["PERF".into()],
        case_sensitive: false,
        ..TodoOptions::default()
    };
    let items = scan_todos_with(dir.path(), &opts, &CancelToken::new()).unwrap();
    assert_eq!(items.iter().map(|t| t.line).collect::<Vec<_>>(), [0, 2]);
    assert!(items.iter().all(|t| t.tag == "PERF"));
}

#[test]
fn max_items_cancel_and_missing_root() {
    let dir = tree(&[("a.rs", b"// TODO 1\n// TODO 2\n// TODO 3\n")]);
    let opts = TodoOptions {
        max_items: 2,
        ..TodoOptions::default()
    };
    let items = scan_todos_with(dir.path(), &opts, &CancelToken::new()).unwrap();
    assert_eq!(items.len(), 2);

    let token = CancelToken::new();
    token.cancel();
    assert!(scan_todos(dir.path(), &[], &token).is_empty());
    assert!(scan_todos(&dir.path().join("nope"), &[], &CancelToken::new()).is_empty());
}
