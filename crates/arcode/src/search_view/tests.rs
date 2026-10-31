use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use arcode_buffer::Buffer;
use gpui::TestAppContext;

use super::*;
use crate::test_support::{init, settle};

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::create_dir_all(root.join("target")).expect("mkdir");
    fs::write(root.join("src/a.rs"), "fn alpha() {}\nalpha();\n").expect("write");
    fs::write(root.join("src/b.rs"), "// alpha beta\n").expect("write");
    fs::write(root.join("target/out.rs"), "alpha\n").expect("write");
    fs::write(root.join("latin1.txt"), b"alpha caf\xe9\n").expect("write");
    (dir, root)
}

fn view(root: &Path, cx: &mut TestAppContext) -> Entity<ProjectSearch> {
    init(cx);
    let view = cx.new(ProjectSearch::new);
    view.update(cx, |v, cx| v.set_root(Some(root.to_path_buf()), cx));
    view
}

fn search(view: &Entity<ProjectSearch>, query: &str, cx: &mut TestAppContext) {
    view.update(cx, |v, cx| {
        v.set_query(query, cx);
        v.run(true, cx);
    });
    settle(cx);
}

fn files(view: &Entity<ProjectSearch>, cx: &mut TestAppContext) -> Vec<String> {
    view.read_with(cx, |v, _| {
        v.results()
            .files()
            .iter()
            .map(|f| f.relative.to_string_lossy().replace('\\', "/"))
            .collect()
    })
}

#[gpui::test]
fn results_are_grouped_sorted_and_open_at_the_match(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let view = view(&root, cx);
    search(&view, "alpha", cx);
    assert_eq!(files(&view, cx), ["latin1.txt", "src/a.rs", "src/b.rs"]);
    view.read_with(cx, |v, _| {
        assert_eq!(v.results().match_count(), 4);
        assert!(matches!(v.status(), Status::Done(_)));
    });
    let opened = Arc::new(Mutex::new(Vec::new()));
    let sink = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &ProjectSearchEvent, _| {
            sink.lock().unwrap().push(event.clone());
        })
        .detach();
    });
    // Rows: latin1 header + 1 line, a.rs header + 2 lines, ...
    view.update(cx, |v, cx| v.open_row(4, cx));
    let events = opened.lock().unwrap().clone();
    assert_eq!(
        events,
        [ProjectSearchEvent::Open {
            path: root.join("src/a.rs"),
            range: Point::new(1, 0)..Point::new(1, 5),
        }]
    );
    view.update(cx, |v, cx| v.open_row(2, cx));
    view.read_with(cx, |v, _| assert_eq!(v.rows.len(), 5, "a.rs collapsed"));
}

#[gpui::test]
fn options_globs_errors_and_the_result_cap(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let view = view(&root, cx);
    view.update(cx, |v, cx| v.set_globs("src", "b.rs", cx));
    search(&view, "alpha", cx);
    assert_eq!(files(&view, cx), ["src/a.rs"]);
    view.update(cx, |v, cx| {
        v.set_globs("", "", cx);
        v.toggle_option(SearchOption::Regex, cx);
    });
    search(&view, "alpha(", cx);
    view.read_with(cx, |v, _| {
        assert!(matches!(v.status(), Status::Invalid(_)));
        assert!(v.results().files().is_empty());
    });
    view.update(cx, |v, cx| {
        v.toggle_option(SearchOption::Regex, cx);
        v.set_query("alpha", cx);
    });
    settle(cx);
    view.update(cx, |v, cx| {
        v.limit = 2;
        v.run(true, cx);
    });
    settle(cx);
    view.read_with(cx, |v, _| {
        assert!(v.results().match_count() <= 2);
        assert!(matches!(v.status(), Status::Done(s) if s.truncated));
    });
    view.update(cx, |v, cx| v.show_more(cx));
    settle(cx);
    view.read_with(cx, |v, _| assert_eq!(v.results().match_count(), 4));
}

#[gpui::test]
fn typing_searches_after_the_debounce(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let view = view(&root, cx);
    view.update(cx, |v, cx| v.set_query("beta", cx));
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert!(v.results().files().is_empty(), "not yet")
    });
    settle(cx);
    assert_eq!(files(&view, cx), ["src/b.rs"]);
}

