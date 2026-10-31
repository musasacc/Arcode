//! [`GitStore`]: the repositories of the open project, the status of the
//! active one, and every git operation the UI triggers. All git work runs
//! on the background executor; the store emits events when results arrive.

mod ops;
#[cfg(test)]
mod tests;
mod watch;

use std::path::{Path, PathBuf};

use arcode_git::{FileStatus, GitError, Head, Repo, StatusSummary};
use gpui::{AppContext, Context, EventEmitter, Task};

use super::watcher::RepoWatcher;

/// How deep below the project root repositories are searched.
const NESTED_REPO_DEPTH: usize = 3;

/// Events emitted by the [`GitStore`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitEvent {
    /// The repository list or the active repository changed.
    ReposChanged,
    /// Status entries or `HEAD` of the active repository changed.
    StatusChanged,
    /// The index or `HEAD` of some repository may have changed: editors
    /// recompute their gutter and blame.
    IndexChanged,
    /// A user-visible message (errors, confirmations).
    Message(String),
    /// Files in the working tree changed on disk (from the watcher, after
    /// the debounce). With `overflow` more changed than were listed.
    FilesChanged {
        /// Changed paths (files or folders; may no longer exist).
        paths: Vec<PathBuf>,
        /// Too many changes to list: treat everything as changed.
        overflow: bool,
    },
}

/// The status of the active repository.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitSnapshot {
    /// What `HEAD` points at.
    pub head: Option<Head>,
    /// Changed, untracked and conflicted paths.
    pub entries: Vec<FileStatus>,
    /// Whether a merge is in progress.
    pub merging: bool,
    /// Why the status could not be read (git missing, a broken
    /// repository); the entries are empty then and must not read as clean.
    pub error: Option<String>,
}

impl GitSnapshot {
    /// Counts for headers and badges.
    pub fn summary(&self) -> StatusSummary {
        StatusSummary::from_entries(&self.entries)
    }
}

/// Repositories and status for one workspace.
pub struct GitStore {
    root: Option<PathBuf>,
    repos: Vec<Repo>,
    active: usize,
    snapshot: GitSnapshot,
    loaded: bool,
    /// Repository discovery is running: "no repository" is not known yet.
    discovering: bool,
    busy: usize,
    refresh_task: Option<Task<()>>,
    refresh_again: bool,
    watcher: Option<RepoWatcher>,
    /// Changed working-tree paths collected by the watcher.
    changed: super::watcher::ChangedPaths,
    _watch_task: Option<Task<()>>,
    _discover_task: Option<Task<()>>,
}

impl EventEmitter<GitEvent> for GitStore {}

