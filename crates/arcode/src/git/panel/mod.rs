//! The Source Control view in the sidebar: branch, commit box, and the
//! changed files grouped Merge Conflicts / Staged / Changes / Untracked
//! with stage, unstage and discard actions. Clicking a file opens its diff.

mod commit;
mod empty;
#[cfg(test)]
mod review_tests;
mod rows;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use arcode_ui_kit::{IconName, icon_button, theme};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, PromptLevel, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use super::store::{GitEvent, GitStore};
use crate::text_field::TextField;
pub use rows::{Group, Row, build_rows, status_letter};

/// Key context of the view.
pub const CONTEXT: &str = "GitPanel";

/// Events for the workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitPanelEvent {
    /// Open a diff of an absolute `path`: the index against `HEAD` when
    /// `staged`, else the working tree against `HEAD`.
    OpenDiff {
        /// The file.
        path: PathBuf,
        /// Which change to show.
        staged: bool,
    },
    /// Open the file in an editor.
    OpenFile(PathBuf),
    /// Open the branch picker.
    PickBranch,
}

/// The source control view.
pub struct GitPanel {
    store: Entity<GitStore>,
    message: Entity<TextField>,
    amend: bool,
    /// A commit is running; further commit requests are ignored.
    committing: bool,
    collapsed: HashSet<Group>,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    _subs: Vec<Subscription>,
}

impl EventEmitter<GitPanelEvent> for GitPanel {}

impl Focusable for GitPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl GitPanel {
    /// A view over `store`.
    pub fn new(store: Entity<GitStore>, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| TextField::multiline(commit::placeholder(), 2, 8, cx));
        let subs = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe(&store, |_, _, event: &GitEvent, cx| {
                if *event == GitEvent::StatusChanged {
                    cx.notify();
                }
            }),
            cx.observe(&message, |_, _, cx| cx.notify()),
        ];
        Self {
            store,
            message,
            amend: false,
            committing: false,
            collapsed: HashSet::new(),
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _subs: subs,
        }
    }

    /// The commit message field.
    pub fn message(&self) -> &Entity<TextField> {
        &self.message
    }

    /// Focuses the commit message.
    pub fn focus_message(&self, window: &mut Window, cx: &App) {
        window.focus(&self.message.focus_handle(cx));
    }

    fn workdir(&self, cx: &App) -> Option<PathBuf> {
        self.store.read(cx).active_repo().map(|r| r.workdir.clone())
    }

    fn toggle_group(&mut self, group: Group, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&group) {
            self.collapsed.insert(group);
        }
        cx.notify();
    }

    fn open_file(&mut self, rel: &Path, cx: &mut Context<Self>) {
        if let Some(dir) = self.workdir(cx) {
            cx.emit(GitPanelEvent::OpenFile(dir.join(rel)));
        }
    }

    fn open_diff(&mut self, rel: &Path, group: Group, cx: &mut Context<Self>) {
        let Some(dir) = self.workdir(cx) else { return };
        let path = dir.join(rel);
        match group {
            Group::Conflicts => cx.emit(GitPanelEvent::OpenFile(path)),
            Group::Staged => cx.emit(GitPanelEvent::OpenDiff { path, staged: true }),
            Group::Changes | Group::Untracked => cx.emit(GitPanelEvent::OpenDiff {
                path,
                staged: false,
            }),
        }
    }

    /// Stages one file (also marks a conflicted file resolved).
    pub fn stage_file(&mut self, rel: &Path, cx: &mut Context<Self>) {
        let rel = rel.to_path_buf();
        self.store.update(cx, |s, cx| {
            s.run(
                move |repo| arcode_git::stage_file(repo, &rel),
                |_, _, _| {},
                cx,
            )
        });
    }

    /// Unstages one file.
    pub fn unstage_file(&mut self, rel: &Path, cx: &mut Context<Self>) {
        let rel = rel.to_path_buf();
        self.store.update(cx, |s, cx| {
            s.run(
                move |repo| arcode_git::unstage_file(repo, &rel),
                |_, _, _| {},
                cx,
            )
        });
    }

    /// Stages exactly the files listed in `group` ("Stage all" on its
    /// header). Never `git add -A` / `-u`: those would also stage the other
    /// groups and mark unresolved conflicts resolved.
    pub(crate) fn stage_group(&mut self, group: Group, cx: &mut Context<Self>) {
        let paths = self.group_paths(group, cx);
        if paths.is_empty() {
            return;
        }
        self.store.update(cx, |s, cx| {
            s.run(
                move |repo| arcode_git::stage_paths(repo, &paths),
                |_, _, _| {},
                cx,
            )
        });
    }

    /// Paths of the active repository's entries in `group`.
    fn group_paths(&self, group: Group, cx: &App) -> Vec<PathBuf> {
        self.store
            .read(cx)
            .snapshot()
            .entries
            .iter()
            .filter(|e| group.contains(e))
            .map(|e| e.path.clone())
            .collect()
    }

    fn unstage_all(&mut self, cx: &mut Context<Self>) {
        self.store
            .update(cx, |s, cx| s.run(arcode_git::unstage_all, |_, _, _| {}, cx));
    }

    fn confirm_discard_group(&mut self, group: Group, window: &mut Window, cx: &mut Context<Self>) {
        let paths = self.group_paths(group, cx);
        self.confirm_discard(paths, window, cx);
    }

    /// Asks, then discards the working tree changes of `paths` (untracked
    /// files are deleted).
    pub fn confirm_discard(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let what = match paths.as_slice() {
            [one] => format!("Discard changes in {}?", one.display()),
            many => format!("Discard changes in {} files?", many.len()),
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &what,
            Some("This cannot be undone. Untracked files are deleted."),
            &["Discard", "Cancel"],
            cx,
        );
        let store = self.store.clone();
        cx.spawn(async move |_, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            store
                .update(cx, |s, cx| {
                    s.run(
                        move |repo| {
                            paths
                                .iter()
                                .try_for_each(|p| arcode_git::discard_changes(repo, p))
                        },
                        |_, _, _| {},
                        cx,
                    )
                })
                .ok();
        })
        .detach();
    }

    fn render_repo_switcher(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let store = self.store.read(cx);
        if store.repos().len() < 2 {
            return None;
        }
        let c = theme(cx).colors.clone();
        let root = store.root().map(Path::to_path_buf).unwrap_or_default();
        let active = store.active_index();
        let names: Vec<String> = store
            .repos()
            .iter()
            .map(|r| match r.workdir.strip_prefix(&root) {
                Ok(rel) if !rel.as_os_str().is_empty() => rel.display().to_string(),
                _ => r
                    .workdir
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            })
            .collect();
        Some(div().flex().flex_wrap().gap_1().px_2().pb_1().children(
            names.into_iter().enumerate().map(|(ix, name)| {
                div()
                    .id(("repo", ix))
                    .px_1p5()
                    .rounded_md()
                    .text_xs()
                    .cursor_pointer()
                    .border_1()
                    .border_color(if ix == active { c.accent } else { c.border })
                    .text_color(if ix == active { c.text } else { c.text_muted })
                    .hover(|s| s.bg(c.hover))
                    .child(name)
                    .on_click(cx.listener(move |p, _, _, cx| {
                        p.store.update(cx, |s, cx| s.set_active_repo(ix, cx))
                    }))
            }),
        ))
    }
}