#[gpui::test]
fn replace_in_files_uses_open_editors_and_skips_non_utf8(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let view = view(&root, cx);
    let mut buffer = Buffer::from_file(root.join("src/a.rs")).expect("buffer");
    buffer.set_path(root.join("src/a.rs"));
    let editor = cx.new(|cx| Editor::new(buffer, cx));
    let editors = editor.clone();
    view.update(cx, |v, _| {
        v.set_editors_fn(Rc::new(move |_| vec![editors.clone()]))
    });
    search(&view, "alpha", cx);
    let paths: Vec<PathBuf> = view.read_with(cx, |v, _| {
        v.results().files().iter().map(|f| f.path.clone()).collect()
    });
    view.update(cx, |v, cx| {
        v.set_replacement("omega", cx);
        let (query, replacement) = v.replace_query(cx).expect("valid query");
        v.replace_files(paths, query, replacement, cx);
    });
    settle(cx);
    let a_buffer = editor.read_with(cx, |e, _| e.buffer().text().to_string());
    assert_eq!(a_buffer, "fn omega() {}\nomega();\n");
    assert!(editor.read_with(cx, |e, _| e.is_dirty()), "not saved");
    assert_eq!(
        fs::read_to_string(root.join("src/a.rs")).unwrap(),
        "fn alpha() {}\nalpha();\n",
        "the open file is changed in its editor only"
    );
    assert_eq!(
        fs::read_to_string(root.join("src/b.rs")).unwrap(),
        "// omega beta\n"
    );
    assert_eq!(
        fs::read(root.join("latin1.txt")).unwrap(),
        b"alpha caf\xe9\n"
    );
    let notice = view.read_with(cx, |v, _| v.notice().map(|n| n.to_string()));
    let notice = notice.expect("notice");
    assert!(
        notice.starts_with("Replaced 3 occurrences in 2 files."),
        "{notice}"
    );
    assert!(notice.contains("latin1.txt (not UTF-8)"), "{notice}");
    // The dirty buffer's text is what the results show now.
    assert_eq!(files(&view, cx), ["latin1.txt"]);
    editor.update(cx, |e, cx| e.undo(cx));
    let a_buffer = editor.read_with(cx, |e, _| e.buffer().text().to_string());
    assert_eq!(a_buffer, "fn alpha() {}\nalpha();\n", "undoable");
}

#[test]
fn preview_inserts_replacements_after_matches() {
    let (text, spans) =
        line_row::preview_line("a xy b xy", &[2..4, 7..9], Some(&["Z".into(), "".into()]));
    assert_eq!(text, "a xyZ b xy");
    assert_eq!(
        spans,
        vec![
            (2..4, line_row::Span::Match),
            (4..5, line_row::Span::Insert),
            (8..10, line_row::Span::Match)
        ]
    );
    let (text, spans) =
        line_row::preview_line("é xy", &std::iter::once(2..4).collect::<Vec<_>>(), None);
    assert_eq!(text, "é xy");
    assert_eq!(spans, vec![(3..5, line_row::Span::Match)]);
}

#[gpui::test]
fn unsaved_buffers_are_searched_even_when_their_file_does_not_match(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let view = view(&root, cx);
    let mut buffer = Buffer::from_file(root.join("src/b.rs")).expect("buffer");
    buffer.set_path(root.join("src/b.rs"));
    let editor = cx.new(|cx| Editor::new(buffer, cx));
    editor.update(cx, |e, cx| {
        e.apply_edits(
            vec![(Point::new(0, 0)..Point::new(0, 0), "zeta ".into())],
            cx,
        );
    });
    let editors = editor.clone();
    view.update(cx, |v, _| {
        v.set_editors_fn(Rc::new(move |_| vec![editors.clone()]))
    });
    search(&view, "zeta", cx);
    assert_eq!(files(&view, cx), ["src/b.rs"], "listed from the buffer");
    view.update(cx, |v, cx| v.set_globs("*.txt", "", cx));
    settle(cx);
    assert!(
        files(&view, cx).is_empty(),
        "the include glob still applies"
    );

    // Replace All covers it as well (through the editor).
    view.update(cx, |v, cx| v.set_globs("", "", cx));
    settle(cx);
    let (query, replacement) = view.update(cx, |v, cx| {
        v.set_replacement("eta", cx);
        v.replace_query(cx).expect("valid")
    });
    let counted = view.read_with(cx, |v, cx| v.unsaved_only_counts(&[], &query, cx));
    assert_eq!(counted, [(root.join("src/b.rs"), 1)]);
    view.update(cx, |v, cx| {
        v.replace_files(vec![root.join("src/b.rs")], query, replacement, cx)
    });
    settle(cx);
    assert_eq!(
        editor.read_with(cx, |e, _| e.buffer().text().to_string()),
        "eta // alpha beta\n"
    );
}
