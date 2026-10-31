//! Workspace side of the file tree and the New Project dialog.

use std::fs;

use gpui::TestAppContext;

use super::tests::{open, project_dir};
use crate::new_project::{Create, NewProjectDialog, NewProjectEvent};
use crate::sidebar::file_tree::FileTreeEvent;

#[gpui::test]
fn renames_and_deletes_follow_open_editors(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&root.join("src/main.rs"), window, cx);
        ws.open_file(&root.join("src/lib.rs"), window, cx);
    });
    // The folder moves on disk, then the tree reports it.
    fs::rename(root.join("src"), root.join("code")).expect("rename");
    ws.update_in(cx, |ws, window, cx| {
        let event = FileTreeEvent::Renamed {
            from: root.join("src"),
            to: root.join("code"),
        };
        ws.on_tree_event(&event, window, cx);
        let paths: Vec<_> = ws
            .editors(cx)
            .iter()
            .filter_map(|e| e.read(cx).path().map(|p| p.to_path_buf()))
            .collect();
        assert!(paths.contains(&root.join("code/main.rs")), "{paths:?}");
        assert!(paths.contains(&root.join("code/lib.rs")), "{paths:?}");
    });
    // A dirty editor of a deleted file stays open; a clean one closes.
    let lib = ws.update(cx, |ws, cx| {
        ws.editors(cx)
            .into_iter()
            .find(|e| e.read(cx).title() == "lib.rs")
            .expect("lib")
    });
    lib.update(cx, |e, cx| {
        let at = arcode_buffer::Point::new(0, 0);
        e.apply_edits(vec![(at..at, "x".into())], cx);
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.on_tree_event(&FileTreeEvent::Deleted(root.join("code")), window, cx);
        let titles: Vec<_> = ws
            .editors(cx)
            .iter()
            .map(|e| e.read(cx).title().to_string())
            .collect();
        assert_eq!(titles, ["lib.rs"]);
    });
}

#[gpui::test]
fn new_project_dialog_creates_and_reports_the_command(cx: &mut TestAppContext) {
    let parent = tempfile::tempdir().expect("tempdir");
    let parent_path = fs::canonicalize(parent.path()).expect("canonical");
    crate::test_support::init(cx);
    let start = parent_path.clone();
    let (dialog, cx) = cx.add_window_view(move |_, cx| NewProjectDialog::new(start, cx));
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        cx.subscribe(&dialog, move |_, e: &NewProjectEvent, _| {
            sink.borrow_mut().push(e.clone())
        })
        .detach()
    });
    dialog.update_in(cx, |d, window, cx| {
        d.set_name("a/b", cx);
        d.create(&Create, window, cx);
        assert!(d.error().is_some());
        d.set_name("demo", cx);
        d.select_template("rust-bin", cx);
        assert_eq!(d.template().id, "rust-bin");
        d.create(&Create, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        seen.borrow().as_slice(),
        [NewProjectEvent::Created {
            path: parent_path.join("demo"),
            command: Some("cargo init --name demo".into()),
            new_window: false,
        }]
    );
    assert!(parent_path.join("demo").is_dir());
}

#[gpui::test]
fn workspace_opens_the_created_project(cx: &mut TestAppContext) {
    let parent = tempfile::tempdir().expect("tempdir");
    let parent_path = fs::canonicalize(parent.path()).expect("canonical");
    let (ws, cx) = open(cx);
    cx.update(|_, cx| crate::new_project::bind_keys(cx));
    ws.update_in(cx, |ws, window, cx| ws.prompt_new_project(window, cx));
    let dialog = ws.read_with(cx, |ws, _| {
        ws.new_project_dialog().cloned().expect("dialog")
    });
    // The empty-folder template runs nothing, so no terminal starts here.
    dialog.update_in(cx, |d, window, cx| {
        d.set_location(&parent_path.display().to_string(), cx);
        d.set_name("fresh", cx);
        d.create(&Create, window, cx);
    });
    cx.run_until_parked();
    ws.update(cx, |ws, _| {
        assert!(ws.new_project_dialog().is_none());
        assert_eq!(ws.project_root(), Some(parent_path.join("fresh").as_path()));
        assert!(!ws.bottom_panel_visible);
    });
}
