use std::fs;

use gpui::{AppContext, TestAppContext};

use super::*;

fn names(tree: &FileTree) -> Vec<String> {
    tree.rows
        .iter()
        .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
        .collect()
}

#[gpui::test]
fn lazy_expand_reveal_and_refresh(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join("src/nested")).expect("mkdir");
    fs::write(root.join("src/nested/deep.rs"), "").expect("write");
    fs::write(root.join("src/lib.rs"), "").expect("write");
    fs::write(root.join("README.md"), "").expect("write");

    let tree = cx.new(|cx| FileTree::new(root.clone(), "demo", cx));
    tree.update(cx, |tree, cx| {
        assert_eq!(names(tree), vec!["src", "README.md"]);
        tree.click(0, cx);
        assert_eq!(
            names(tree),
            vec!["src", "  nested", "  lib.rs", "README.md"]
        );
        tree.click(0, cx);
        assert_eq!(names(tree), vec!["src", "README.md"]);

        tree.reveal(&root.join("src/nested/deep.rs"), cx);
        assert_eq!(
            names(tree),
            vec!["src", "  nested", "    deep.rs", "  lib.rs", "README.md"]
        );
        assert_eq!(tree.selected_index(), Some(2));

        fs::write(root.join("src/new.rs"), "").expect("write");
        tree.refresh(cx);
        assert!(names(tree).contains(&"  new.rs".to_string()));

        tree.collapse_all(cx);
        assert_eq!(names(tree), vec!["src", "README.md"]);
    });
}

#[gpui::test]
fn clicking_a_file_emits_open(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.txt"), "").expect("write");
    let tree = cx.new(|cx| FileTree::new(dir.path().to_path_buf(), "demo", cx));
    let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = opened.clone();
    cx.update(|cx| {
        cx.subscribe(&tree, move |_, event: &FileTreeEvent, _| {
            if let FileTreeEvent::OpenFile(path) = event {
                sink.borrow_mut().push(path.clone());
            }
        })
        .detach();
    });
    tree.update(cx, |tree, cx| tree.click(0, cx));
    assert_eq!(opened.borrow().as_slice(), &[dir.path().join("a.txt")]);
}

#[gpui::test]
fn revealing_a_file_outside_the_root_clears_the_selection(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let other = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("main.rs"), "").expect("write");
    let tree = cx.new(|cx| FileTree::new(dir.path().to_path_buf(), "demo", cx));
    tree.update(cx, |tree, cx| {
        tree.reveal(&dir.path().join("main.rs"), cx);
        assert_eq!(tree.selected_index(), Some(0));
        tree.reveal(&other.path().join("config.toml"), cx);
        assert_eq!(tree.selected_path(), None);
    });
}

fn events(
    tree: &gpui::Entity<FileTree>,
    cx: &mut gpui::VisualTestContext,
) -> std::rc::Rc<std::cell::RefCell<Vec<FileTreeEvent>>> {
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        cx.subscribe(tree, move |_, event: &FileTreeEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    seen
}

fn window_tree<'a>(
    root: &std::path::Path,
    cx: &'a mut TestAppContext,
) -> (gpui::Entity<FileTree>, &'a mut gpui::VisualTestContext) {
    crate::test_support::init(cx);
    cx.update(crate::sidebar::file_tree::bind_keys);
    let root = root.to_path_buf();
    cx.add_window_view(move |window, cx| {
        let tree = FileTree::new(root, "demo", cx);
        window.focus(&tree.focus_handle);
        tree
    })
}