impl Render for GitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let has_repo = self.store.read(cx).active_repo().is_some();
        let has_root = self.store.read(cx).root().is_some();
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(34.))
            .pl_3()
            .pr_1()
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(c.text_muted)
                    .child("SOURCE CONTROL"),
            )
            .when(has_repo, |el| {
                el.child(
                    icon_button("git-blame", IconName::History)
                        .size(px(24.))
                        .tooltip(format!(
                            "Toggle Blame ({})",
                            crate::actions::shortcut_label("secondary-alt-b")
                        ))
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(crate::actions::ToggleBlame), cx)
                        }),
                )
            })
            .when(has_root, |el| {
                el.child(
                    icon_button("git-refresh", IconName::Refresh)
                        .size(px(24.))
                        .tooltip("Refresh")
                        .on_click(
                            cx.listener(|p, _, _, cx| p.store.update(cx, |s, cx| s.reload(cx))),
                        ),
                )
            });
        let root = div()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::commit))
            .size_full()
            .flex()
            .flex_col()
            .child(header);
        if !has_repo {
            return root.child(self.render_empty(cx));
        }
        let rows = build_rows(&self.store.read(cx).snapshot().entries, &self.collapsed);
        let error = self.store.read(cx).snapshot().error.clone();
        let empty = rows.is_empty() && error.is_none();
        let list = uniform_list(
            "git-files",
            rows.len(),
            cx.processor(move |panel, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|ix| match rows.get(ix)? {
                        Row::Header(group, count) => {
                            Some(panel.render_header_row(ix, *group, *count, cx))
                        }
                        Row::File(group, entry) => {
                            Some(panel.render_file_row(ix, *group, entry, cx))
                        }
                    })
                    .collect()
            }),
        )
        .track_scroll(self.scroll.clone())
        .w_full()
        .flex_1();
        root.children(self.render_repo_switcher(cx))
            .child(self.render_branch_row(cx))
            .child(self.render_commit_box(window, cx))
            .child(div().h(px(1.)).mx_2().my_1().bg(c.border))
            .when(empty, |el| el.child(rows::note("No changes.", cx)))
            .children(error.map(|err| empty::error_note(err, cx)))
            .child(list)
    }
}
