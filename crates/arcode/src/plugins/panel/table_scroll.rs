//! Moving around a `table` widget: the keys, and the vertical scrollbar
//! (where the viewport is in a table of 100,000 rows, and a track that
//! jumps there on click or drag). The scrollbar is drawn from the list's
//! last layout; nothing is polled or animated.

use arcode_plugin_host::bindings::NodeId;
use arcode_plugin_host::bindings::ui::EventKind;
use arcode_ui_kit::theme;
use gpui::{
    AnyElement, Context, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, ScrollStrategy,
    UniformListScrollHandle, div, prelude::*, px,
};

use super::PanelView;
use super::table::ROW_HEIGHT;

/// Width of the track.
const TRACK_WIDTH: f32 = 8.;
/// Smallest thumb, so it stays grabbable in huge tables.
const MIN_THUMB: f32 = 24.;

/// Thumb position and length in pixels inside a track as tall as the
/// viewport, or `None` when everything fits.
pub(super) fn thumb(viewport: f32, rows: u64, scroll_top: f32) -> Option<(f32, f32)> {
    let content = rows as f32 * ROW_HEIGHT;
    if viewport <= 0. || content <= viewport {
        return None;
    }
    let length = (viewport * viewport / content).clamp(MIN_THUMB.min(viewport), viewport);
    let fraction = (scroll_top / (content - viewport)).clamp(0., 1.);
    Some((fraction * (viewport - length), length))
}

/// The row to center when the track is clicked `y` pixels below its top.
pub(super) fn row_at(y: f32, viewport: f32, rows: u64) -> u64 {
    if rows == 0 || viewport <= 0. {
        return 0;
    }
    let fraction = (y / viewport).clamp(0., 1.);
    ((fraction * rows as f32) as u64).min(rows - 1)
}

impl PanelView {
    /// Keyboard navigation of the table last clicked (or the first one
    /// shown): arrows, Page Up / Page Down, Home / End (also with Ctrl or
    /// Cmd) move the selection (fetching rows as needed), Enter activates
    /// it. Returns whether the key was used.
    pub(super) fn table_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.key_table else {
            return false;
        };
        let Some(state) = self.tables.get_mut(&id) else {
            return false;
        };
        let count = state.item_count();
        let selected = state.selected();
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
        state.select_and_reveal(target);
        cx.notify();
        true
    }

    /// The scrollbar of table `id` with `rows` rows, placed over the right
    /// edge of the table's area (below the header).
    pub(super) fn render_table_scrollbar(
        &self,
        id: NodeId,
        scroll: &UniformListScrollHandle,
        rows: u64,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let c = theme(cx).colors.clone();
        let (viewport, scroll_top) = {
            let state = scroll.0.borrow();
            let handle = &state.base_handle;
            (
                f32::from(handle.bounds().size.height),
                -f32::from(handle.offset().y),
            )
        };
        let (top, length) = thumb(viewport, rows, scroll_top)?;
        let jump = move |view: &mut PanelView, y: Pixels, cx: &mut Context<PanelView>| {
            view.table_scroll_to(id, y, rows, cx)
        };
        Some(
            div()
                .id(("table-scrollbar", id))
                .absolute()
                .top(px(ROW_HEIGHT))
                .right_0()
                .w(px(TRACK_WIDTH))
                .h(px(viewport))
                .cursor_default()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, e: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        jump(view, e.position.y, cx);
                    }),
                )
                .on_mouse_move(cx.listener(move |view, e: &MouseMoveEvent, _, cx| {
                    if e.dragging() {
                        jump(view, e.position.y, cx);
                    }
                }))
                .child(
                    div()
                        .absolute()
                        .top(px(top))
                        .left(px(1.))
                        .w(px(TRACK_WIDTH - 2.))
                        .h(px(length))
                        .rounded_full()
                        .bg(c.text_muted.opacity(0.35)),
                )
                .into_any_element(),
        )
    }

    /// Scrolls table `id` so the row under window position `y` on its
    /// track is centered.
    fn table_scroll_to(&mut self, id: NodeId, y: Pixels, rows: u64, cx: &mut Context<Self>) {
        let Some(state) = self.tables.get(&id) else {
            return;
        };
        let scroll = state.scroll_handle();
        let bounds = scroll.0.borrow().base_handle.bounds();
        let row = row_at(
            f32::from(y - bounds.top()),
            f32::from(bounds.size.height),
            rows,
        );
        let ix = usize::try_from(row).unwrap_or(usize::MAX);
        scroll.scroll_to_item(ix, ScrollStrategy::Center);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_follows_the_scroll_position() {
        // 100 rows of 24 px in a 240 px viewport: a tenth is visible.
        assert_eq!(thumb(240., 100, 0.), Some((0., 24.)));
        let (top, len) = thumb(240., 100, 2160.).expect("scrollable");
        assert_eq!(top + len, 240.);
        // Everything fits: no scrollbar.
        assert_eq!(thumb(240., 5, 0.), None);
        // 100k rows: the thumb keeps its minimum size.
        assert_eq!(thumb(500., 100_000, 0.).map(|t| t.1), Some(MIN_THUMB));
    }

    #[test]
    fn track_positions_map_to_rows() {
        assert_eq!(row_at(0., 500., 100_000), 0);
        assert_eq!(row_at(250., 500., 100_000), 50_000);
        assert_eq!(row_at(900., 500., 100_000), 99_999);
        assert_eq!(row_at(10., 500., 0), 0);
    }
}
