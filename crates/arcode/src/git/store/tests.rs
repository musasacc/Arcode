//! Store tests: status errors, repositories created outside Arcode.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use gpui::{AppContext, Entity, TestAppContext};

use super::GitStore;

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}");
}

fn has_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn settle(cx: &mut TestAppContext) {
    for _ in 0..3 {
        cx.executor().advance_clock(Duration::from_millis(700));
        cx.run_until_parked();
    }
}

fn store_for(root: &Path, cx: &mut TestAppContext) -> Entity<GitStore> {
    let store = cx.new(GitStore::new);
    store.update(cx, |s, cx| s.set_project_root(Some(root.to_path_buf()), cx));
    settle(cx);
    store
}

fn temp_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap().join("p");
    fs::create_dir(&root).unwrap();
    (dir, root)
}

/// A status git cannot read is reported, not shown as "no changes".
#[gpui::test]
fn unreadable_status_is_an_error_not_a_clean_tree(cx: &mut TestAppContext) {
    if !has_git() {
        return;
    }
    let (_dir, root) = temp_root();
    git(&root, &["init", "-q", "-b", "main"]);
    fs::write(root.join("a.txt"), "a\n").unwrap();
    git(&root, &["add", "a.txt"]);
    fs::write(root.join(".git/index"), "not an index").unwrap();
    let store = store_for(&root, cx);
    let error = store.read_with(cx, |s, _| s.snapshot().error.clone());
    assert!(
        error.is_some_and(|e| e.contains("Cannot read the repository status")),
        "a corrupt index must be reported"
    );
    // A missing git binary gets its own hint.
    let text = super::status_error(&arcode_git::GitError::GitNotFound);
    assert!(text.starts_with("Git was not found"), "{text}");
}

/// `git init` run in a terminal is picked up without reopening the folder.
#[gpui::test]
fn repository_created_outside_is_discovered(cx: &mut TestAppContext) {
    if !has_git() {
        return;
    }
    let (_dir, root) = temp_root();
    fs::create_dir(root.join("sub")).unwrap();
    let store = store_for(&root, cx);
    assert!(store.read_with(cx, |s, _| s.is_loaded() && s.repos().is_empty()));
    // Let the watcher thread add its folder watches.
    std::thread::sleep(Duration::from_millis(200));
    git(&root.join("sub"), &["init", "-q", "-b", "main"]);
    let found = (0..60).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        settle(cx);
        store.read_with(cx, |s, _| !s.repos().is_empty())
    });
    assert!(found, "the new repository in sub/ was not discovered");
    let workdir = store.read_with(cx, |s, _| s.repos()[0].workdir.clone());
    assert_eq!(workdir, root.join("sub"));
}

/// Refresh looks for repositories again when there is none, and an
/// Initialize click is refused while discovery has not finished.
#[gpui::test]
fn reload_rediscovers_and_init_waits_for_discovery(cx: &mut TestAppContext) {
    if !has_git() {
        return;
    }
    let (_dir, root) = temp_root();
    let store = cx.new(GitStore::new);
    store.update(cx, |s, cx| {
        s.set_project_root(Some(root.clone()), cx);
        s.init_repository(cx);
    });
    settle(cx);
    assert!(
        !root.join(".git").exists(),
        "init before discovery finished"
    );
    // Created while nobody watches (the watcher is dropped here).
    store.update(cx, |s, _| s.watcher = None);
    git(&root, &["init", "-q", "-b", "main"]);
    store.update(cx, |s, cx| s.reload(cx));
    settle(cx);
    assert_eq!(store.read_with(cx, |s, _| s.repos().len()), 1);
}
