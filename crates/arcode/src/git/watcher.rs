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

use super::ignore_filter::IgnoreFilter;
use crate::project::is_skipped_dir;

/// Upper bound on per-folder watches (Linux).
const MAX_DIR_WATCHES: usize = 8000;
/// Folder depth watched when the project has no repository yet: enough to
/// notice a `git init` or `git clone` where repository discovery looks.
const NO_REPO_DEPTH: usize = 3;

/// What changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Change {
    /// The index, `HEAD`, a ref or the merge state of a repository.
    Git,
    /// A file in a working tree.
    Worktree,
    /// A `.git` appeared or vanished outside the known repositories (`git
    /// init` or `git clone` in a terminal): the repository list may differ.
    Repo,
}

/// Most changed paths remembered between two batches; beyond it the
/// receiver is told to treat everything as changed.
const MAX_CHANGED_PATHS: usize = 1000;

/// Working-tree paths reported since the receiver last looked, for features
/// that update per file (the TODO list).
#[derive(Debug, Default)]
pub struct PathSink {
    paths: std::collections::HashSet<PathBuf>,
    overflow: bool,
}

impl PathSink {
    /// Remembers `path` (or notes an overflow past the cap).
    pub fn push(&mut self, path: &Path) {
        if self.paths.len() >= MAX_CHANGED_PATHS {
            self.overflow = true;
        } else {
            self.paths.insert(path.to_path_buf());
        }
    }

    /// Takes the paths seen so far and whether some were dropped.
    pub fn take(&mut self) -> (Vec<PathBuf>, bool) {
        let mut paths: Vec<PathBuf> = self.paths.drain().collect();
        paths.sort();
        (paths, std::mem::take(&mut self.overflow))
    }
}

/// A [`PathSink`] shared between the watcher thread and the UI.
pub type ChangedPaths = Arc<Mutex<PathSink>>;

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
    if path.file_name().is_some_and(|n| n == ".git") {
        return Some(Change::Repo);
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
    ///
    /// Without repositories only the top few folder levels are watched on
    /// Linux, just enough to report [`Change::Repo`] for a new repository.
    /// The folder walk runs on a background thread, so this returns at once.
    pub fn start(root: &Path, repos: &[Repo], tx: async_channel::Sender<Change>) -> Option<Self> {
        Self::start_with_paths(root, repos, tx, None)
    }

    /// Like [`RepoWatcher::start`], also recording every changed
    /// working-tree path in `paths`.
    pub fn start_with_paths(
        root: &Path,
        repos: &[Repo],
        tx: async_channel::Sender<Change>,
        paths: Option<ChangedPaths>,
    ) -> Option<Self> {
        let git_dirs: Vec<PathBuf> = repos
            .iter()
            .flat_map(|r| [r.git_dir.clone(), r.common_dir.clone()])
            .collect();
        let (dir_tx, dir_rx) = std::sync::mpsc::channel::<PathBuf>();
        let dirs = git_dirs.clone();
        let mut queued = Queued::default();
        let walk_tx = dir_tx.clone();
        let repo_tx = tx.clone();
        let depth = repos.is_empty().then_some(NO_REPO_DEPTH);
        // Gitignored paths stay out of `paths`; the rules are read on the
        // watcher thread at the first event, not on the caller's thread.
        let ignore_root = root.to_path_buf();
        let ignore_repos = IgnoreFilter::repo_dirs(repos);
        let mut ignored: Option<IgnoreFilter> = None;
        let handler = move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let mut kinds = Vec::new();
            for path in &event.paths {
                if let Some(kind) = classify(path, &dirs) {
                    if kind == Change::Worktree
                        && let Some(sink) = &paths
                        && !ignored
                            .get_or_insert_with(|| IgnoreFilter::new(&ignore_root, &ignore_repos))
                            .is_ignored(path)
                        && let Ok(mut sink) = sink.lock()
                    {
                        sink.push(path);
                    }
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
                if queued.should_send(kind, tx.is_empty()) {
                    tx.try_send(kind).ok();
                }
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
                // The walk can take a while on big projects: it runs on the
                // folder-adding thread, never on the caller's (main) thread.
                walk_tx.send(root.to_path_buf()).ok();
                drop(walk_tx);
            } else if let Err(err) = w.watch(root, RecursiveMode::Recursive) {
                log::warn!("git: cannot watch {}: {err}", root.display());
            }
        }
        spawn_dir_adder(Arc::downgrade(&watcher), dir_rx, depth, repo_tx);
        Some(Self { _watcher: watcher })
    }
}

/// Which kinds were sent since the channel was last seen empty. At most one
/// message per kind is queued, so a burst of working-tree events can never
/// fill the channel and push out an index/`HEAD` change.
#[derive(Default)]
struct Queued {
    git: bool,
    worktree: bool,
    repo: bool,
}

impl Queued {
    /// Whether to send `kind`; `empty` is whether the channel is empty now
    /// (everything sent before has been taken by the receiver).
    ///
    /// Skipping is safe: a skipped kind is either still queued, or was taken
    /// by a receiver that has not yet finished debouncing its batch (which
    /// happens after this event), and any queued message triggers a refresh.
    fn should_send(&mut self, kind: Change, empty: bool) -> bool {
        if empty {
            *self = Self::default();
        }
        let flag = match kind {
            Change::Git => &mut self.git,
            Change::Worktree => &mut self.worktree,
            Change::Repo => &mut self.repo,
        };
        !std::mem::replace(flag, true)
    }
}

/// Adds a non-recursive watch for every non-ignored folder under `root`
/// (down to `depth` levels when given).
fn watch_tree(watcher: &mut RecommendedWatcher, root: &Path, depth: Option<usize>) {
    let mut count = 0;
    let walk = crate::project::walker(root).max_depth(depth).build();
    for entry in walk.flatten() {
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

/// Watches the project tree and folders created after start (Linux). Runs
/// on its own thread because the walk can be slow and the watcher cannot be
/// changed from inside its event handler; the thread ends when the watcher
/// (and with it the sender) is dropped.
///
/// A created folder that already holds a `.git` (a clone written before
/// its watch existed) is reported as [`Change::Repo`].
fn spawn_dir_adder(
    watcher: Weak<Mutex<RecommendedWatcher>>,
    rx: std::sync::mpsc::Receiver<PathBuf>,
    depth: Option<usize>,
    tx: async_channel::Sender<Change>,
) {
    std::thread::Builder::new()
        .name("arcode-git-watch".into())
        .spawn(move || {
            let mut first = true;
            while let Ok(dir) = rx.recv() {
                let Some(watcher) = watcher.upgrade() else {
                    break;
                };
                if let Ok(mut w) = watcher.lock() {
                    watch_tree(&mut w, &dir, depth);
                }
                if !std::mem::take(&mut first) && dir.join(".git").exists() {
                    tx.try_send(Change::Repo).ok();
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
        assert_eq!(
            classify(Path::new("/p/sub/.git"), &dirs),
            Some(Change::Repo)
        );
        assert_eq!(classify(&git, &dirs), None, "the known repository itself");
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
}
