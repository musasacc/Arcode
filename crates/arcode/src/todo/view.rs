//! The TODO list view, used twice over one [`TodoStore`]: in the sidebar
//! (TODO activity) and as the Todo tab of the bottom panel.

use std::collections::HashSet;
use std::path::PathBuf;

use arcode_ui_kit::metrics::{ROW_RADIUS, icon_size};
use arcode_ui_kit::{IconName, ThemeColors, badge, icon, icon_button, readout, theme};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, SharedString,
    Subscription, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use super::TodoStore;
use super::rows::{Row, build_rows, relative_label, tag_counts};
use crate::file_icons::file_icon;
use crate::search_view::results::split_path;

const ROW_HEIGHT: f32 = 24.;

/// Emitted when the user picks an item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TodoEvent {
    /// Open `path` at `line`, `column` (zero-based, chars).
    Open {
        /// The file.
        path: PathBuf,
        /// Line of the comment.
        line: usize,
        /// Column of the tag.
        column: usize,
    },
}

/// Where the view is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TodoPlacement {
    /// The sidebar (with its own header).
    Sidebar,
    /// The bottom panel tab.
    Panel,
}

/// A TODO list view.
pub struct TodoView {
    store: Entity<TodoStore>,
    placement: TodoPlacement,
    filter: HashSet<String>,
    collapsed: HashSet<PathBuf>,
    rows: Vec<Row>,
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    _observe: Subscription,
}

impl EventEmitter<TodoEvent> for TodoView {}

impl Focusable for TodoView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// The theme color of a tag.
pub fn tag_color(tag: &str, c: &ThemeColors) -> Hsla {
    match tag.trim_start_matches('@').to_ascii_uppercase().as_str() {
        "FIXME" | "BUG" => c.error,
        "HACK" | "XXX" => c.warning,
        "NOTE" => c.info,
        _ => c.accent,
    }
}

impl TodoView {
    /// A view over `store`.
    pub fn new(store: Entity<TodoStore>, placement: TodoPlacement, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&store, |view, _, cx| view.rebuild(cx));
        let mut view = Self {
            store,
            placement,
            filter: HashSet::new(),
            collapsed: HashSet::new(),
            rows: Vec::new(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _observe: observe,
        };
        view.rebuild(cx);
        view
    }

