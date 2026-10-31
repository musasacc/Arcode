//! The `table` widget (docs/PLUGINS.md §7 "Big tables never cross the
//! boundary in full"): a virtualized `uniform_list` whose rows come from a
//! [`RowCache`]. Drawing a range of rows tells the cache the viewport; the
//! cache answers which page to fetch (visible pages first, then one ahead,
//! one fetch outstanding) and keeps at most a few pages. Rows not cached
//! yet draw as placeholders and fill in when their page arrives.

use std::ops::Range;

use arcode_plugin_host::bindings::NodeId;
use arcode_plugin_host::bindings::ui::{Cell, EventKind, TableProps, Widget};
use arcode_plugin_host::rows::{PAGE_ROWS, RowCache};
use arcode_plugin_host::{CallError, CallOutput};
use arcode_ui_kit::{BUNDLED_MONO_FONT, IconName, icon, theme};
use gpui::{
    AnyElement, ClickEvent, Context, ScrollStrategy, ScrollWheelEvent, SharedString,
    UniformListScrollHandle, div, prelude::*, px, uniform_list,
};

use super::PanelView;
use super::cells::{cell_text, column_width, index_width};
use super::layout::copyable;

/// Height of a row (and of the header).
pub(super) const ROW_HEIGHT: f32 = 24.;

/// One table's host-side state.
pub(super) struct TableState {
    cache: RowCache,
    scroll: UniformListScrollHandle,
    /// The sort the user asked for: (column, ascending).
    sort: Option<(u32, bool)>,
    selected: Option<u64>,
    /// The last fetch failed; no refetch until the table changes.
    error: Option<String>,
}

impl TableState {
    fn new(row_count: Option<u64>) -> Self {
        Self {
            cache: RowCache::new(row_count),
            scroll: UniformListScrollHandle::new(),
            sort: None,
            selected: None,
            error: None,
        }
    }

    /// Rows the list shows: the known count, or (while unknown) the rows
    /// paged so far plus one page to scroll into.
    pub(super) fn item_count(&self) -> u64 {
        match self.cache.row_count() {
            Some(n) => n,
            None => {
                let pages = self.cache.cached_pages();
                let next = pages.iter().max().map_or(0, |p| p + 1);
                (next + 1) * u64::from(PAGE_ROWS)
            }
        }
    }
}

#[cfg(test)]
impl TableState {
    pub(super) fn new_for_test(row_count: Option<u64>) -> Self {
        Self::new(row_count)
    }

    pub(super) fn cache_mut(&mut self) -> &mut RowCache {
        &mut self.cache
    }
}

fn table_props(view: &PanelView, id: NodeId) -> Option<TableProps> {
    match view.snapshot.node(id).map(|n| &n.widget) {
        Some(Widget::Table(p)) => Some(p.clone()),
        _ => None,
    }
}

impl PanelView {
    /// A `TableChange` from the plugin: drop pages or adjust the count.
    pub(super) fn table_changed(&mut self, id: NodeId, rows: Option<u64>, reset: bool) {
        let Some(state) = self.tables.get_mut(&id) else {
            return;
        };
        if reset {
            state.cache.reset(rows);
            state.selected = None;
            state.scroll.scroll_to_item(0, ScrollStrategy::Top);
        } else {
            state.cache.set_row_count(rows);
        }
        state.error = None;
    }