#[gpui::test]
fn new_file_and_folder_are_typed_inline(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir(root.join("src")).expect("mkdir");
    let (tree, cx) = window_tree(&root, cx);
    let seen = events(&tree, cx);
    tree.update_in(cx, |tree, window, cx| {
        tree.selected = Some(root.join("src"));
        tree.new_file(&NewFile, window, cx);
        assert_eq!(tree.editing(), Some(&EditKind::NewFile(root.join("src"))));
        // The placeholder row sits right under the (now expanded) folder.
        assert!(tree.rows[1].placeholder);
        assert_eq!(tree.rows[1].depth, 1);
        tree.set_edit_text("main.rs", cx);
        tree.confirm_edit(&ConfirmEdit, window, cx);
        assert!(tree.editing().is_none());
    });
    assert!(root.join("src/main.rs").is_file());
    assert_eq!(
        seen.borrow().as_slice(),
        [FileTreeEvent::OpenFile(root.join("src/main.rs"))]
    );
    tree.update_in(cx, |tree, window, cx| {
        assert_eq!(
            tree.selected_path(),
            Some(root.join("src/main.rs").as_path())
        );
        assert_eq!(names(tree), ["src", "  main.rs"]);
        // A name that exists keeps the field open with an error.
        tree.new_folder(&NewFolder, window, cx);
        assert_eq!(tree.editing(), Some(&EditKind::NewFolder(root.join("src"))));
        tree.set_edit_text("main.rs", cx);
        tree.confirm_edit(&ConfirmEdit, window, cx);
        assert!(tree.edit_error().is_some());
        tree.set_edit_text("a/b", cx);
    });
    tree.update_in(cx, |tree, window, cx| {
        assert!(tree.edit_error().is_none(), "typing clears the error");
        tree.confirm_edit(&ConfirmEdit, window, cx);
        assert!(tree.edit_error().unwrap().contains('/'));
        tree.set_edit_text("util", cx);
        tree.confirm_edit(&ConfirmEdit, window, cx);
    });
    assert!(root.join("src/util").is_dir());
    // Escape cancels without creating anything.
    tree.update_in(cx, |tree, window, cx| {
        tree.new_file(&NewFile, window, cx);
        tree.set_edit_text("never.rs", cx);
        tree.cancel_edit(window, cx);
        assert!(tree.rows.iter().all(|r| !r.placeholder));
    });
    assert!(!root.join("src/util/never.rs").exists());
}

#[gpui::test]
fn rename_moves_expanded_state_and_reports(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir_all(root.join("old/inner")).expect("mkdir");
    fs::write(root.join("old/inner/a.rs"), "").expect("write");
    let (tree, cx) = window_tree(&root, cx);
    let seen = events(&tree, cx);
    tree.update_in(cx, |tree, window, cx| {
        tree.reveal(&root.join("old/inner/a.rs"), cx);
        tree.selected = Some(root.join("old"));
        tree.rename(&Rename, window, cx);
        assert_eq!(tree.edit_text(cx).as_deref(), Some("old"));
        tree.set_edit_text("new", cx);
        tree.confirm_edit(&ConfirmEdit, window, cx);
        assert_eq!(names(tree), ["new", "  inner", "    a.rs"]);
        assert_eq!(tree.selected_path(), Some(root.join("new").as_path()));
    });
    assert_eq!(
        seen.borrow().as_slice(),
        [FileTreeEvent::Renamed {
            from: root.join("old"),
            to: root.join("new"),
        }]
    );
}

#[gpui::test]
fn context_menu_entries_and_keys(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("a.txt"), "").expect("write");
    let (tree, cx) = window_tree(&root, cx);
    let seen = events(&tree, cx);
    tree.update(cx, |tree, cx| {
        tree.open_menu(
            root.join("a.txt"),
            gpui::point(gpui::px(10.), gpui::px(10.)),
            cx,
        );
        let labels = tree.menu_labels();
        assert_eq!(labels[0], "Open");
        for label in [
            "New File…",
            "Rename…",
            "Delete",
            "Copy Path",
            "Copy Relative Path",
            "Open in Terminal",
        ] {
            assert!(labels.contains(&label), "{label} missing from {labels:?}");
        }
        assert!(labels.iter().any(|l| l.starts_with("Reveal in")));
        tree.open_menu(root.clone(), gpui::point(gpui::px(10.), gpui::px(10.)), cx);
        let labels = tree.menu_labels();
        assert!(!labels.contains(&"Rename…") && !labels.contains(&"Delete"));
    });
    cx.run_until_parked();
    // Keyboard: Down twice from nothing lands on the second entry of the
    // root menu ("New Folder…"); Enter runs it and the menu closes.
    cx.simulate_keystrokes("down down enter");
    cx.run_until_parked();
    tree.update(cx, |tree, _| {
        assert!(!tree.menu_open());
        assert_eq!(tree.editing(), Some(&EditKind::NewFolder(root.clone())));
    });
    // Copy Relative Path through the menu of a file.
    tree.update_in(cx, |tree, window, cx| {
        tree.cancel_edit(window, cx);
        tree.open_menu(
            root.join("a.txt"),
            gpui::point(gpui::px(10.), gpui::px(10.)),
            cx,
        );
        tree.copy_relative_path(&CopyRelativePath, window, cx);
        assert!(!tree.menu_open());
        tree.selected = Some(root.join("a.txt"));
        tree.open_in_terminal(&OpenInTerminal, window, cx);
    });
    let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert_eq!(copied.as_deref(), Some("a.txt"));
    assert_eq!(
        seen.borrow().last(),
        Some(&FileTreeEvent::OpenTerminal(root.clone()))
    );
}

