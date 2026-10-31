//! A small filterable list for quick choices: the language, line ending
//! and indentation pickers of the status bar. Uses the finder's key
//! bindings (up/down/enter/escape).

use arcode_ui_kit::{IconName, icon, theme, truncated};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, ScrollHandle,
    SharedString, Subscription, Window, div, point, prelude::*, px,
};

use crate::finder::{Confirm, Dismiss, SelectNext, SelectPrev};
use crate::picker_frame::{
    self, LIST_PAD, ROW_H, count_status, empty_state, footer, input_row, picker_frame, picker_row,
    row_detail, row_label,
};
use crate::text_field::{TextChanged, TextField};

const ROW_HEIGHT: f32 = ROW_H;
const MAX_ROWS: usize = 12;
/// Padding above the first row (and below the last) of the list.
const LIST_PADDING: f32 = LIST_PAD;

/// The scroll offset (negative, as GPUI stores it) that shows `row` fully
/// in a list of fixed-height rows scrolled to `offset`, moving as little
/// as possible.
pub fn offset_showing(offset: f32, row: usize) -> f32 {
    picker_frame::offset_showing(offset, row, MAX_ROWS)
}

/// One choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickItem {
    /// Main text, matched by the filter.
    pub label: SharedString,
    /// Muted text after the label (also matched).
    pub detail: SharedString,
    /// Marked as the current value.
    pub current: bool,
}

impl PickItem {
    /// An item with a label and a detail.
    pub fn new(label: impl Into<SharedString>, detail: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            current: false,
        }
    }

    /// Marks the item as the current value.
    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }
}

/// What the user did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListPickerEvent {
    /// Chose the item at this index of the original list.
    Picked(usize),
    /// The selection (arrow keys, hover or a new filter) moved to the item
    /// at this index of the original list; for live previews.
    Highlighted(usize),
    /// Closed without a choice.
    Dismissed,
}

/// Indices of `items` whose label or detail contains every word of
/// `query` (case-insensitive), in list order.
pub fn filter(items: &[PickItem], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let text = format!("{} {}", item.label, item.detail).to_lowercase();
            words.iter().all(|w| text.contains(w))
        })
        .map(|(ix, _)| ix)
        .collect()
}

/// The picker view.
pub struct ListPicker {
    input: Entity<TextField>,
    icon: IconName,
    items: Vec<PickItem>,
    shown: Vec<usize>,
    selected: usize,
    scroll: ScrollHandle,
    _sub: Subscription,
}

impl EventEmitter<ListPickerEvent> for ListPicker {}

impl Focusable for ListPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl ListPicker {
    /// A picker over `items` with a search field showing `placeholder`.
    /// The current item is selected first.
    pub fn new(
        placeholder: impl Into<SharedString>,
        icon: IconName,
        items: Vec<PickItem>,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextField::new(placeholder, cx));
        let sub = cx.subscribe(&input, |p, _, _: &TextChanged, cx| p.refilter(cx));
        let shown: Vec<usize> = (0..items.len()).collect();
        let selected = items.iter().position(|i| i.current).unwrap_or(0);
        let picker = Self {
            input,
            icon,
            items,
            shown,
            selected,
            scroll: ScrollHandle::new(),
            _sub: sub,
        };
        picker.reveal_selected();
        picker
    }

    /// The selected row (an index into [`Self::shown`]).
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The list's scroll offset (tests).
    pub fn scroll_offset(&self) -> f32 {
        f32::from(self.scroll.offset().y)
    }

    /// Scrolls the list so the selected row is in view.
    fn reveal_selected(&self) {
        let offset = f32::from(self.scroll.offset().y);
        let wanted = offset_showing(offset, self.selected);
        if wanted != offset {
            self.scroll.set_offset(point(px(0.), px(wanted)));
        }
    }

    /// The items currently listed (indices into the original list).
    pub fn shown(&self) -> &[usize] {
        &self.shown
    }

    /// Types `text` into the search field (tests).
    pub fn set_query(&mut self, text: &str, cx: &mut Context<Self>) {
        self.input.update(cx, |f, cx| f.set_text(text, cx));
        self.refilter(cx);
    }

    /// Chooses the selected row (Enter).
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        self.choose(self.selected, cx);
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let shown = filter(&self.items, self.input.read(cx).text());
        if shown != self.shown {
            self.shown = shown;
            self.selected = 0;
            self.reveal_selected();
            self.emit_highlight(cx);
            cx.notify();
        }
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.shown.len() as isize;
        if len > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
            self.reveal_selected();
            self.emit_highlight(cx);
            cx.notify();
        }
    }

    fn emit_highlight(&self, cx: &mut Context<Self>) {
        if let Some(ix) = self.shown.get(self.selected) {
            cx.emit(ListPickerEvent::Highlighted(*ix));
        }
    }

    /// Selects `row` (the mouse rests on it).
    fn hover_row(&mut self, row: usize, cx: &mut Context<Self>) {
        if row != self.selected && row < self.shown.len() {
            self.selected = row;
            self.emit_highlight(cx);
            cx.notify();
        }
    }

    fn choose(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(ix) = self.shown.get(row) {
            cx.emit(ListPickerEvent::Picked(*ix));
        }
    }

    fn render_row(&self, row: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let item = &self.items[self.shown[row]];
        let selected = row == self.selected;
        picker_row(("pick-item", row), selected, cx)
            .h(px(ROW_HEIGHT))
            .child(div().w(px(15.)).flex_none().when(item.current, |el| {
                el.child(icon(IconName::Check).size(px(15.)).color(c.accent))
            }))
            .child(row_label(
                truncated(item.label.clone()),
                !item.detail.is_empty(),
            ))
            .child(row_detail(truncated(item.detail.clone()), cx))
            .on_hover(cx.listener(move |p, hovered: &bool, _, cx| {
                if *hovered {
                    p.hover_row(row, cx)
                }
            }))
            .on_click(cx.listener(move |p, _, _, cx| p.choose(row, cx)))
    }
}

