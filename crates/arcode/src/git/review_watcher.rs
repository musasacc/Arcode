//! Review tests for the git file watcher.

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use super::watcher::{Change, RepoWatcher};

fn git(root: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}");
}

/// `GitStore` creates the watcher channel with capacity 4 and drains it
/// only after the 250 ms debounce; the watcher uses `try_send`, so when a
/// few working-tree events arrive first (a build, a formatter, a log file,
/// `git checkout` writing files) the following index/HEAD event is
/// dropped. The store then never emits `IndexChanged`, and gutters and
/// blame of open editors stay stale. Expected: a Git change is never lost
/// behind Worktree changes.
#[test]
fn review_git_change_is_not_dropped_behind_worktree_changes() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    let repo = arcode_git::find_repo(&root).unwrap();
    // Same capacity as `GitStore::start_watching`.
    let (tx, rx) = async_channel::bounded::<Change>(4);
    let Some(_watcher) = RepoWatcher::start(&root, std::slice::from_ref(&repo), tx) else {
        return;
    };
    // The store has taken the first event and is in its debounce sleep.
    for i in 0..8 {
        fs::write(root.join(format!("f{i}.txt")), "x").unwrap();
    }
    std::thread::sleep(Duration::from_millis(50));
    git(&root, &["add", "f0.txt"]); // writes .git/index
    std::thread::sleep(Duration::from_millis(250));
    let mut got = Vec::new();
    while let Ok(kind) = rx.try_recv() {
        got.push(kind);
    }
    assert!(
        got.contains(&Change::Git),
        "the index change was dropped: received {got:?}"
    );
}

/// `RepoWatcher::start` walks the whole project tree (adding one inotify
/// watch per folder) synchronously, and `GitStore::set_repos` calls it on
/// the main thread. On a large project this freezes the UI right after
/// opening it. Expected: starting the watcher returns within a frame (the
/// walk belongs on a background thread).
#[test]
fn review_watcher_start_does_not_walk_the_tree_on_the_caller_thread() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    for a in 0..40 {
        for b in 0..50 {
            let d = root.join(format!("d{a}")).join(format!("e{b}"));
            fs::create_dir_all(&d).unwrap();
            for f in 0..4 {
                fs::write(d.join(format!("f{f}.rs")), "x").unwrap();
            }
        }
    }
    let (tx, _rx) = async_channel::bounded::<Change>(4);
    let started = Instant::now();
    let watcher = RepoWatcher::start(&root, &[], tx);
    let took = started.elapsed();
    drop(watcher);
    assert!(
        took < Duration::from_millis(16),
        "RepoWatcher::start blocked the calling (main) thread for {took:?} on 2,040 folders"
    );
}
