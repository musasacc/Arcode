use std::fs;

use super::*;

fn touch(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create dirs");
    }
    fs::write(path, "x").expect("write file");
}

#[test]
fn listing_respects_gitignore_and_skips_build_dirs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join(".gitignore"));
    fs::write(root.join(".gitignore"), "*.log\n").expect("write gitignore");
    touch(&root.join("src/main.rs"));
    touch(&root.join("debug.log"));
    touch(&root.join("target/debug/out"));
    touch(&root.join("node_modules/pkg/index.js"));
    touch(&root.join(".git/HEAD"));
    touch(&root.join(".github/ci.yml"));
    let files = list_files(root, 100);
    assert_eq!(files, vec![".github/ci.yml", ".gitignore", "src/main.rs"]);
}

#[test]
fn listing_stops_at_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    for i in 0..10 {
        touch(&dir.path().join(format!("f{i}")));
    }
    assert_eq!(list_files(dir.path(), 4).len(), 4);
}

#[test]
fn dir_entries_sorted_folders_first_case_insensitive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("b.txt"));
    touch(&root.join("A.txt"));
    touch(&root.join("zeta/x"));
    touch(&root.join("Alpha/x"));
    touch(&root.join("target/x"));
    let names: Vec<_> = read_dir_sorted(root).into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["Alpha", "zeta", "A.txt", "b.txt"]);
}

#[test]
fn run_command_detection() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(detect_run_command(dir.path()), None);
    touch(&dir.path().join("main.py"));
    assert_eq!(detect_run_command(dir.path()), Some("python main.py"));
    touch(&dir.path().join("Cargo.toml"));
    assert_eq!(detect_run_command(dir.path()), Some("cargo run"));
}

#[test]
fn root_for_file_prefers_git_then_markers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("loose/notes.txt"));
    assert_eq!(root_for_file(&root.join("loose/notes.txt")), root.join("loose"));
    touch(&root.join("pkg/package.json"));
    touch(&root.join("pkg/src/index.js"));
    assert_eq!(root_for_file(&root.join("pkg/src/index.js")), root.join("pkg"));
    fs::create_dir_all(root.join("repo/.git")).expect("mkdir");
    touch(&root.join("repo/crates/a/Cargo.toml"));
    touch(&root.join("repo/crates/a/src/lib.rs"));
    assert_eq!(
        root_for_file(&root.join("repo/crates/a/src/lib.rs")),
        root.join("repo")
    );
}

#[test]
fn new_project_dirs_are_unique() {
    let dir = tempfile::tempdir().expect("tempdir");
    let a = create_project_dir(dir.path()).expect("first");
    let b = create_project_dir(dir.path()).expect("second");
    assert_ne!(a, b);
    assert!(a.is_dir() && b.is_dir());
}

#[test]
fn relative_display_uses_slashes() {
    let root = Path::new("/p");
    assert_eq!(relative_display(root, &root.join("a").join("b.rs")), "a/b.rs");
}
