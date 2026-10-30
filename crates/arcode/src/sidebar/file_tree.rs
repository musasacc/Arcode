//! The project file tree: a lazily loaded, expandable list of folders and
//! files rendered with a `uniform_list` (only visible rows are built).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use arcode_ui_kit::{IconName, icon, icon_button, theme, truncated};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, KeyBinding, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window, actions, div, prelude::*, px, uniform_list,
};

use crate::file_icons::file_icon;
use crate::project::{DirEntry, read_dir_sorted};

actions!(file_tree, [SelectPrev, SelectNext, Expand, Collapse, Open]);

const CONTEXT: &str = "FileTree";
const ROW_HEIGHT: f32 = 24.;
const INDENT: f32 = 12.;
const LEFT_PAD: f32 = 8.;

/// Registers the tree's keyboard navigation bindings.
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, ctx),
        KeyBinding::new("down", SelectNext, ctx),
        KeyBinding::new("right", Expand, ctx),
        KeyBinding::new("left", Collapse, ctx),
        KeyBinding::new("enter", Open, ctx),
    ]);
}

/// Events emitted by the [`FileTree`].
pub enum FileTreeEvent {
    /// A file was chosen.
    OpenFile(PathBuf),
}

/// One visible row.
struct Row {
    path: PathBuf,
    name: SharedString,
    depth: usize,
    is_dir: bool,
    expanded: bool,
}

/// The file tree view of one project root.
pub struct FileTree {
    root: PathBuf,
    title: SharedString,
    children: HashMap<PathBuf, Vec<DirEntry>>,
    expanded: HashSet<PathBuf>,
    rows: Vec<Row>,
    selected: Option<PathBuf>,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
}

impl EventEmitter<FileTreeEvent> for FileTree {}

impl Focusable for FileTree {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl FileTree {
    /// Creates a tree for `root`, showing its direct children.
    pub fn new(root: PathBuf, title: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let mut tree = Self {
            root,
            title: title.into(),
            children: HashMap::new(),
            expanded: HashSet::new(),
            rows: Vec::new(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
        };
        tree.rebuild();
        tree
    }

    /// Re-reads every expanded folder from disk.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.children.clear();
        self.expanded.retain(|p| p.is_dir());
        self.rebuild();
        cx.notify();
    }

    /// Collapses every folder.
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expanded.clear();
        self.rebuild();
        cx.notify();
    }

