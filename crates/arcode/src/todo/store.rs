//! [`TodoStore`]: the project's TODO comments, scanned once in full and
//! then updated per file: after a save in Arcode and for files the watcher
//! reports as changed on disk, only those files are scanned again
//! (`arcode_search::scan_todos_in_paths`).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use arcode_search::{CancelToken, TodoItem, TodoOptions, scan_todos_in_paths, scan_todos_with};
use arcode_ui_kit::settings;
use gpui::{AppContext, Context, Task};

/// Quiet time before changed files are scanned again.
const UPDATE_DEBOUNCE: Duration = Duration::from_millis(300);
/// Above this many changed paths a full rescan is cheaper.
const MAX_INCREMENTAL: usize = 500;

/// The TODO comments of the project.
#[derive(Default)]
pub struct TodoStore {
    root: Option<PathBuf>,
    tags: Vec<String>,
    /// Items per absolute file path, each sorted by line.
    files: BTreeMap<PathBuf, Vec<TodoItem>>,
    scanning: bool,
    loaded: bool,
    full_scans: usize,
    cancel: Option<CancelToken>,
    scan_task: Option<Task<()>>,
    pending: HashSet<PathBuf>,
    update_task: Option<Task<()>>,
}

impl TodoStore {
    /// An empty store.
    pub fn new(_: &mut Context<Self>) -> Self {
        Self::default()
    }

    /// The tags searched for.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Items grouped by file, files in path order.
    pub fn files(&self) -> &BTreeMap<PathBuf, Vec<TodoItem>> {
        &self.files
    }

    /// Number of items.
    pub fn count(&self) -> usize {
        self.files.values().map(Vec::len).sum()
    }

    /// A full scan is running.
    pub fn is_scanning(&self) -> bool {
        self.scanning
    }

    /// The first scan finished.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// How many full scans ran (tests check updates stay incremental).
    pub fn full_scans(&self) -> usize {
        self.full_scans
    }

    /// The project root.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Opens `root` (scanning it) or clears the list.
    pub fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.root = root;
        self.files.clear();
        self.loaded = false;
        self.pending.clear();
        self.update_task = None;
        self.rescan(cx);
    }

    fn options(&self) -> TodoOptions {
        TodoOptions {
            tags: self.tags.clone(),
            exclude_globs: crate::project::skipped_dir_globs(),
            ..TodoOptions::default()
        }
    }

    /// Scans the whole project again (cancelling a running scan).
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.cancel.take() {
            token.cancel();
        }
        self.tags = settings(cx).todo.tags.clone();
        let Some(root) = self.root.clone() else {
            self.scanning = false;
            self.scan_task = None;
            cx.notify();
            return;
        };
        let token = CancelToken::new();
        self.cancel = Some(token.clone());
        self.scanning = true;
        self.full_scans += 1;
        let opts = self.options();
        let scan = cx.background_spawn(async move { scan_todos_with(&root, &opts, &token) });
        self.scan_task = Some(cx.spawn(async move |this, cx| {
            let items = scan.await.unwrap_or_else(|err| {
                log::debug!("todo scan: {err}");
                Vec::new()
            });
            this.update(cx, |store, cx| {
                store.files = group(items);
                store.scanning = false;
                store.loaded = true;
                store.cancel = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Rescans when the configured tags changed (after a settings reload).
    pub fn settings_changed(&mut self, cx: &mut Context<Self>) {
        if self.root.is_some() && settings(cx).todo.tags != self.tags {
            self.rescan(cx);
        }
    }

    /// Schedules a rescan of `paths` (files or folders under the root,
    /// existing or deleted); `overflow` means "too many to list".
    pub fn paths_changed(&mut self, paths: &[PathBuf], overflow: bool, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if overflow {
            self.rescan(cx);
            return;
        }
        self.pending
            .extend(paths.iter().filter(|p| p.starts_with(&root)).cloned());
        if self.pending.is_empty() || self.update_task.is_some() {
            return;
        }
        self.update_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UPDATE_DEBOUNCE).await;
            this.update(cx, |store, cx| store.flush(cx)).ok();
        }));
    }

    /// Scans the pending paths now.
    fn flush(&mut self, cx: &mut Context<Self>) {
        self.update_task = None;
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.pending.len() > MAX_INCREMENTAL {
            self.pending.clear();
            self.rescan(cx);
            return;
        }
        // A folder (created, moved or deleted) stands for every file in it;
        // a deleted path is neither file nor folder any more, so it is
        // treated as both.
        let changed: Vec<PathBuf> = self.pending.drain().collect();
        let files: Vec<PathBuf> = changed.iter().filter(|p| !p.is_dir()).cloned().collect();
        let dirs: Vec<PathBuf> = changed.iter().filter(|p| !p.is_file()).cloned().collect();
        let opts = self.options();
        let work = cx.background_spawn(async move {
            scan_todos_in_paths(&root, &changed, &opts, &CancelToken::new()).unwrap_or_default()
        });
        self.update_task = Some(cx.spawn(async move |this, cx| {
            let items = work.await;
            this.update(cx, |store, cx| {
                store.update_task = None;
                store.replace(&files, &dirs, items);
                cx.notify();
                if !store.pending.is_empty() {
                    store.paths_changed(&[], false, cx);
                }
            })
            .ok();
        }));
    }

    /// Replaces the items of `files` and of everything under `dirs` with
    /// `items` (the fresh scan of exactly those).
    fn replace(&mut self, files: &[PathBuf], dirs: &[PathBuf], items: Vec<TodoItem>) {
        for file in files {
            self.files.remove(file);
        }
        if !dirs.is_empty() {
            self.files
                .retain(|path, _| !dirs.iter().any(|d| path.starts_with(d)));
        }
        let root = self.root.clone();
        for (path, mut list) in group(items) {
            if let Some(root) = &root {
                for item in &mut list {
                    item.relative = item.path.strip_prefix(root).unwrap_or(&item.path).into();
                }
            }
            self.files.insert(path, list);
        }
    }
}

/// Groups items by file, each file's items sorted by line.
fn group(items: Vec<TodoItem>) -> BTreeMap<PathBuf, Vec<TodoItem>> {
    let mut files: BTreeMap<PathBuf, Vec<TodoItem>> = BTreeMap::new();
    for item in items {
        files.entry(item.path.clone()).or_default().push(item);
    }
    for list in files.values_mut() {
        list.sort_by_key(|i| (i.line, i.column));
    }
    files
}
