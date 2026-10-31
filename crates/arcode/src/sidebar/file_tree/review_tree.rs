//! Reviewer tests for the file tree (session restore / file tree unit).

use std::fs;

use gpui::{AppContext, TestAppContext};

use super::*;

fn names(tree: &FileTree) -> Vec<String> {
    tree.rows
        .iter()
        .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
        .collect()
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

/// A collapsed folder is no longer watched, but its old listing stays in
/// `children`. Expanding it again shows that stale listing: files created
/// while it was collapsed are missing and deleted ones are still shown
/// (the implementer's report says "changes in collapsed folders appear
/// when they are expanded").
#[gpui::test]
fn review_expanding_a_folder_shows_changes_made_while_collapsed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::create_dir(root.join("src")).expect("mkdir");
    fs::write(root.join("src/old.rs"), "").expect("write");
    let tree = cx.new(|cx| FileTree::new(root.clone(), "demo", cx));
    tree.update(cx, |tree, cx| {
        tree.click(0, cx); // expand src
        assert_eq!(names(tree), ["src", "  old.rs"]);
        tree.click(0, cx); // collapse src
        assert_eq!(names(tree), ["src"]);
    });
    // Changed outside Arcode while the folder is collapsed (unwatched).
    fs::remove_file(root.join("src/old.rs")).expect("rm");
    fs::write(root.join("src/new.rs"), "").expect("write");
    cx.run_until_parked();
    tree.update(cx, |tree, cx| {
        tree.click(0, cx); // expand again
        assert_eq!(
            names(tree),
            ["src", "  new.rs"],
            "the folder shows its listing from before it was collapsed"
        );
    });
}

/// Renaming `a.txt` to `A.txt` inline, on a case-sensitive file system
/// where `A.txt` is a different file, silently replaces `A.txt` (its
/// content is lost, nothing goes to the trash). The rename should be
/// refused with an "already exists" error like any other clash.
#[gpui::test]
fn review_inline_case_rename_does_not_destroy_another_file(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("a.txt"), "lower").expect("write");
    fs::write(root.join("A.txt"), "UPPER").expect("write");
    if fs::read_dir(&root).expect("ls").count() != 2 {
        return; // case-insensitive file system: not applicable
    }
    let (tree, cx) = window_tree(&root, cx);
    tree.update_in(cx, |tree, window, cx| {
        tree.start_edit(EditKind::Rename(root.join("a.txt")), window, cx);
        tree.set_edit_text("A.txt", cx);
        tree.confirm_edit(&ConfirmEdit, window, cx);
    });
    assert_eq!(
        fs::read_to_string(root.join("A.txt")).expect("read"),
        "UPPER",
        "A.txt was overwritten by the case-only rename of a.txt"
    );
    tree.update(cx, |tree, _| {
        assert!(
            tree.edit_error().is_some(),
            "the clash should be reported inline"
        );
    });
}