#[gpui::test]
fn delete_asks_then_trashes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("a.txt"), "").expect("write");
    fs::write(root.join("b.txt"), "").expect("write");
    let (tree, cx) = window_tree(&root, cx);
    let seen = events(&tree, cx);
    tree.update_in(cx, |tree, window, cx| {
        tree.selected = Some(root.join("a.txt"));
        tree.delete(&Delete, window, cx);
    });
    // Cancel keeps the file.
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(root.join("a.txt").exists());
    assert!(seen.borrow().is_empty());
    // `deleted` is what a successful trash (or permanent delete) calls.
    tree.update(cx, |tree, cx| {
        fs::remove_file(root.join("a.txt")).expect("rm");
        tree.deleted(&root.join("a.txt"), cx);
        assert_eq!(names(tree), ["b.txt"]);
        assert_eq!(tree.selected_path(), Some(root.join("b.txt").as_path()));
    });
    assert_eq!(
        seen.borrow().as_slice(),
        [FileTreeEvent::Deleted(root.join("a.txt"))]
    );
}

#[gpui::test]
fn external_changes_are_applied_per_folder(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir(root.join("src")).expect("mkdir");
    let (tree, cx) = window_tree(&root, cx);
    tree.update(cx, |tree, cx| {
        tree.reveal(&root.join("src"), cx);
        tree.click(0, cx);
        assert_eq!(names(tree), ["src"]);
        assert_eq!(tree.watched_count(), 2, "root and src are watched");
    });
    fs::write(root.join("src/x.rs"), "").expect("write");
    fs::write(root.join("top.md"), "").expect("write");
    tree.update(cx, |tree, cx| {
        tree.fs_changed(vec![root.join("src"), root.join("not-shown")], cx)
    });
    cx.run_until_parked();
    tree.update(cx, |tree, cx| {
        assert_eq!(names(tree), ["src", "  x.rs"], "only src was re-read");
        tree.fs_changed(vec![root.clone()], cx);
    });
    cx.run_until_parked();
    tree.update(cx, |tree, cx| {
        assert_eq!(names(tree), ["src", "  x.rs", "top.md"]);
        fs::remove_dir_all(root.join("src")).expect("rm");
        tree.fs_changed(vec![root.join("src"), root.clone()], cx);
    });
    cx.run_until_parked();
    tree.update(cx, |tree, _| {
        assert_eq!(names(tree), ["top.md"]);
        assert_eq!(tree.watched_count(), 1);
    });
}

#[gpui::test]
fn dropping_moves_into_folders(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir(root.join("dest")).expect("mkdir");
    fs::write(root.join("a.txt"), "").expect("write");
    let (tree, cx) = window_tree(&root, cx);
    let seen = events(&tree, cx);
    tree.update(cx, |tree, cx| {
        tree.move_entry(&root.join("a.txt"), &root.join("dest"), cx);
        assert_eq!(names(tree), ["dest", "  a.txt"]);
        // Into itself is refused with a message.
        tree.move_entry(&root.join("dest"), &root.join("dest"), cx);
    });
    assert!(root.join("dest/a.txt").is_file());
    assert_eq!(
        seen.borrow()[0],
        FileTreeEvent::Renamed {
            from: root.join("a.txt"),
            to: root.join("dest/a.txt"),
        }
    );
}

#[gpui::test]
fn keyboard_navigation(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir_all(root.join("src/inner")).expect("mkdir");
    fs::write(root.join("src/lib.rs"), "").expect("write");
    fs::write(root.join("z.txt"), "").expect("write");
    let (tree, cx) = window_tree(&root, cx);
    let seen = events(&tree, cx);
    cx.simulate_keystrokes("down right");
    tree.update(cx, |tree, _| {
        assert_eq!(names(tree), ["src", "  inner", "  lib.rs", "z.txt"])
    });
    // Right on an open folder steps into it; Left goes back to the parent,
    // then collapses it.
    cx.simulate_keystrokes("right");
    tree.update(cx, |tree, _| assert_eq!(tree.selected_index(), Some(1)));
    cx.simulate_keystrokes("left");
    tree.update(cx, |tree, _| assert_eq!(tree.selected_index(), Some(0)));
    cx.simulate_keystrokes("left");
    tree.update(cx, |tree, _| assert_eq!(names(tree), ["src", "z.txt"]));
    cx.simulate_keystrokes("end enter");
    assert_eq!(
        seen.borrow().as_slice(),
        [FileTreeEvent::OpenFile(root.join("z.txt"))]
    );
    cx.simulate_keystrokes("home f2");
    tree.update(cx, |tree, _| {
        assert_eq!(tree.editing(), Some(&EditKind::Rename(root.join("src"))))
    });
    cx.simulate_keystrokes("escape");
    tree.update(cx, |tree, _| assert!(tree.editing().is_none()));
}
