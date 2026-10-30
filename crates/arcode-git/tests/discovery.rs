//! Repository discovery and HEAD parsing against hand-written `.git` layouts.

use std::fs;
use std::path::Path;

use arcode_git::{Head, find_repo};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Creates `dir/.git` with the given HEAD content.
fn git_dir(dir: &Path, head: &str) {
    write(&dir.join(".git").join("HEAD"), head);
}

#[test]
fn branch_from_loose_ref() {
    let tmp = tempfile::tempdir().unwrap();
    git_dir(tmp.path(), "ref: refs/heads/main\n");
    write(
        &tmp.path().join(".git/refs/heads/main"),
        &format!("{SHA}\n"),
    );

    let repo = find_repo(tmp.path()).unwrap();
    assert_eq!(repo.workdir, tmp.path());
    assert_eq!(repo.git_dir, tmp.path().join(".git"));
    assert_eq!(repo.common_dir, repo.git_dir);
    assert!(!repo.is_linked());
    assert_eq!(repo.head(), Head::Branch("main".into()));
    assert_eq!(repo.branch_display(), "main");
}

#[test]
fn branch_with_slash_from_packed_refs() {
    let tmp = tempfile::tempdir().unwrap();
    git_dir(tmp.path(), "ref: refs/heads/feature/login\n");
    write(
        &tmp.path().join(".git/packed-refs"),
        &format!("# pack-refs with: peeled\n{SHA} refs/heads/feature/login\n"),
    );
    let repo = find_repo(tmp.path()).unwrap();
    assert_eq!(repo.head(), Head::Branch("feature/login".into()));
    assert_eq!(repo.branch_display(), "feature/login");
}

#[test]
fn unborn_branch_without_refs() {
    let tmp = tempfile::tempdir().unwrap();
    git_dir(tmp.path(), "ref: refs/heads/trunk\n");
    let repo = find_repo(tmp.path()).unwrap();
    assert_eq!(repo.head(), Head::Unborn("trunk".into()));
    assert_eq!(repo.branch_display(), "trunk");
}

#[test]
fn detached_head() {
    let tmp = tempfile::tempdir().unwrap();
    git_dir(tmp.path(), &format!("{SHA}\n"));
    let repo = find_repo(tmp.path()).unwrap();
    assert_eq!(repo.head(), Head::Detached("0123456".into()));
    assert_eq!(repo.branch_display(), "detached@0123456");
}

#[test]
fn malformed_head_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    git_dir(tmp.path(), "garbage\n");
    let repo = find_repo(tmp.path()).unwrap();
    assert!(repo.try_head().is_err());
    assert_eq!(repo.head(), Head::Detached(String::new()));
    assert_eq!(repo.branch_display(), "detached");

    git_dir(tmp.path(), "ref:   \n");
    assert!(repo.try_head().is_err());
}

#[test]
fn nested_subdirectory_and_file_lookup() {
    let tmp = tempfile::tempdir().unwrap();
    git_dir(tmp.path(), "ref: refs/heads/main\n");
    let deep = tmp.path().join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    let file = deep.join("file.rs");
    fs::write(&file, "fn main() {}\n").unwrap();

    for start in [deep.clone(), file, deep.join("does-not-exist.txt")] {
        let repo = find_repo(&start).unwrap();
        assert_eq!(repo.workdir, tmp.path(), "from {}", start.display());
    }
    // `..` and `.` in the input are normalized.
    let dotted = deep.join("..").join(".").join("c");
    assert_eq!(find_repo(&dotted).unwrap().workdir, tmp.path());
}

#[test]
fn nearest_repository_wins() {
    let tmp = tempfile::tempdir().unwrap();
    git_dir(tmp.path(), "ref: refs/heads/outer\n");
    let inner = tmp.path().join("vendor").join("inner");
    git_dir(&inner, "ref: refs/heads/inner\n");
    let repo = find_repo(&inner.join("src")).unwrap();
    assert_eq!(repo.workdir, inner);
    assert_eq!(repo.head(), Head::Unborn("inner".into()));
}

#[test]
fn git_dir_without_head_is_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    git_dir(tmp.path(), "ref: refs/heads/main\n");
    let sub = tmp.path().join("sub");
    fs::create_dir_all(sub.join(".git")).unwrap();
    assert_eq!(find_repo(&sub).unwrap().workdir, tmp.path());
}

#[test]
fn no_repository() {
    let tmp = tempfile::tempdir().unwrap();
    // The temp dir itself might live inside some repository on odd setups;
    // all we require is that nothing inside it is reported.
    let found = find_repo(tmp.path());
    assert!(found.is_none_or(|r| !r.workdir.starts_with(tmp.path())));
}

#[test]
fn worktree_with_relative_gitdir_and_commondir() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    git_dir(&main, "ref: refs/heads/main\n");
    write(&main.join(".git/refs/heads/feat"), &format!("{SHA}\n"));
    let wt_git = main.join(".git").join("worktrees").join("wt");
    write(&wt_git.join("HEAD"), "ref: refs/heads/feat\n");
    write(&wt_git.join("commondir"), "../..\n");

    let wt = tmp.path().join("wt");
    write(&wt.join(".git"), "gitdir: ../main/.git/worktrees/wt\n");
    fs::create_dir_all(wt.join("src")).unwrap();

    let repo = find_repo(&wt.join("src")).unwrap();
    assert_eq!(repo.workdir, wt);
    assert_eq!(repo.git_dir, wt_git);
    assert_eq!(repo.common_dir, main.join(".git"));
    assert!(repo.is_linked());
    // HEAD comes from the worktree's git dir, the ref from the common dir.
    assert_eq!(repo.head(), Head::Branch("feat".into()));
}

#[test]
fn submodule_with_absolute_gitdir() {
    let tmp = tempfile::tempdir().unwrap();
    let modules = tmp
        .path()
        .join("super")
        .join(".git")
        .join("modules")
        .join("lib");
    write(&modules.join("HEAD"), &format!("{SHA}\n"));
    let sub = tmp.path().join("super").join("lib");
    write(
        &sub.join(".git"),
        &format!("gitdir: {}\n", modules.display()),
    );

    let repo = find_repo(&sub).unwrap();
    assert_eq!(repo.workdir, sub);
    assert_eq!(repo.git_dir, modules);
    assert_eq!(repo.common_dir, modules);
    assert_eq!(repo.branch_display(), "detached@0123456");
}

#[test]
fn broken_gitdir_file_yields_none() {
    let tmp = tempfile::tempdir().unwrap();
    git_dir(tmp.path(), "ref: refs/heads/main\n");
    let sub = tmp.path().join("sub");
    write(&sub.join(".git"), "gitdir: ../missing\n");
    assert_eq!(find_repo(&sub), None);

    write(&sub.join(".git"), "not a gitdir line\n");
    assert_eq!(find_repo(&sub), None);
}
