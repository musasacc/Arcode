//! Search and TODO wiring of the workspace: the Search view, the TODO
//! store with its sidebar and panel views, opening their results, and
//! keeping the TODO list current after saves and file changes.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use arcode_buffer::Point;
use arcode_editor::{Editor, EditorEvent, ScrollAnchor};
use gpui::{AppContext, Context, Entity, Focusable, Subscription, Window};

use super::Workspace;
use crate::actions::*;
use crate::activity_bar::Activity;
use crate::bottom_panel::{BottomPanel, BottomTab};
use crate::search_view::{ProjectSearch, ProjectSearchEvent};
use crate::todo::{TodoEvent, TodoPlacement, TodoStore, TodoView};

/// The search and TODO entities of a workspace.
pub struct Features {
    /// The Search view.
    pub search: Entity<ProjectSearch>,
    /// The project's TODO comments.
    pub todo: Entity<TodoStore>,
    /// The TODO activity view.
    pub todo_side: Entity<TodoView>,
}

impl Features {
    /// Creates the entities, gives the bottom panel its Todo view and
    /// returns the workspace's subscriptions to them.
    pub(super) fn new(
        bottom_panel: &Entity<BottomPanel>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> (Self, Vec<Subscription>) {
        let weak = cx.entity().downgrade();
        let search = cx.new(|cx| {
            let mut view = ProjectSearch::new(cx);
            view.set_editors_fn(Rc::new(move |cx| {
                weak.upgrade()
                    .map(|ws| ws.read(cx).editors(cx))
                    .unwrap_or_default()
            }));
            view
        });
        let todo = cx.new(TodoStore::new);
        let todo_side = cx.new(|cx| TodoView::new(todo.clone(), TodoPlacement::Sidebar, cx));
        let todo_panel = cx.new(|cx| TodoView::new(todo.clone(), TodoPlacement::Panel, cx));
        bottom_panel.update(cx, |p, cx| p.set_todo(todo.clone(), todo_panel.clone(), cx));
        let store = todo.clone();
        let subs = vec![
            cx.subscribe_in(&search, window, |ws, _, event, window, cx| match event {
                ProjectSearchEvent::Open { path, range } => {
                    ws.open_at(path, range.clone(), window, cx)
                }
                ProjectSearchEvent::Message(text) => ws.show_toast(text.clone(), cx),
            }),
            cx.subscribe_in(&todo_side, window, on_todo),
            cx.subscribe_in(&todo_panel, window, on_todo),
            cx.observe_global::<arcode_ui_kit::Settings>(move |_, cx| {
                store.update(cx, |s, cx| s.settings_changed(cx));
            }),
        ];
        (
            Self {
                search,
                todo,
                todo_side,
            },
            subs,
        )
    }

    /// Points search and the TODO list at the project root.
    pub(super) fn set_root(&self, root: Option<PathBuf>, cx: &mut Context<Workspace>) {
        self.search.update(cx, |s, cx| s.set_root(root.clone(), cx));
        self.todo.update(cx, |t, cx| t.set_root(root, cx));
    }
}

/// Opens a TODO item the user picked.
fn on_todo(
    ws: &mut Workspace,
    _: &Entity<TodoView>,
    event: &TodoEvent,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let TodoEvent::Open { path, line, column } = event;
    let at = Point::new(*line, *column);
    ws.open_at(path, at..at, window, cx);
}

impl Workspace {
    /// The Search view.
    pub fn project_search(&self) -> &Entity<ProjectSearch> {
        &self.features.search
    }

    /// The TODO store.
    pub fn todos(&self) -> &Entity<TodoStore> {
        &self.features.todo
    }

    /// Opens `path` (or switches to it), selects `range` and centers it.
    pub fn open_at(
        &mut self,
        path: &Path,
        range: Range<Point>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file(path, window, cx);
        let Some(editor) = self.editor_for_path(path, cx) else {
            return;
        };
        editor.update(cx, |e, cx| {
            e.select_ranges(vec![range.clone()], cx);
            e.scroll_to_line(range.start.line, ScrollAnchor::Center, cx);
        });
        window.focus(&editor.focus_handle(cx));
    }

    /// Follows saves of `editor` (the TODO list rescans the file).
    pub(super) fn track_feature_editor(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let todo = self.features.todo.clone();
        let sub = cx.subscribe(editor, move |_, editor, event: &EditorEvent, cx| {
            if *event == EditorEvent::Saved
                && let Some(path) = editor.read(cx).path().map(Path::to_path_buf)
            {
                todo.update(cx, |t, cx| t.paths_changed(&[path], false, cx));
            }
        });
        self.subs.tie(editor.downgrade().into(), sub);
    }

    /// Files changed on disk (from the project watcher).
    pub(super) fn files_changed_on_disk(
        &mut self,
        paths: &[PathBuf],
        overflow: bool,
        cx: &mut Context<Self>,
    ) {
        self.features
            .todo
            .update(cx, |t, cx| t.paths_changed(paths, overflow, cx));
        self.debug
            .debugger
            .update(cx, |d, cx| d.paths_changed(paths, overflow, cx));
        self.packages_paths_changed(paths, overflow, cx);
    }

    /// The one-line selection of the active editor (to seed searches).
    fn selected_text(&self, cx: &gpui::App) -> Option<String> {
        let editor = self.active_editor(cx)?;
        let e = editor.read(cx);
        let sel = e.selections().primary();
        let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
        let text = e.text_in_range(e.offset_to_point(a)..e.offset_to_point(b));
        (!text.is_empty() && !text.contains('\n')).then_some(text)
    }

    fn show_search(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let seed = self.selected_text(cx);
        self.activity = Activity::Search;
        self.sidebar_visible = true;
        self.features
            .search
            .update(cx, |s, cx| s.focus_query(seed, replace, window, cx));
        cx.notify();
    }

    pub(super) fn register_feature_actions<E: gpui::InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(cx.listener(Self::toggle_palette))
            .on_action(cx.listener(|ws, _: &ShowProjectSearch, window, cx| {
                ws.show_search(false, window, cx)
            }))
            .on_action(
                cx.listener(|ws, _: &ReplaceInFiles, window, cx| ws.show_search(true, window, cx)),
            )
            .on_action(cx.listener(|ws, _: &ShowTodos, window, cx| {
                ws.activity = Activity::Todo;
                ws.sidebar_visible = true;
                window.focus(&ws.features.todo_side.focus_handle(cx));
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &ShowTodoPanel, window, cx| {
                ws.bottom_panel_visible = true;
                ws.bottom_panel.update(cx, |p, cx| {
                    p.set_active_tab(BottomTab::Todo, cx);
                    p.focus(window, cx);
                });
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &RefreshTodos, _, cx| {
                ws.features.todo.update(cx, |t, cx| t.rescan(cx));
            }))
    }
}