    /// Clears the selection (e.g. when the last tab closed).
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected.take().is_some() {
            cx.notify();
        }
    }

    /// The selected path, if any.
    pub fn selected_path(&self) -> Option<&Path> {
        self.selected.as_deref()
    }

    /// Expands the folders containing `path`, selects it and scrolls to it.
    pub fn reveal(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Ok(rel) = path.strip_prefix(&self.root) else {
            // Outside this project: nothing in the tree matches the file,
            // so no stale entry should stay selected.
            self.clear_selection(cx);
            return;
        };
        let mut dir = self.root.clone();
        let mut parts = rel.components().peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                dir.push(part);
                self.expanded.insert(dir.clone());
            }
        }
        self.selected = Some(path.to_path_buf());
        self.rebuild();
        if let Some(ix) = self.rows.iter().position(|r| r.path == path) {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn entries(&mut self, dir: &Path) -> &[DirEntry] {
        self.children
            .entry(dir.to_path_buf())
            .or_insert_with(|| read_dir_sorted(dir))
    }

    /// Flattens the expanded tree into visible rows.
    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        // Depth-first walk; children are pushed in reverse to keep order.
        let root = self.root.clone();
        let mut pending: Vec<(DirEntry, usize)> = self
            .entries(&root)
            .iter()
            .rev()
            .map(|e| (e.clone(), 0))
            .collect();
        while let Some((entry, depth)) = pending.pop() {
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            if expanded {
                let children = self.entries(&entry.path);
                pending.extend(children.iter().rev().map(|c| (c.clone(), depth + 1)));
            }
            rows.push(Row {
                name: entry.name.into(),
                path: entry.path,
                depth,
                is_dir: entry.is_dir,
                expanded,
            });
        }
        self.rows = rows;
    }

    fn toggle(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_path_buf());
        }
        self.rebuild();
    }

    fn click(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(ix) else { return };
        let (path, is_dir) = (row.path.clone(), row.is_dir);
        self.selected = Some(path.clone());
        if is_dir {
            self.toggle(&path);
        } else {
            cx.emit(FileTreeEvent::OpenFile(path));
        }
        cx.notify();
    }

    fn selected_index(&self) -> Option<usize> {
        let sel = self.selected.as_ref()?;
        self.rows.iter().position(|r| &r.path == sel)
    }

    fn select_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(ix) {
            self.selected = Some(row.path.clone());
            self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
            cx.notify();
        }
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        let ix = self.selected_index().map_or(0, |i| i.saturating_sub(1));
        self.select_index(ix, cx);
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let ix = self.selected_index().map_or(0, |i| i + 1);
        self.select_index(ix.min(self.rows.len().saturating_sub(1)), cx);
    }

    fn expand(&mut self, _: &Expand, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected_index()
            && self.rows[ix].is_dir
            && !self.rows[ix].expanded
        {
            self.click(ix, cx);
        }
    }

    fn collapse(&mut self, _: &Collapse, _: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.selected_index() else {
            return;
        };
        let row = &self.rows[ix];
        if row.is_dir && row.expanded {
            self.click(ix, cx);
        } else if let Some(parent) = row.path.parent().filter(|p| *p != self.root) {
            let parent = parent.to_path_buf();
            self.expanded.remove(&parent);
            self.selected = Some(parent);
            self.rebuild();
            cx.notify();
        }
    }

    fn open(&mut self, _: &Open, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected_index() {
            self.click(ix, cx);
        }
    }

    fn render_row(&self, ix: usize, focused: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme(cx).colors.clone();
        let row = &self.rows[ix];
        let selected = self.selected.as_ref() == Some(&row.path);
        let focused = selected && focused;
        let (icon_name, icon_color) = if row.is_dir {
            let name = if row.expanded {
                IconName::FolderOpen
            } else {
                IconName::Folder
            };
            (name, c.accent)
        } else {
            file_icon(&row.name, &c)
        };
        let chevron = if row.expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        let indent = px(LEFT_PAD + row.depth as f32 * INDENT);
        div()
            .id(ix)
            .relative()
            .w_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap_1()
            .h(px(ROW_HEIGHT))
            .pl(indent)
            .pr_2()
            .cursor_pointer()
            .text_color(if selected { c.text } else { c.text_muted })
            .when(selected, |el| el.bg(c.selection))
            .when(focused, |el| el.border_1().border_color(c.border_focused))
            .when(!selected, |el| {
                el.hover(|s| s.bg(c.hover).text_color(c.text))
            })
            .children((0..row.depth).map(|d| {
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(LEFT_PAD + d as f32 * INDENT + 6.))
                    .w(px(1.))
                    .bg(c.border)
            }))
            .child(
                div()
                    .size(px(14.))
                    .flex_none()
                    .when(row.is_dir, |el| el.child(icon(chevron).size(px(14.)))),
            )
            .child(icon(icon_name).size(px(14.)).color(icon_color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(truncated(row.name.clone())),
            )
            .on_click(cx.listener(move |tree, _, window, cx| {
                window.focus(&tree.focus_handle);
                tree.click(ix, cx);
            }))
            .into_any_element()
    }
}

impl Render for FileTree {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(34.))
            .pl_3()
            .pr_1()
            .text_xs()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(c.text_muted)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(truncated(self.title.to_uppercase())),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .child(
                        icon_button("tree-refresh", IconName::Refresh)
                            .size(px(24.))
                            .tooltip("Refresh")
                            .on_click(cx.listener(|tree, _, _, cx| tree.refresh(cx))),
                    )
                    .child(
                        icon_button("tree-collapse", IconName::Minus)
                            .size(px(24.))
                            .tooltip("Collapse folders")
                            .on_click(cx.listener(|tree, _, _, cx| tree.collapse_all(cx))),
                    ),
            );
        let list = uniform_list(
            "file-tree",
            self.rows.len(),
            cx.processor(|tree, range: std::ops::Range<usize>, window, cx| {
                let focused = tree.focus_handle.is_focused(window);
                range
                    .map(|ix| tree.render_row(ix, focused, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .flex_1()
        .text_sm();
        let empty = self.rows.is_empty().then(|| {
            div()
                .px_3()
                .py_2()
                .text_sm()
                .text_color(c.text_disabled)
                .child("This folder is empty.")
        });
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::expand))
            .on_action(cx.listener(Self::collapse))
            .on_action(cx.listener(Self::open))
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .children(empty)
            .child(list)
    }
}

#[cfg(test)]
mod tests;