    /// Issues the next page fetch the cache asks for, if any.
    fn pump_fetch(&mut self, id: NodeId, cx: &mut Context<Self>) {
        if self.retired {
            return;
        }
        let Some(state) = self.tables.get_mut(&id) else {
            return;
        };
        if state.error.is_some() {
            return;
        }
        let Some(request) = state.cache.next_fetch() else {
            return;
        };
        let pending = self
            .client
            .manager()
            .map(|m| m.fetch_rows(&self.plugin, self.panel, id, request));
        let Some(Ok(pending)) = pending else {
            state.cache.page_failed(request);
            state.error = Some("the plugin is not running".to_owned());
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = pending.result.recv().await;
            this.update(cx, |view, cx| {
                let Some(state) = view.tables.get_mut(&id) else {
                    return;
                };
                match result {
                    Ok(Ok(CallOutput::Rows(page))) => {
                        state.cache.page_arrived(request, page);
                    }
                    Ok(Err(CallError::Superseded)) => state.cache.page_failed(request),
                    Ok(Err(e)) => {
                        state.cache.page_failed(request);
                        state.error = Some(e.to_string());
                    }
                    Ok(Ok(_)) | Err(_) => state.cache.page_failed(request),
                }
                view.pump_fetch(id, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The rows `range` of table `id`, drawn by the list. Also moves the
    /// cache's viewport there and starts the fetch it needs.
    fn table_rows(
        &mut self,
        id: NodeId,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(props) = table_props(self, id) else {
            return Vec::new();
        };
        let c = theme(cx).colors.clone();
        let Some(state) = self.tables.get_mut(&id) else {
            return Vec::new();
        };
        // The list also lays out row 0 alone to measure the row height;
        // that is not where the user is looking.
        let measuring = range == (0..1) && state.item_count() > 1;
        if !range.is_empty() && !measuring {
            state
                .cache
                .set_viewport(range.start as u64, (range.end - 1) as u64);
        }
        let selected = state.selected;
        let index = index_width(state.item_count());
        let rows: Vec<(usize, Option<Vec<Cell>>)> = range
            .map(|ix| (ix, state.cache.row(ix as u64).map(<[Cell]>::to_vec)))
            .collect();
        self.pump_fetch(id, cx);
        rows.into_iter()
            .map(|(ix, cells)| {
                let row = ix as u64;
                let mut el = div()
                    .id(("row", ix))
                    .flex()
                    .h(px(ROW_HEIGHT))
                    .items_center()
                    .border_b_1()
                    .border_color(c.border.opacity(0.4))
                    .when(selected == Some(row), |el| el.bg(c.selection))
                    .hover(|s| s.bg(c.hover))
                    .on_click(cx.listener(move |view, e: &ClickEvent, window, cx| {
                        view.key_table = Some(id);
                        window.focus(&view.focus);
                        if let Some(s) = view.tables.get_mut(&id) {
                            s.selected = Some(row);
                            cx.notify();
                        }
                        if e.click_count() >= 2 {
                            view.send(id, EventKind::RowActivate(row), cx);
                        }
                    }))
                    .child(
                        div()
                            .w(px(index))
                            .flex_none()
                            .px_2()
                            .text_right()
                            .text_color(c.text_muted)
                            .child(SharedString::from((ix + 1).to_string())),
                    );
                for (col, column) in props.columns.iter().enumerate() {
                    let cell = cells.as_ref().map(|r| r.get(col).unwrap_or(&Cell::Empty));
                    let text = cell.map(cell_text);
                    let null = matches!(cell, Some(Cell::Empty));
                    let inner = div()
                        .w(px(column_width(column)))
                        .flex_none()
                        .px_2()
                        .truncate()
                        .font_family(BUNDLED_MONO_FONT)
                        .when(column.numeric, |el| el.text_right())
                        .when(null || text.is_none(), |el| el.text_color(c.text_muted));
                    el = el.child(match text {
                        Some(t) => copyable(inner.child(SharedString::from(t.clone())), t),
                        None => inner.child("…"),
                    });
                }
                el.into_any_element()
            })
            .collect()
    }

    /// Keyboard navigation of the table last clicked: arrows, Page Up /
    /// Page Down, Home / End move the selection (fetching rows as needed),
    /// Enter activates it. Returns whether the key was used.
    pub(super) fn table_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.key_table else {
            return false;
        };
        let Some(state) = self.tables.get_mut(&id) else {
            return false;
        };
        let count = state.item_count();
        let selected = state.selected;
        if count == 0 {
            return false;
        }
        let current = selected.unwrap_or(0);
        let page = 20;
        let target = match key {
            "home" => 0,
            "end" => count - 1,
            "pageup" => current.saturating_sub(page),
            "pagedown" => (current + page).min(count - 1),
            "up" => current.saturating_sub(1),
            "down" => (current + 1).min(count - 1),
            "enter" => {
                let Some(row) = selected else {
                    return false;
                };
                self.send(id, EventKind::RowActivate(row), cx);
                return true;
            }
            _ => return false,
        };
        state.selected = Some(target);
        let ix = usize::try_from(target).unwrap_or(usize::MAX);
        state.scroll.scroll_to_item(ix, ScrollStrategy::Center);
        cx.notify();
        true
    }

    fn sort_clicked(&mut self, id: NodeId, column: u32, cx: &mut Context<Self>) {
        let Some(state) = self.tables.get_mut(&id) else {
            return;
        };
        let ascending = match state.sort {
            Some((c, asc)) if c == column => !asc,
            _ => true,
        };
        state.sort = Some((column, ascending));
        cx.notify();
        self.send(id, EventKind::Sort((column, ascending)), cx);
    }

    pub(super) fn render_table(
        &mut self,
        id: NodeId,
        p: &TableProps,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        let state = self
            .tables
            .entry(id)
            .or_insert_with(|| TableState::new(p.row_count));
        let count = usize::try_from(state.item_count()).unwrap_or(usize::MAX);
        let scroll = state.scroll.clone();
        let sort = state.sort;
        let error = state.error.clone();
        let index = index_width(state.item_count());
        let width = index + p.columns.iter().map(column_width).sum::<f32>();
        let header = div()
            .flex()
            .flex_none()
            .h(px(ROW_HEIGHT))
            .items_center()
            .bg(c.panel_header)
            .border_b_1()
            .border_color(c.border)
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(div().w(px(index)).flex_none())
            .children(p.columns.iter().enumerate().map(|(i, column)| {
                let i = i as u32;
                let arrow = match sort {
                    Some((col, true)) if col == i => Some(IconName::ArrowUp),
                    Some((col, false)) if col == i => Some(IconName::ArrowDown),
                    _ => None,
                };
                div()
                    .id(("column", i as usize))
                    .flex()
                    .items_center()
                    .gap_1()
                    .w(px(column_width(column)))
                    .flex_none()
                    .px_2()
                    .min_w_0()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(column.title.clone())),
                    )
                    .when_some(arrow, |el, a| {
                        el.child(icon(a).size(px(12.)).color(c.text_muted))
                    })
                    .when(column.sortable, |el| {
                        el.cursor_pointer().hover(|s| s.bg(c.hover)).on_click(
                            cx.listener(move |view, _, _, cx| view.sort_clicked(id, i, cx)),
                        )
                    })
            }));
        let weak = cx.entity().downgrade();
        let list = uniform_list(("table", id), count, move |range, _, cx| {
            weak.update(cx, |view, cx| view.table_rows(id, range, cx))
                .unwrap_or_default()
        })
        .track_scroll(scroll)
        .size_full();
        // Vertical wheel scrolls the rows only; the horizontal scroller
        // around them would otherwise turn it into sideways scrolling.
        let list = div()
            .flex_1()
            .min_h_0()
            .w(px(width))
            .on_scroll_wheel(|e: &ScrollWheelEvent, _, cx| {
                let d = e.delta.pixel_delta(px(ROW_HEIGHT));
                if d.y.abs() > d.x.abs() {
                    cx.stop_propagation();
                }
            })
            .child(list);
        div()
            .size_full()
            .flex()
            .flex_col()
            .when_some(error, |el, e| {
                el.child(
                    div()
                        .flex_none()
                        .px_2()
                        .py_1()
                        .text_color(c.error)
                        .child(SharedString::from(format!("Rows could not be loaded: {e}"))),
                )
            })
            .child(
                div()
                    .id(("table-h", id))
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_x_scroll()
                    .child(
                        div()
                            .h_full()
                            .w(px(width))
                            .flex()
                            .flex_col()
                            .child(header)
                            .child(list),
                    ),
            )
            .into_any_element()
    }
}
