//! Tests of the git file watcher.

use super::*;

#[test]
fn classifies_git_and_worktree_paths() {
    let git = PathBuf::from("/p/.git");
    let dirs = [git.clone()];
    assert_eq!(classify(&git.join("index"), &dirs), Some(Change::Git));
    assert_eq!(classify(&git.join("HEAD"), &dirs), Some(Change::Git));
    assert_eq!(
        classify(&git.join("refs/heads/main"), &dirs),
        Some(Change::Git)
    );
    assert_eq!(classify(&git.join("index.lock"), &dirs), None);
    assert_eq!(classify(&git.join("objects/ab/cd"), &dirs), None);
    assert_eq!(
        classify(Path::new("/p/src/main.rs"), &dirs),
        Some(Change::Worktree)
    );
    assert_eq!(classify(Path::new("/p/target/debug/x"), &dirs), None);
    assert_eq!(classify(Path::new("/p/sub/.git/index"), &dirs), None);
    assert_eq!(
        classify(Path::new("/p/sub/.git"), &dirs),
        Some(Change::Repo)
    );
    assert_eq!(classify(&git, &dirs), None, "the known repository itself");
}

#[test]
fn install_folders_are_markers_where_changes_are_reported() {
    let dirs = [PathBuf::from("/p/.git")];
    assert!(is_install_marker(Path::new("/p/node_modules"), &dirs));
    assert!(is_install_marker(Path::new("/p/web/.venv"), &dirs));
    assert!(!is_install_marker(Path::new("/p/src/main.rs"), &dirs));
    assert!(!is_install_marker(
        Path::new("/p/node_modules/a/node_modules"),
        &dirs
    ));
    assert!(!is_install_marker(Path::new("/p/.git/vendor"), &dirs));
}

#[test]
fn path_sink_dedupes_and_caps() {
    let mut sink = PathSink::default();
    sink.push(Path::new("/p/b"));
    sink.push(Path::new("/p/a"));
    sink.push(Path::new("/p/b"));
    assert_eq!(
        sink.take(),
        (vec![PathBuf::from("/p/a"), PathBuf::from("/p/b")], false)
    );
    for i in 0..=MAX_CHANGED_PATHS {
        sink.push(&PathBuf::from(format!("/p/{i}")));
    }
    let (paths, overflow) = sink.take();
    assert_eq!((paths.len(), overflow), (MAX_CHANGED_PATHS, true));
    assert_eq!(sink.take(), (Vec::new(), false));
}

#[test]
fn queues_at_most_one_message_per_kind() {
    let mut q = Queued::default();
    assert!(q.should_send(Change::Worktree, true));
    assert!(!q.should_send(Change::Worktree, false));
    assert!(q.should_send(Change::Git, false));
    assert!(!q.should_send(Change::Git, false));
    // The receiver drained the channel: both kinds may be sent again.
    assert!(q.should_send(Change::Git, true));
    assert!(q.should_send(Change::Worktree, false));
}

/// `npm install` creates `node_modules` (always skipped, and usually
/// gitignored): the folder itself is reported, its contents are not.
#[test]
fn reports_install_folders_but_not_their_contents() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();
    let (tx, rx) = async_channel::bounded(16);
    let sink: ChangedPaths = Default::default();
    let Some(_watcher) = RepoWatcher::start_with_paths(&root, &[], tx, Some(sink.clone())) else {
        return;
    };
    // The watches are added on a background thread: create the folder
    // again until its creation is seen.
    let node_modules = root.join("node_modules");
    let seen = (0..200).any(|i| {
        if i % 10 == 0 {
            std::fs::remove_dir_all(&node_modules).ok();
            std::fs::create_dir_all(node_modules.join("left-pad")).unwrap();
            std::fs::write(node_modules.join("left-pad/index.js"), "x").unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        rx.try_recv().is_ok()
    });
    assert!(seen, "no change reported");
    std::thread::sleep(std::time::Duration::from_millis(50));
    let (paths, _) = sink.lock().unwrap().take();
    assert!(paths.contains(&node_modules), "{paths:?}");
    assert!(
        paths
            .iter()
            .all(|p| !p.starts_with(&node_modules) || *p == node_modules)
    );
}

#[test]
fn reports_changes_in_a_watched_tree() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    let (tx, rx) = async_channel::bounded(16);
    let Some(_watcher) = RepoWatcher::start(&root, &[], tx) else {
        return;
    };
    // The folder watches are added on a background thread: keep writing
    // until one of the writes is seen.
    let got = (0..200).find_map(|i| {
        if i % 10 == 0 {
            std::fs::write(root.join("src/a.txt"), format!("x{i}")).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        rx.try_recv().ok()
    });
    assert_eq!(got, Some(Change::Worktree));
}