impl GitStore {
    /// An empty store (no project).
    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            root: None,
            repos: Vec::new(),
            active: 0,
            snapshot: GitSnapshot::default(),
            loaded: false,
            discovering: false,
            busy: 0,
            refresh_task: None,
            refresh_again: false,
            watcher: None,
            changed: Default::default(),
            _watch_task: None,
            _discover_task: None,
        }
    }

    /// The project folder.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Repositories of the project: the one containing the root first,
    /// then those in sub-folders.
    pub fn repos(&self) -> &[Repo] {
        &self.repos
    }

    /// The repository shown in the source control view.
    pub fn active_repo(&self) -> Option<&Repo> {
        self.repos.get(self.active)
    }

    /// Index of [`GitStore::active_repo`].
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Status of the active repository.
    pub fn snapshot(&self) -> &GitSnapshot {
        &self.snapshot
    }

    /// True once the first discovery and status run finished.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// True while an operation runs.
    pub fn is_busy(&self) -> bool {
        self.busy > 0
    }

    /// Current branch of the active repository for display.
    pub fn branch_label(&self) -> Option<String> {
        self.snapshot.head.as_ref().map(ToString::to_string)
    }

    /// The repository containing `path`: the innermost known one, else
    /// discovered from the file system.
    pub fn repo_for_path(&self, path: &Path) -> Option<Repo> {
        self.repos
            .iter()
            .filter(|r| path.starts_with(&r.workdir))
            .max_by_key(|r| r.workdir.as_os_str().len())
            .cloned()
            .or_else(|| arcode_git::find_repo(path))
    }

    /// The status entry of an absolute `path` in the active repository.
    pub fn status_of(&self, path: &Path) -> Option<&FileStatus> {
        let repo = self.active_repo()?;
        let rel = path.strip_prefix(&repo.workdir).ok()?;
        self.snapshot.entries.iter().find(|e| e.path == rel)
    }

    /// Opens `root` as the project (or clears with `None`): finds its
    /// repositories in the background and starts watching them.
    pub fn set_project_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.root = root.clone();
        self.repos.clear();
        self.active = 0;
        self.snapshot = GitSnapshot::default();
        self.loaded = false;
        self.watcher = None;
        self._watch_task = None;
        self.refresh_task = None;
        self.refresh_again = false;
        cx.emit(GitEvent::ReposChanged);
        cx.notify();
        if root.is_none() {
            self.discovering = false;
            self._discover_task = None;
            return;
        }
        self.rediscover(cx);
    }

    /// Finds the repositories of the project again in the background (after
    /// `git init`, or a repository appearing on disk). Nothing changes when
    /// the list is the same.
    pub fn rediscover(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.discovering = true;
        let scan = cx.background_spawn({
            let root = root.clone();
            async move { arcode_git::discover_repos(&root, NESTED_REPO_DEPTH) }
        });
        self._discover_task = Some(cx.spawn(async move |this, cx| {
            let repos = scan.await;
            this.update(cx, |store, cx| {
                if store.root.as_ref() != Some(&root) {
                    return;
                }
                store.discovering = false;
                let same = store.loaded
                    && repos.len() == store.repos.len()
                    && repos
                        .iter()
                        .zip(&store.repos)
                        .all(|(a, b)| a.workdir == b.workdir);
                if !same {
                    store.set_repos(repos, cx);
                }
            })
            .ok();
        }));
    }

    fn set_repos(&mut self, repos: Vec<Repo>, cx: &mut Context<Self>) {
        // Keep the active repository when it is still there.
        let previous = self.active_repo().map(|r| r.workdir.clone());
        self.repos = repos;
        let active = previous
            .and_then(|dir| self.repos.iter().position(|r| r.workdir == dir))
            .unwrap_or(0);
        if active != self.active || self.snapshot.head.is_none() {
            self.snapshot = GitSnapshot::default();
        }
        self.active = active;
        // A running refresh may belong to a repository that is gone.
        self.refresh_task = None;
        self.refresh_again = false;
        self.start_watching(cx);
        cx.emit(GitEvent::ReposChanged);
        cx.emit(GitEvent::IndexChanged);
        if self.repos.is_empty() {
            // Known now: no repository (the view offers `git init`).
            self.loaded = true;
            self.snapshot = GitSnapshot::default();
            cx.emit(GitEvent::StatusChanged);
            cx.notify();
        } else {
            self.refresh(cx);
        }
    }

    /// Makes repository `ix` the one shown in the source control view.
    pub fn set_active_repo(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.repos.len() && ix != self.active {
            self.active = ix;
            self.snapshot = GitSnapshot::default();
            // Drop a refresh of the previous repository: its result must
            // not be shown as this one's status.
            self.refresh_task = None;
            self.refresh_again = false;
            cx.emit(GitEvent::ReposChanged);
            cx.emit(GitEvent::StatusChanged);
            cx.notify();
            self.refresh(cx);
        }
    }

    /// Activates the repository containing `path`, if it is a different
    /// known one (the view follows the active editor).
    pub fn follow_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        let best = self
            .repos
            .iter()
            .enumerate()
            .filter(|(_, r)| path.starts_with(&r.workdir))
            .max_by_key(|(_, r)| r.workdir.as_os_str().len())
            .map(|(ix, _)| ix);
        if let Some(ix) = best {
            self.set_active_repo(ix, cx);
        }
    }

    /// What Refresh does: re-reads the status, or looks for repositories
    /// again when there is none (one may have been created outside).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.repos.is_empty() {
            if !self.discovering {
                self.rediscover(cx);
            }
        } else {
            self.refresh(cx);
        }
    }

    /// Re-reads the status of the active repository in the background.
    /// Calls while a refresh runs are coalesced into one more run.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_task.is_some() {
            self.refresh_again = true;
            return;
        }
        let Some(repo) = self.active_repo().cloned() else {
            if self.discovering {
                // Not known yet whether there is a repository: the view
                // keeps "Loading…" and never offers `git init` too early.
                return;
            }
            // Discovery decided "no repository" (`set_repos`); a new one is
            // found by the watcher or by `reload`.
            return;
        };
        let workdir = repo.workdir.clone();
        let work = cx.background_spawn(async move {
            let head = repo.head();
            let merging = repo.git_dir.join("MERGE_HEAD").is_file();
            let entries = arcode_git::status(&repo);
            (head, merging, entries)
        });
        self.refresh_task = Some(cx.spawn(async move |this, cx| {
            let (head, merging, entries) = work.await;
            this.update(cx, |store, cx| {
                store.refresh_task = None;
                if store.active_repo().map(|r| &r.workdir) != Some(&workdir) {
                    // The active repository changed meanwhile.
                    store.refresh(cx);
                    return;
                }
                let (entries, error) = match entries {
                    Ok(entries) => (entries, None),
                    Err(err) => {
                        log::warn!("git status failed: {err}");
                        (Vec::new(), Some(status_error(&err)))
                    }
                };
                let new_error = error.is_some() && error != store.snapshot.error;
                let snapshot = GitSnapshot {
                    head: Some(head),
                    entries,
                    merging,
                    error,
                };
                let first = !store.loaded;
                store.loaded = true;
                if snapshot != store.snapshot || first {
                    if new_error && let Some(err) = &snapshot.error {
                        cx.emit(GitEvent::Message(err.clone()));
                    }
                    store.snapshot = snapshot;
                    cx.emit(GitEvent::StatusChanged);
                    cx.notify();
                }
                if std::mem::take(&mut store.refresh_again) {
                    store.refresh(cx);
                }
            })
            .ok();
        }));
    }
}

/// The user-facing text for a failed status read.
fn status_error(err: &GitError) -> String {
    match err {
        GitError::GitNotFound => {
            "Git was not found. Install git (or add it to PATH) to see changes.".into()
        }
        other => format!("Cannot read the repository status: {other}"),
    }
}
