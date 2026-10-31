//! Following changes made outside Arcode: the root and every expanded
//! folder get a non-recursive OS watch (inotify, FSEvents,
//! ReadDirectoryChangesW). Nothing is polled; collapsed folders cost
//! nothing. Events are coalesced for 150 ms, then only the folders that
//! changed are read again, on the background executor.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{AppContext, Context, Task};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::FileTree;
use crate::project::read_dir_sorted;

/// Quiet time after an event before folders are read again.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// The OS watcher and the folders it watches.
pub(crate) struct TreeWatch {
    watcher: RecommendedWatcher,
    watched: HashSet<PathBuf>,
    _task: Task<()>,
}

impl TreeWatch {
    /// Starts the watcher; `None` when the platform refuses (the tree then
    /// only refreshes on demand).
    pub(crate) fn start(cx: &mut Context<FileTree>) -> Option<Self> {
        let pending: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
        let (tx, rx) = async_channel::bounded::<()>(1);
        let sink = pending.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            if let Ok(mut dirs) = sink.lock() {
                for path in &event.paths {
                    // A changed entry changes its folder's listing; a
                    // removed watched folder is checked itself.
                    if let Some(parent) = path.parent() {
                        dirs.insert(parent.to_path_buf());
                    }
                    dirs.insert(path.clone());
                }
            }
            tx.try_send(()).ok();
        });
        let watcher = match watcher {
            Ok(watcher) => watcher,
            Err(err) => {
                log::warn!("file tree: cannot watch files: {err}");
                return None;
            }
        };
        let task = cx.spawn(async move |this, cx| {
            while rx.recv().await.is_ok() {
                cx.background_executor().timer(DEBOUNCE).await;
                let dirs: Vec<PathBuf> = pending
                    .lock()
                    .map(|mut d| d.drain().collect())
                    .unwrap_or_default();
                let alive = this.update(cx, |tree, cx| tree.fs_changed(dirs, cx));
                if alive.is_err() {
                    break;
                }
            }
        });
        Some(Self {
            watcher,
            watched: HashSet::new(),
            _task: task,
        })
    }
}

impl FileTree {
    /// Makes the OS watches match the root plus the expanded folders.
    pub(crate) fn sync_watches(&mut self) {
        let Some(watch) = &mut self.watch else { return };
        let wanted: HashSet<PathBuf> = std::iter::once(self.root.clone())
            .chain(self.expanded.iter().cloned())
            .collect();
        for gone in watch
            .watched
            .difference(&wanted)
            .cloned()
            .collect::<Vec<_>>()
        {
            watch.watcher.unwatch(&gone).ok();
            watch.watched.remove(&gone);
        }
        for dir in wanted {
            if !watch.watched.contains(&dir)
                && watch
                    .watcher
                    .watch(&dir, RecursiveMode::NonRecursive)
                    .is_ok()
            {
                watch.watched.insert(dir);
            }
        }
    }

    /// Drops the watches on `path` and the folders inside it before it is
    /// renamed, moved or deleted: on Windows an open watch handle can make
    /// that fail. [`FileTree::sync_watches`] restores what is still wanted.
    pub(crate) fn release_watches_under(&mut self, path: &Path) {
        let Some(watch) = &mut self.watch else { return };
        let inside: Vec<PathBuf> = watch
            .watched
            .iter()
            .filter(|d| d.starts_with(path))
            .cloned()
            .collect();
        for dir in inside {
            watch.watcher.unwatch(&dir).ok();
            watch.watched.remove(&dir);
        }
    }

    /// Number of folders watched (tests).
    pub fn watched_count(&self) -> usize {
        self.watch.as_ref().map_or(0, |w| w.watched.len())
    }

    /// Reads the listed folders again (those shown in the tree) in the
    /// background and redraws only if a listing changed.
    pub(crate) fn fs_changed(&mut self, dirs: Vec<PathBuf>, cx: &mut Context<Self>) {
        let shown: Vec<PathBuf> = dirs
            .into_iter()
            .filter(|d| self.children.contains_key(d))
            .collect();
        if shown.is_empty() {
            return;
        }
        let read = cx.background_spawn(async move {
            shown
                .into_iter()
                .map(|dir| {
                    let listing = dir.is_dir().then(|| read_dir_sorted(&dir));
                    (dir, listing)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let listings = read.await;
            this.update(cx, |tree, cx| tree.apply_listings(listings, cx))
                .ok();
        })
        .detach();
    }

    /// Replaces cached listings; `None` means the folder is gone.
    pub(crate) fn apply_listings(
        &mut self,
        listings: Vec<(PathBuf, Option<Vec<crate::project::DirEntry>>)>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for (dir, listing) in listings {
            match listing {
                Some(entries) => {
                    if self.children.get(&dir) != Some(&entries) {
                        self.children.insert(dir, entries);
                        changed = true;
                    }
                }
                None => {
                    changed |= self.children.remove(&dir).is_some();
                    changed |= self.expanded.remove(&dir);
                }
            }
        }
        if !changed {
            return;
        }
        // Drop state of folders that vanished with their parent.
        self.expanded.retain(|d| d.is_dir());
        self.rebuild();
        self.sync_watches();
        if self.selected.as_ref().is_some_and(|s| !s.exists()) {
            self.selected = None;
        }
        cx.notify();
    }
}
