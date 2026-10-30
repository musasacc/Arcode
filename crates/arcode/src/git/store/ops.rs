//! Running git operations for the UI on the background executor.

use arcode_git::{GitResult, Repo};
use gpui::{AppContext, Context};

use super::{GitEvent, GitStore};

impl GitStore {
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
        if !self.loaded || self.discovering {
            // Never create a repository before discovery has shown there is
            // none (it could end up nested inside an existing one).
            cx.emit(GitEvent::Message("Still looking for repositories…".into()));
            return;
        }
        if self.repos.iter().any(|r| root.starts_with(&r.workdir)) {
            cx.emit(GitEvent::Message(
                "This folder is already in a git repository".into(),
            ));
            return;
        }
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
