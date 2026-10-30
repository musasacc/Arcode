use std::fs;

use gpui::TestAppContext;

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
        assert_eq!(names(tree), vec!["src", "  nested", "  lib.rs", "README.md"]);
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
            let FileTreeEvent::OpenFile(path) = event;
            sink.borrow_mut().push(path.clone());
        })
        .detach();
    });
    tree.update(cx, |tree, cx| tree.click(0, cx));
    assert_eq!(opened.borrow().as_slice(), &[dir.path().join("a.txt")]);
}
