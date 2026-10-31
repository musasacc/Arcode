use std::fs;
use std::path::PathBuf;

use gpui::{AppContext, Entity, TestAppContext};

use super::rows::Row;
use super::*;
use crate::test_support::{init, settle};

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::create_dir_all(root.join("node_modules/x")).expect("mkdir");
    fs::write(root.join("src/a.rs"), "// TODO: one\n// FIXME: two\n").expect("write");
    fs::write(root.join("src/b.rs"), "// NOTE: three\n").expect("write");
    fs::write(root.join("node_modules/x/i.js"), "// TODO: vendored\n").expect("write");
    (dir, root)
}

fn texts(store: &Entity<TodoStore>, cx: &mut TestAppContext) -> Vec<String> {
    store.read_with(cx, |s, _| {
        s.files()
            .values()
            .flatten()
            .map(|i| {
                format!(
                    "{}:{}",
                    i.relative.to_string_lossy().replace('\\', "/"),
                    i.text
                )
            })
            .collect()
    })
}

#[gpui::test]
fn scans_once_then_updates_only_changed_files(cx: &mut TestAppContext) {
    init(cx);
    let (_dir, root) = project();
    let store = cx.new(TodoStore::new);
    store.update(cx, |s, cx| s.set_root(Some(root.clone()), cx));
    settle(cx);
    assert_eq!(
        texts(&store, cx),
        ["src/a.rs:one", "src/a.rs:two", "src/b.rs:three"]
    );

    fs::write(root.join("src/a.rs"), "// TODO: one changed\n").expect("write");
    fs::create_dir_all(root.join("src/new")).expect("mkdir");
    fs::write(root.join("src/new/c.rs"), "// HACK: four\n").expect("write");
    fs::remove_file(root.join("src/b.rs")).expect("rm");
    store.update(cx, |s, cx| {
        s.paths_changed(
            &[
                root.join("src/a.rs"),
                root.join("src/new"),
                root.join("src/b.rs"),
            ],
            false,
            cx,
        )
    });
    settle(cx);
    assert_eq!(
        texts(&store, cx),
        ["src/a.rs:one changed", "src/new/c.rs:four"]
    );
    store.read_with(cx, |s, _| {
        assert_eq!(s.full_scans(), 1, "updates never rescan the whole project");
        assert_eq!(s.count(), 2);
    });

    store.update(cx, |s, cx| s.paths_changed(&[], true, cx));
    settle(cx);
    store.read_with(cx, |s, _| assert_eq!(s.full_scans(), 2, "overflow rescans"));
}

#[gpui::test]
fn views_filter_by_tag_and_open_items(cx: &mut TestAppContext) {
    init(cx);
    let (_dir, root) = project();
    let store = cx.new(TodoStore::new);
    store.update(cx, |s, cx| s.set_root(Some(root.clone()), cx));
    settle(cx);
    let view = cx.new(|cx| TodoView::new(store.clone(), TodoPlacement::Panel, cx));
    view.read_with(cx, |v, _| assert_eq!(v.rows().len(), 5));
    view.update(cx, |v, cx| v.toggle_tag("FIXME", cx));
    view.read_with(cx, |v, _| {
        assert_eq!(v.rows().len(), 2);
        assert!(matches!(&v.rows()[1], Row::Item { index: 1, .. }));
    });
    let opened = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, e: &TodoEvent, _| {
            sink.lock().unwrap().push(e.clone())
        })
        .detach();
    });
    view.update(cx, |v, cx| v.open_row(1, cx));
    assert_eq!(
        opened.lock().unwrap().clone(),
        [TodoEvent::Open {
            path: root.join("src/a.rs"),
            line: 1,
            column: 3
        }]
    );
}

#[test]
fn tags_use_status_colors_never_the_accent() {
    let registry = arcode_config::ThemeRegistry::builtin();
    for name in ["Arcode Ember", "Arcode Chalk"] {
        let theme = registry.get(name).expect("built-in theme").clone();
        let theme = arcode_ui_kit::UiTheme::new(std::sync::Arc::new(theme));
        check_tag_colors(&theme.colors);
    }
}

fn check_tag_colors(c: &arcode_ui_kit::ThemeColors) {
    assert_eq!(tag_color("FIXME", c), c.error);
    assert_eq!(tag_color("@bug", c), c.error);
    assert_eq!(tag_color("HACK", c), c.warning);
    assert_eq!(tag_color("NOTE", c), c.info);
    for tag in ["TODO", "PERF", "todo"] {
        assert_ne!(tag_color(tag, c), c.accent, "{tag}");
    }
}