impl Render for ListPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<_> = (0..self.shown.len())
            .map(|row| self.render_row(row, cx).into_any_element())
            .collect();
        let empty = rows.is_empty();
        let status = count_status(None, self.shown.len(), self.items.len());
        picker_frame(px(480.), cx)
            .key_context(crate::finder::CONTEXT)
            .on_action(cx.listener(|p, _: &SelectPrev, _, cx| p.step(-1, cx)))
            .on_action(cx.listener(|p, _: &SelectNext, _, cx| p.step(1, cx)))
            .on_action(cx.listener(|p, _: &Confirm, _, cx| p.confirm(cx)))
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(ListPickerEvent::Dismissed)))
            .child(input_row(self.icon, None, self.input.clone(), None, cx))
            .when(empty, |el| el.child(empty_state("No matches", cx)))
            .when(!empty, |el| {
                el.child(
                    div()
                        .id("pick-list")
                        .track_scroll(&self.scroll)
                        .p(px(LIST_PADDING))
                        .max_h(px(ROW_HEIGHT * MAX_ROWS as f32 + 2. * LIST_PADDING))
                        .overflow_y_scroll()
                        .children(rows),
                )
            })
            .child(footer("choose", Some(status), cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_by_every_word_in_label_or_detail() {
        let items = [
            PickItem::new("Rust", "rust"),
            PickItem::new("TypeScript React", "typescriptreact"),
            PickItem::new("JavaScript React", "javascriptreact"),
        ];
        assert_eq!(filter(&items, ""), [0, 1, 2]);
        assert_eq!(filter(&items, "react"), [1, 2]);
        assert_eq!(filter(&items, "type REACT"), [1]);
        assert!(filter(&items, "cobol").is_empty());
    }

    #[test]
    fn scrolls_as_little_as_needed_to_show_a_row() {
        // Rows 0..12 fit without scrolling.
        assert_eq!(offset_showing(0., 0), 0.);
        assert_eq!(offset_showing(0., MAX_ROWS - 1), 0.);
        // Row 12 becomes the last visible row.
        assert_eq!(offset_showing(0., MAX_ROWS), -ROW_HEIGHT);
        assert_eq!(offset_showing(0., 20), -9. * ROW_HEIGHT);
        // Going back up shows the row at the top.
        assert_eq!(offset_showing(-9. * ROW_HEIGHT, 3), -3. * ROW_HEIGHT);
        assert_eq!(offset_showing(-9. * ROW_HEIGHT, 0), 0.);
        // Already visible: no change.
        assert_eq!(offset_showing(-9. * ROW_HEIGHT, 15), -9. * ROW_HEIGHT);
    }
}
