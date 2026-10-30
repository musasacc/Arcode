//! [`GitStore`]: the repositories of the open project, the status of the
//! active one, and every git operation the UI triggers. All git work runs
//! on the background executor; the store emits events when results arrive.

use std::path::{Path, PathBuf};
use std::time::Duration;

use arcode_git::{FileStatus, GitResult, Head, Repo, StatusSummary};
use gpui::{AppContext, Context, EventEmitter, Task};

use super::watcher::{Change, RepoWatcher};

/// How deep below the project root repositories are searched.
const NESTED_REPO_DEPTH: usize = 3;
/// Quiet time after a file-system event before refreshing.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(250);

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
    busy: usize,
    refresh_task: Option<Task<()>>,
    refresh_again: bool,
    watcher: Option<RepoWatcher>,
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
            busy: 0,
            refresh_task: None,
            refresh_again: false,
            watcher: None,
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
        cx.emit(GitEvent::ReposChanged);
        cx.notify();
        let Some(root) = root else {
            self._discover_task = None;
            return;
        };
        let scan = cx.background_spawn({
            let root = root.clone();
            async move { arcode_git::discover_repos(&root, NESTED_REPO_DEPTH) }
        });
        self._discover_task = Some(cx.spawn(async move |this, cx| {
            let repos = scan.await;
            this.update(cx, |store, cx| {
                if store.root.as_ref() == Some(&root) {
                    store.set_repos(repos, cx);
                }
            })
            .ok();
        }));
    }

    /// Re-discovers repositories (after `git init`).
    pub fn rediscover(&mut self, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.set_project_root(root, cx);
    }

    fn set_repos(&mut self, repos: Vec<Repo>, cx: &mut Context<Self>) {
        self.repos = repos;
        self.active = 0;
        self.start_watching(cx);
        cx.emit(GitEvent::ReposChanged);
        cx.emit(GitEvent::IndexChanged);
        self.refresh(cx);
    }

    /// Makes repository `ix` the one shown in the source control view.
    pub fn set_active_repo(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.repos.len() && ix != self.active {
            self.active = ix;
            self.snapshot = GitSnapshot::default();
            cx.emit(GitEvent::ReposChanged);
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

    fn start_watching(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.repos.is_empty() {
            // Nothing to watch until a repository exists (`git init`
            // rediscovers and starts watching).
            return;
        }
        let (tx, rx) = async_channel::bounded::<Change>(4);
        self.watcher = RepoWatcher::start(&root, &self.repos, tx);
        self._watch_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(first) = rx.recv().await {
                cx.background_executor().timer(WATCH_DEBOUNCE).await;
                let mut git = first == Change::Git;
                while let Ok(kind) = rx.try_recv() {
                    git |= kind == Change::Git;
                }
                let alive = this.update(cx, |store, cx| {
                    if git {
                        cx.emit(GitEvent::IndexChanged);
                    }
                    store.refresh(cx);
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    /// Re-reads the status of the active repository in the background.
    /// Calls while a refresh runs are coalesced into one more run.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_task.is_some() {
            self.refresh_again = true;
            return;
        }
        let Some(repo) = self.active_repo().cloned() else {
            self.loaded = self.root.is_some();
            self.snapshot = GitSnapshot::default();
            cx.emit(GitEvent::StatusChanged);
            cx.notify();
            return;
        };
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
                let entries = entries.unwrap_or_else(|err| {
                    log::warn!("git status failed: {err}");
                    Vec::new()
                });
                let snapshot = GitSnapshot {
                    head: Some(head),
                    entries,
                    merging,
                };
                let first = !store.loaded;
                store.loaded = true;
                if snapshot != store.snapshot || first {
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

    /// Runs `op` on the active repository in the background. On success
    /// the status refreshes, editors are told the index changed and
    /// `then` runs with the result; failures become a message.
    pub fn run<T: Send + 'static>(
        &mut self,
        op: impl FnOnce(&Repo) -> GitResult<T> + Send + 'static,
        then: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.active_repo().cloned() else {
            cx.emit(GitEvent::Message("No git repository".into()));
            return;
        };
        self.run_in(repo, op, then, cx);
    }

    /// Like [`GitStore::run`] on a given repository.
    pub fn run_in<T: Send + 'static>(
        &mut self,
        repo: Repo,
        op: impl FnOnce(&Repo) -> GitResult<T> + Send + 'static,
        then: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy += 1;
        cx.notify();
        let work = cx.background_spawn(async move { op(&repo) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |store, cx| {
                store.busy -= 1;
                match result {
                    Ok(value) => then(store, value, cx),
                    Err(err) => cx.emit(GitEvent::Message(err.to_string())),
                }
                cx.emit(GitEvent::IndexChanged);
                store.refresh(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Runs `git init` in the project folder (only on an explicit click).
    pub fn init_repository(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.busy += 1;
        cx.notify();
        let work = cx.background_spawn(async move { arcode_git::init_repo(&root).map(drop) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |store, cx| {
                store.busy -= 1;
                match result {
                    Ok(()) => {
                        cx.emit(GitEvent::Message("Initialized a git repository".into()));
                        store.rediscover(cx);
                    }
                    Err(err) => cx.emit(GitEvent::Message(err.to_string())),
                }
            })
            .ok();
        })
        .detach();
    }
}