    /// The rows as displayed.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Shows only `tag` (together with other selected chips), or stops
    /// filtering by it.
    pub fn toggle_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        if !self.filter.remove(tag) {
            self.filter.insert(tag.to_owned());
        }
        self.rebuild(cx);
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        self.filter.retain(|t| store.tags().contains(t));
        let rows = build_rows(store.files(), &self.filter, &self.collapsed);
        if rows != self.rows {
            self.rows = rows;
            self.selected = None;
        }
        cx.notify();
    }

    /// Activates row `ix`: a file collapses or expands, an item opens.
    pub fn open_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        match self.rows.get(ix).cloned() {
            Some(Row::File { path, .. }) => {
                if !self.collapsed.remove(&path) {
                    self.collapsed.insert(path);
                }
                self.rebuild(cx);
            }
            Some(Row::Item { path, index }) => {
                let item = self
                    .store
                    .read(cx)
                    .files()
                    .get(&path)
                    .and_then(|l| l.get(index));
                if let Some(item) = item {
                    cx.emit(TodoEvent::Open {
                        path: path.clone(),
                        line: item.line,
                        column: item.column,
                    });
                }
                cx.notify();
            }
            None => {}
        }
    }

    fn render_chips(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let store = self.store.read(cx);
        let counts = tag_counts(store.files(), store.tags());
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .px_2()
            .py_1()
            .children(counts.into_iter().map(|(tag, n)| {
                let on = self.filter.contains(&tag);
                let color = tag_color(&tag, &c);
                let id = SharedString::from(format!("todo-chip-{tag}"));
                let tag_for_click = tag.clone();
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(20.))
                    .px_2()
                    .rounded_full()
                    .border_1()
                    .cursor_pointer()
                    .text_xs()
                    .border_color(if on { color } else { c.border })
                    .when(on, |el| el.bg(c.pill))
                    .when(!on, |el| el.hover(|s| s.bg(c.hover)))
                    .text_color(if n == 0 { c.text_disabled } else { c.text })
                    .child(div().size(px(6.)).rounded_full().bg(color))
                    .child(tag)
                    .child(div().text_color(c.text_muted).child(n.to_string()))
                    .on_click(
                        cx.listener(move |view, _, _, cx| view.toggle_tag(&tag_for_click, cx)),
                    )
            }))
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme(cx).colors.clone();
        let selected = self.selected == Some(ix);
        let base = div()
            .id(ix)
            .flex()
            .items_center()
            .gap_1p5()
            .h(px(ROW_HEIGHT))
            .px_2()
            .w_full()
            .rounded(ROW_RADIUS)
            .overflow_hidden()
            .whitespace_nowrap()
            .cursor_pointer()
            .when(selected, |el| el.bg(c.pill))
            .when(!selected, |el| el.hover(|s| s.bg(c.hover)))
            .on_click(cx.listener(move |view, _, _, cx| view.open_row(ix, cx)));
        crate::sidebar::inset(self.render_row_content(ix, base, cx)).into_any_element()
    }

    fn render_row_content(
        &self,
        ix: usize,
        base: gpui::Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let c = theme(cx).colors.clone();
        let store = self.store.read(cx);
        match self.rows.get(ix) {
            Some(Row::File {
                path,
                count,
                collapsed,
            }) => {
                let label = relative_label(store.root(), path);
                let (name, dir) = split_path(std::path::Path::new(&label));
                let (file_icon_name, file_icon_color) = file_icon(&name, &c);
                base.child(
                    icon(if *collapsed {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    })
                    .size(px(14.))
                    .color(c.text_muted),
                )
                .child(
                    icon(file_icon_name)
                        .size(icon_size::ROW)
                        .color(file_icon_color),
                )
                .child(div().flex_none().text_color(c.text).child(name))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(c.text_muted)
                        .child(dir),
                )
                .child(badge(count.to_string()))
                .into_any_element()
            }
            Some(Row::Item { path, index }) => {
                let Some(item) = store.files().get(path).and_then(|l| l.get(*index)) else {
                    return base.into_any_element();
                };
                let color = tag_color(&item.tag, &c);
                let text = if item.text.is_empty() {
                    "(no text)".to_owned()
                } else {
                    item.text.clone()
                };
                base.pl(px(30.))
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_xs()
                            .text_color(color)
                            .child(item.tag.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(c.text)
                            .child(text),
                    )
                    .when_some(item.author.clone(), |el, a| {
                        el.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(c.text_muted)
                                .child(a),
                        )
                    })
                    .child(readout((item.line + 1).to_string()))
                    .into_any_element()
            }
            None => base.into_any_element(),
        }
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let store = self.store.read(cx);
        let text = if store.root().is_none() {
            "Open a folder to list its TODO comments."
        } else if store.is_scanning() && !store.is_loaded() {
            "Scanning…"
        } else if !self.filter.is_empty() && store.count() > 0 {
            "Nothing with the selected tags."
        } else {
            "No TODO comments found."
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .p_4()
            .text_sm()
            .text_color(c.text_muted)
            .child(
                icon(IconName::ListTodo)
                    .size(px(24.))
                    .color(c.text_disabled),
            )
            .child(text)
    }
}

impl Render for TodoView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let total = store.count();
        let scanning = store.is_scanning();
        let refresh = icon_button(
            if self.placement == TodoPlacement::Sidebar {
                "todo-refresh-side"
            } else {
                "todo-refresh-panel"
            },
            IconName::Refresh,
        )
        .size(px(22.))
        .tooltip(if scanning {
            "Scanning…"
        } else {
            "Rescan Project"
        })
        .on_click(cx.listener(|view, _, _, cx| {
            view.store.update(cx, |s, cx| s.rescan(cx));
        }));
        let top = match self.placement {
            TodoPlacement::Sidebar => crate::sidebar::header_count("Todo", total, cx)
                .child(refresh.size(px(24.)))
                .into_any_element(),
            TodoPlacement::Panel => div()
                .flex()
                .items_start()
                .child(div().flex_1().min_w_0().child(self.render_chips(cx)))
                .child(div().pt_0p5().pr_1().child(refresh))
                .into_any_element(),
        };
        let list = (!self.rows.is_empty()).then(|| {
            uniform_list(
                "todo-list",
                self.rows.len(),
                cx.processor(|view, range: std::ops::Range<usize>, _, cx| {
                    range.map(|ix| view.render_row(ix, cx)).collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .flex_1()
        });
        let empty = self.rows.is_empty().then(|| self.render_empty(cx));
        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .text_sm()
            .child(top)
            .when(self.placement == TodoPlacement::Sidebar, |el| {
                el.child(self.render_chips(cx))
            })
            .children(list)
            .children(empty)
    }
}
