//! Git wiring of the workspace: the store, editor decorations, the Source
//! Control view, the branch picker, diff tabs and the git actions.

use std::path::{Path, PathBuf};

use arcode_git::Repo;
use gpui::{AppContext, Context, Entity, Focusable, Subscription, Window};

use super::Workspace;
use crate::actions::*;
use crate::activity_bar::Activity;
use crate::git::branch_picker::{BranchPicker, BranchPickerEvent};
use crate::git::diff_view::diff_editor;
use crate::git::panel::{GitPanel, GitPanelEvent};
use crate::git::{GitEditors, GitEditorsEvent, GitEvent, GitStore};

/// The git entities of a workspace.
pub struct GitParts {
    /// Repositories and status.
    pub store: Entity<GitStore>,
    /// Editor decorations.
    pub editors: Entity<GitEditors>,
    /// The Source Control view.
    pub panel: Entity<GitPanel>,
}

impl GitParts {
    /// Creates the entities and the workspace's subscriptions to them.
    pub(super) fn new(
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> (Self, Vec<Subscription>) {
        let store = cx.new(GitStore::new);
        let editors = cx.new(|cx| GitEditors::new(store.clone(), cx));
        let panel = cx.new(|cx| GitPanel::new(store.clone(), cx));
        let subs = vec![
            cx.subscribe(&store, |ws, _, event: &GitEvent, cx| match event {
                GitEvent::Message(text) => ws.show_toast(text.clone(), cx),
                GitEvent::StatusChanged | GitEvent::ReposChanged => {
                    // The title bar shows the branch too.
                    ws.refresh_status(cx);
                    cx.notify();
                }
                GitEvent::IndexChanged => {}
            }),
            cx.subscribe_in(
                &editors,
                window,
                |ws, _, event: &GitEditorsEvent, window, cx| match event {
                    GitEditorsEvent::OpenDiff {
                        title, old, new, ..
                    } => ws.open_diff_tab(title.clone(), old, new, window, cx),
                    GitEditorsEvent::Message(text) => ws.show_toast(text.clone(), cx),
                    GitEditorsEvent::ConflictsChanged => ws.refresh_status(cx),
                },
            ),
            cx.subscribe_in(
                &panel,
                window,
                |ws, _, event: &GitPanelEvent, window, cx| match event {
                    GitPanelEvent::OpenFile(path) => ws.open_file(path, window, cx),
                    GitPanelEvent::OpenDiff { path, staged } => {
                        ws.open_file_diff(path.clone(), *staged, window, cx)
                    }
                    GitPanelEvent::PickBranch => ws.open_branch_picker(window, cx),
                },
            ),
        ];
        (
            Self {
                store,
                editors,
                panel,
            },
            subs,
        )
    }
}

impl Workspace {
    /// The git store.
    pub fn git(&self) -> &Entity<GitStore> {
        &self.git.store
    }

    /// The git editor decorations.
    pub fn git_editors(&self) -> &Entity<GitEditors> {
        &self.git.editors
    }

    /// The Source Control view.
    pub fn git_panel(&self) -> &Entity<GitPanel> {
        &self.git.panel
    }

    pub(super) fn register_git_actions<E: gpui::InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(cx.listener(|ws, _: &ShowSourceControl, window, cx| {
            ws.activity = Activity::Git;
            ws.sidebar_visible = true;
            ws.git.panel.read(cx).focus_message(window, cx);
            cx.notify();
        }))
        .on_action(cx.listener(|ws, _: &ToggleBlame, _, cx| {
            ws.git.editors.update(cx, |g, cx| g.toggle_blame_column(cx));
        }))
        .on_action(
            cx.listener(|ws, _: &SwitchBranch, window, cx| ws.open_branch_picker(window, cx)),
        )
        .on_action(cx.listener(|ws, _: &OpenChanges, window, cx| {
            let path = ws
                .active_editor(cx)
                .and_then(|e| e.read(cx).path().map(Path::to_path_buf));
            match path {
                Some(path) => ws.open_file_diff(path, false, window, cx),
                None => ws.show_toast("Open a file to see its changes", cx),
            }
        }))
        .on_action(cx.listener(|ws, _: &NextConflict, _, cx| ws.go_to_conflict(true, cx)))
        .on_action(cx.listener(|ws, _: &PreviousConflict, _, cx| ws.go_to_conflict(false, cx)))
        .on_action(cx.listener(|ws, _: &RefreshGit, _, cx| {
            ws.git.store.update(cx, |s, cx| s.refresh(cx));
        }))
        .on_action(cx.listener(|ws, _: &InitRepository, _, cx| {
            let has_repo = ws.git.store.read(cx).active_repo().is_some();
            if has_repo {
                ws.show_toast("This folder is already in a git repository", cx);
            } else {
                ws.git.store.update(cx, |s, cx| s.init_repository(cx));
            }
        }))
    }

