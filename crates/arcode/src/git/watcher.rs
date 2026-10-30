//! File-system watching for the git integration.
//!
//! Reports two kinds of change over a channel: something inside a git
//! directory that matters (index, `HEAD`, refs, merge state) or a file in a
//! working tree. Events are not debounced here; the receiver coalesces
//! them. Nothing is polled: the OS notifies us (inotify, FSEvents,
//! ReadDirectoryChangesW).
//!
//! On Linux every non-ignored folder gets its own non-recursive watch
//! (inotify watches cost kernel memory per folder, so ignored build output
//! like `target/` is left out); new folders are added as they appear.
//! Elsewhere the project root is watched recursively, which is cheap there.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use arcode_git::Repo;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::project::is_skipped_dir;

/// Upper bound on per-folder watches (Linux).
const MAX_DIR_WATCHES: usize = 8000;

/// What changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Change {
    /// The index, `HEAD`, a ref or the merge state of a repository.
    Git,
    /// A file in a working tree.
    Worktree,
}

/// Keeps the OS watches alive; dropping it stops watching.
pub struct RepoWatcher {
    _watcher: Arc<Mutex<RecommendedWatcher>>,
}

/// Classifies `path`: `None` for changes nobody cares about (objects,
/// logs, lock files, ignored build folders).
pub fn classify(path: &Path, git_dirs: &[PathBuf]) -> Option<Change> {
    if let Some(dir) = git_dirs.iter().find(|d| path.starts_with(d)) {
        let rel = path.strip_prefix(dir).ok()?;
        let first = rel.components().next()?.as_os_str().to_str()?;
        let name = path.file_name()?.to_str()?;
        if name.ends_with(".lock") {
            return None;
        }
        return matches!(
            first,
            "index" | "HEAD" | "MERGE_HEAD" | "packed-refs" | "refs" | "rebase-merge"
        )
        .then_some(Change::Git);
    }
    let skipped = path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| s == ".git" || is_skipped_dir(s))
    });
    (!skipped).then_some(Change::Worktree)
}

impl RepoWatcher {
    /// Watches `root` (the project folder) and the git directories of
    /// `repos`, sending a [`Change`] for every relevant event. Returns
    /// `None` when the platform watcher cannot be created.
    pub fn start(root: &Path, repos: &[Repo], tx: async_channel::Sender<Change>) -> Option<Self> {
        let git_dirs: Vec<PathBuf> = repos
            .iter()
            .flat_map(|r| [r.git_dir.clone(), r.common_dir.clone()])
            .collect();
        let (dir_tx, dir_rx) = std::sync::mpsc::channel::<PathBuf>();
        let dirs = git_dirs.clone();
        let handler = move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let mut kinds = Vec::new();
            for path in &event.paths {
                if let Some(kind) = classify(path, &dirs) {
                    if !kinds.contains(&kind) {
                        kinds.push(kind);
                    }
                    if cfg!(target_os = "linux")
                        && kind == Change::Worktree
                        && matches!(event.kind, EventKind::Create(_))
                        && path.is_dir()
                    {
                        dir_tx.send(path.clone()).ok();
                    }
                }
            }
            for kind in kinds {
                // A full channel already holds a pending refresh.
                tx.try_send(kind).ok();
            }
        };
        let watcher = match notify::recommended_watcher(handler) {
            Ok(w) => Arc::new(Mutex::new(w)),
            Err(err) => {
                log::warn!("git: cannot watch files: {err}");
                return None;
            }
        };
        {
            let mut w = watcher.lock().ok()?;
            for repo in repos {
                for dir in [&repo.git_dir, &repo.common_dir] {
                    w.watch(dir, RecursiveMode::NonRecursive).ok();
                    w.watch(&dir.join("refs"), RecursiveMode::Recursive).ok();
                }
            }
            if cfg!(target_os = "linux") {
                watch_tree(&mut *w, root);
            } else if let Err(err) = w.watch(root, RecursiveMode::Recursive) {
                log::warn!("git: cannot watch {}: {err}", root.display());
            }
        }
        spawn_dir_adder(Arc::downgrade(&watcher), dir_rx);
        Some(Self { _watcher: watcher })
    }
}

/// Adds a non-recursive watch for every non-ignored folder under `root`.
fn watch_tree(watcher: &mut RecommendedWatcher, root: &Path) {
    let mut count = 0;
    for entry in crate::project::walker(root).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_dir()) {
            continue;
        }
        if watcher
            .watch(entry.path(), RecursiveMode::NonRecursive)
            .is_ok()
        {
            count += 1;
        }
        if count >= MAX_DIR_WATCHES {
            log::warn!("git: watching only the first {MAX_DIR_WATCHES} folders");
            break;
        }
    }
}

/// Watches folders created after start (Linux). Runs on its own thread
/// because the watcher cannot be changed from inside its event handler;
/// the thread ends when the watcher (and with it the sender) is dropped.
fn spawn_dir_adder(
    watcher: Weak<Mutex<RecommendedWatcher>>,
    rx: std::sync::mpsc::Receiver<PathBuf>,
) {
    std::thread::Builder::new()
        .name("arcode-git-watch".into())
        .spawn(move || {
            while let Ok(dir) = rx.recv() {
                let Some(watcher) = watcher.upgrade() else {
                    break;
                };
                if let Ok(mut w) = watcher.lock() {
                    watch_tree(&mut *w, &dir);
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
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
        std::fs::write(root.join("src/a.txt"), "x").unwrap();
        let got = std::iter::repeat_with(|| rx.try_recv())
            .take(200)
            .find_map(|r| {
                std::thread::sleep(std::time::Duration::from_millis(10));
                r.ok()
            });
        assert_eq!(got, Some(Change::Worktree));
    }
}
