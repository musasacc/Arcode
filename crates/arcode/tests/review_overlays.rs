//! Review tests for the overlays lane (docs/DESIGN.md 6.11): the palette,
//! the file finder and Go to symbol put their rows in a `uniform_list`
//! inside `picker_list` (they used to pad the `uniform_list` itself with
//! `.p(px(LIST_PAD))`, which GPUI 0.2.2 applies twice at the top).
//! `picker_frame` promises `LIST_PAD` (6 px) above the first row and below
//! the last one, as in dark_debug_palette.png, and the list must not jump
//! when the keyboard selects a row that is already fully visible.

use std::sync::Arc;

use arcode::picker_frame::{LIST_PAD, ROW_H, picker_list, picker_row};
use arcode_config::{Config, ThemeRegistry};
use gpui::{
    Context, IntoElement, Render, ScrollStrategy, TestAppContext, UniformListScrollHandle, Window,
    div, prelude::*, px, uniform_list,
};

const MAX_VISIBLE_ROWS: usize = 10;

/// The list of the palette, built as `CommandPalette::render` builds it.
struct PaletteList {
    count: usize,
    scroll: UniformListScrollHandle,
}

impl Render for PaletteList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = uniform_list(
            "palette-results",
            self.count,
            cx.processor(|_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|ix| {
                        picker_row(ix, ix == 0, cx)
                            .debug_selector(move || format!("row-{ix}"))
                            .child(format!("Row {ix}"))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone());
        div()
            .size_full()
            .child(picker_list(list, self.count, MAX_VISIBLE_ROWS).debug_selector(|| "list".into()))
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(Config::default(), cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
    });
}

fn bounds(cx: &mut gpui::VisualTestContext, sel: &'static str) -> gpui::Bounds<gpui::Pixels> {
    cx.debug_bounds(sel)
        .unwrap_or_else(|| panic!("{sel} was not drawn"))
}

#[gpui::test]
fn rows_sit_list_pad_from_both_ends(cx: &mut TestAppContext) {
    init(cx);
    let (_view, cx) = cx.add_window_view(|_, _| PaletteList {
        count: 3,
        scroll: UniformListScrollHandle::new(),
    });
    cx.run_until_parked();
    let list = bounds(cx, "list");
    let first = bounds(cx, "row-0");
    let last = bounds(cx, "row-2");
    assert_eq!(list.size.height, px(3. * ROW_H + 2. * LIST_PAD));
    assert_eq!(
        first.top() - list.top(),
        px(LIST_PAD),
        "space above the first row"
    );
    assert_eq!(
        list.bottom() - last.bottom(),
        px(LIST_PAD),
        "space below the last row (it touches the footer's hairline)"
    );
}

#[gpui::test]
fn selecting_a_fully_visible_row_does_not_scroll(cx: &mut TestAppContext) {
    init(cx);
    let scroll = UniformListScrollHandle::new();
    let (view, cx) = cx.add_window_view({
        let scroll = scroll.clone();
        move |_, _| PaletteList { count: 16, scroll }
    });
    cx.run_until_parked();
    let list = bounds(cx, "list");
    let row9 = bounds(cx, "row-9");
    assert!(
        row9.bottom() <= list.bottom(),
        "the 10th row is fully visible before the move"
    );
    // What `select_next` does when Down reaches the 10th row.
    scroll.scroll_to_item(9, ScrollStrategy::Top);
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let moved = bounds(cx, "row-9");
    assert_eq!(
        moved.top(),
        row9.top(),
        "the list jumped by {:?} although the row was visible",
        row9.top() - moved.top()
    );
}