    pub(super) fn go_to_conflict(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx) else {
            return;
        };
        let found = self
            .git
            .editors
            .update(cx, |g, cx| g.go_to_conflict(&editor, forward, cx));
        if !found {
            self.show_toast("No merge conflicts in this file", cx);
        }
    }

    /// Opens the branch picker for the active repository.
    pub fn open_branch_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.git.store.read(cx).active_repo().cloned() else {
            self.show_toast("No git repository", cx);
            return;
        };
        self.project_menu_open = false;
        let picker = cx.new(|cx| BranchPicker::new(repo, cx));
        let sub = cx.subscribe_in(
            &picker,
            window,
            |ws, _, event: &BranchPickerEvent, window, cx| {
                ws.close_branch_picker(window, cx);
                match event.clone() {
                    BranchPickerEvent::Switch(name) => ws.git.store.update(cx, |s, cx| {
                        let label = name.clone();
                        s.run(
                            move |repo| arcode_git::checkout_branch(repo, &name),
                            move |_, _, cx| {
                                cx.emit(GitEvent::Message(format!("Switched to {label}")))
                            },
                            cx,
                        )
                    }),
                    BranchPickerEvent::Create(name) => ws.git.store.update(cx, |s, cx| {
                        let label = name.clone();
                        s.run(
                            move |repo| arcode_git::create_branch(repo, &name),
                            move |_, _, cx| {
                                cx.emit(GitEvent::Message(format!("Created branch {label}")))
                            },
                            cx,
                        )
                    }),
                    BranchPickerEvent::Dismissed => {}
                }
            },
        );
        self._subscriptions.push(sub);
        window.focus(&picker.focus_handle(cx));
        self.branch_picker = Some(picker);
        cx.notify();
    }

    pub(super) fn close_branch_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.branch_picker.take().is_some() {
            self.active_pane().read(cx).focus(window, cx);
            cx.notify();
        }
    }

    /// Opens a read-only tab with `old` against `new`, replacing an open
    /// diff tab of the same title.
    pub fn open_diff_tab(
        &mut self,
        title: String,
        old: &str,
        new: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for pane in self.panes.clone() {
            if let Some(ix) = pane.read(cx).position_of_title(&title) {
                pane.update(cx, |p, cx| p.close(ix, window, cx));
            }
        }
        let editor = diff_editor(old, new, cx);
        self.active_pane().clone().update(cx, |pane, cx| {
            pane.add_titled_editor(editor, title.into(), window, cx)
        });
        cx.notify();
    }

    /// Opens the diff of `path` against `HEAD`: its staged version when
    /// `staged`, else the editor buffer (or the file on disk).
    pub fn open_file_diff(
        &mut self,
        path: PathBuf,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.git.store.read(cx).repo_for_path(&path) else {
            self.show_toast("This file is not in a git repository", cx);
            return;
        };
        let buffer = self
            .editor_for_path(&path, cx)
            .map(|e| e.read(cx).buffer().text().clone());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = if staged {
            format!("{name} (HEAD ↔ Staged)")
        } else {
            format!("{name} (HEAD ↔ Working Tree)")
        };
        let work =
            cx.background_spawn(async move { read_diff_sides(&repo, &path, staged, buffer) });
        cx.spawn_in(window, async move |this, cx| {
            let (old, new) = work.await;
            this.update_in(cx, |ws, window, cx| {
                ws.open_diff_tab(title, &old, &new, window, cx)
            })
            .ok();
        })
        .detach();
    }
}

/// The two texts of a file diff (runs off the main thread).
fn read_diff_sides(
    repo: &Repo,
    path: &Path,
    staged: bool,
    buffer: Option<arcode_buffer::Rope>,
) -> (String, String) {
    let old = repo.head_file_contents(path).unwrap_or_default();
    let new = if staged {
        repo.index_file_contents(path).unwrap_or_default()
    } else {
        match buffer {
            Some(rope) => rope.to_string(),
            None => std::fs::read_to_string(path).unwrap_or_default(),
        }
    };
    (old, new)
}
