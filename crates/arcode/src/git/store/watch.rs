//! Following file-system changes: refresh the status, tell editors the
//! index moved, and look for repositories again when a `.git` appears.

use std::time::Duration;

use gpui::Context;

use super::{GitEvent, GitStore};
use crate::git::watcher::{Change, RepoWatcher};

/// Quiet time after a file-system event before refreshing.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(250);
/// Channel room: the watcher queues at most one message per kind.
const CHANNEL_CAPACITY: usize = 8;

impl GitStore {
    /// (Re)starts the watcher for the current repositories. Without any, the
    /// top folders are still watched so a `git init` or `git clone` made
    /// outside Arcode is picked up.
    pub(super) fn start_watching(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let (tx, rx) = async_channel::bounded::<Change>(CHANNEL_CAPACITY);
        self.watcher =
            RepoWatcher::start_with_paths(&root, &self.repos, tx, Some(self.changed.clone()));
        self._watch_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(first) = rx.recv().await {
                cx.background_executor().timer(WATCH_DEBOUNCE).await;
                let mut kinds = vec![first];
                while let Ok(kind) = rx.try_recv() {
                    kinds.push(kind);
                }
                let alive = this.update(cx, |store, cx| store.on_changes(&kinds, cx));
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    fn on_changes(&mut self, kinds: &[Change], cx: &mut Context<Self>) {
        let taken = self.changed.lock().map(|mut sink| sink.take()).ok();
        if let Some((paths, overflow)) = taken
            && (overflow || !paths.is_empty())
        {
            cx.emit(GitEvent::FilesChanged { paths, overflow });
        }
        if kinds.contains(&Change::Repo) {
            self.rediscover(cx);
        }
        if self.repos.is_empty() {
            return;
        }
        if kinds.contains(&Change::Git) {
            cx.emit(GitEvent::IndexChanged);
        }
        self.refresh(cx);
    }
}
