//! The pane's tab strip (docs/DESIGN.md 6.6): pill tabs on the panel with
//! a hairline under them, then the breadcrumb, Split and the tab list.
//!
//! Tabs keep their size and scroll horizontally when they do not fit; the
//! breadcrumb gives way from the left and hides in narrow panes.
//!
//! The active tab is scrolled into view from the strip's own last layout
//! (its bounds and the tabs' bounds of the same frame), again whenever the
//! active tab or the strip's bounds change, so a strip laid out first at
//! another size or scale never stays scrolled past it. Scrolling the strip
//! by hand changes neither, so it is left alone.

use arcode_ui_kit::metrics::{CONTROL_H_PANEL, TAB_STRIP_H};
use arcode_ui_kit::{
    IconName, icon, icon_button, list_row, popover, theme, truncated, unsaved_ring,
};
use gpui::{
    AnyElement, Bounds, Context, Pixels, Window, anchored, canvas, deferred, div, point,
    prelude::*, px,
};

use super::{Pane, PaneEvent};
use crate::actions::{SplitRight, shortcut_label};
use crate::file_icons::file_icon;

/// Below this pane width the breadcrumb is hidden.
pub const BREADCRUMB_MIN_PANE_WIDTH: Pixels = px(520.);

/// Whether a pane `width` wide shows the breadcrumb.
pub fn shows_breadcrumb(width: Pixels) -> bool {
    width >= BREADCRUMB_MIN_PANE_WIDTH
}

/// Whether tabs whose scroll range is `max_offset` wide overflow the
/// strip, so a hairline marks where the clipped tab area ends.
pub fn tabs_overflow(max_offset: Pixels) -> bool {
    max_offset >= px(1.)
}

/// The horizontal scroll offset (0 or below, down to `-max`) that shows a
/// tab laid out at `tab` in a strip at `strip`, when the strip is now
/// scrolled by `offset`; `None` when the tab is in view already. A tab
/// wider than the strip is shown from its left edge.
pub fn reveal_offset(
    strip: Bounds<Pixels>,
    tab: Bounds<Pixels>,
    offset: Pixels,
    max: Pixels,
) -> Option<Pixels> {
    let (left, right) = (tab.left() + offset, tab.right() + offset);
    let target = if tab.size.width >= strip.size.width || left < strip.left() {
        strip.left() - tab.left()
    } else if right > strip.right() {
        strip.right() - tab.right()
    } else {
        return None;
    };
    let target = target.clamp(-max.max(px(0.)), px(0.));
    ((target - offset).abs() >= px(0.5)).then_some(target)
}

impl Pane {
    pub(super) fn render_tab_strip(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focused = self
            .framed
            .unwrap_or_else(|| self.focus_handle.contains_focused(window, cx));
        let mut tabs: Vec<AnyElement> = self
            .items
            .clone()
            .iter()
            .enumerate()
            .map(|(ix, e)| self.render_tab(ix, e, focused, cx).into_any_element())
            .collect();
        for (ix, page) in self.pages.clone().iter().enumerate() {
            tabs.push(
                self.render_page_tab(ix, page, focused, cx)
                    .into_any_element(),
            );
        }
        let active_tab = match self.active_page {
            Some(page) => self.items.len() + page,
            None => self.active,
        };
        let breadcrumb = if self.wide {
            self.render_breadcrumb(cx)
        } else {
            None
        };
        let c = theme(cx).colors.clone();
        // Measures the strip so the breadcrumb can hide in narrow panes,
        // marks overflowing tabs and keeps the active tab in view; a new
        // frame is asked for only when one of them changes something.
        let pane = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                let now = shows_breadcrumb(bounds.size.width);
                pane.update(cx, |pane, cx| {
                    if now != pane.wide {
                        pane.wide = now;
                        cx.notify();
                    }
                    let overflow = tabs_overflow(pane.tab_scroll.max_offset().width);
                    if overflow != pane.tabs_overflow {
                        pane.tabs_overflow = overflow;
                        cx.notify();
                    }
                    if pane.reveal_active_tab(active_tab) {
                        cx.notify();
                    }
                })
                .ok();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .h(TAB_STRIP_H)
            .px(px(6.))
            .border_b_1()
            .border_color(c.border)
            .child(measure)
            .child(
                div()
                    .id("tabs")
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_scroll)
                    .children(tabs),
            )
            // Clipped tabs end at a hairline, not straight into the
            // breadcrumb or the buttons.
            .when(self.tabs_overflow, |el| {
                el.child(
                    div()
                        .flex_none()
                        .w(px(1.))
                        .h(px(16.))
                        .ml(px(6.))
                        .bg(c.border),
                )
            })
            .children(breadcrumb)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .ml(px(6.))
                    .child(
                        icon_button("split-right", IconName::Columns)
                            .size(CONTROL_H_PANEL)
                            .tooltip(format!("Split right ({})", shortcut_label("secondary-\\")))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(SplitRight), cx)
                            }),
                    )
                    .when(self.has_tabs(), |el| {
                        el.child(
                            div()
                                .relative()
                                .child(
                                    icon_button("tab-list", IconName::Ellipsis)
                                        .size(CONTROL_H_PANEL)
                                        .active(self.tab_menu_open)
                                        .tooltip("Open tabs")
                                        .on_click(cx.listener(|pane, _, _, cx| {
                                            pane.tab_menu_open = !pane.tab_menu_open;
                                            cx.notify();
                                        })),
                                )
                                .children(self.render_tab_menu(cx)),
                        )
                    })
                    .when(self.closable && !self.has_tabs(), |el| {
                        el.child(
                            icon_button("close-pane", IconName::X)
                                .size(CONTROL_H_PANEL)
                                .tooltip("Close pane")
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(PaneEvent::CloseRequested)),
                                ),
                        )
                    }),
            )
    }

    /// Scrolls the tab at `ix` into view when it or the strip's bounds
    /// changed since the last time, using the layout of the frame before
    /// (strip and tabs measured together). Returns whether to draw once
    /// more: to confirm the new layout, or because the tab is not laid out
    /// yet.
    fn reveal_active_tab(&mut self, ix: usize) -> bool {
        if ix >= self.items.len() + self.pages.len() {
            return false;
        }
        let strip = self.tab_scroll.bounds();
        if self.scrolled_to == Some((ix, strip)) {
            return false;
        }
        let Some(tab) = self.tab_scroll.bounds_for_item(ix) else {
            return true;
        };
        self.scrolled_to = Some((ix, strip));
        let offset = self.tab_scroll.offset();
        let max = self.tab_scroll.max_offset().width;
        if let Some(x) = reveal_offset(strip, tab, offset.x, max) {
            self.tab_scroll.set_offset(point(x, offset.y));
        }
        true
    }

    /// The list of open tabs under the `…` button: the way to reach a tab
    /// scrolled out of view.
    fn render_tab_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.tab_menu_open || !self.has_tabs() {
            return None;
        }
        let c = theme(cx).colors.clone();
        let mut list = popover(cx)
            .id("tab-menu")
            .min_w(px(220.))
            .max_w(px(360.))
            .max_h(px(420.))
            .overflow_y_scroll()
            .occlude()
            .on_mouse_down_out(cx.listener(|pane, _, _, cx| {
                pane.tab_menu_open = false;
                cx.notify();
            }));
        for (ix, editor) in self.items.iter().enumerate() {
            let title = self.title_of(editor, cx);
            let e = editor.read(cx);
            let dirty = e.is_dirty() && !self.titles.contains_key(&editor.entity_id());
            let (glyph, tint) = if self.titles.contains_key(&editor.entity_id()) {
                (IconName::GitCompare, c.accent_alt)
            } else {
                file_icon(&title, &c)
            };
            let selected = self.active_page.is_none() && ix == self.active;
            list = list.child(
                list_row(("tab-menu-item", ix), selected, cx)
                    .mx(px(0.))
                    .h(px(26.))
                    .child(icon(glyph).size(px(15.)).color(tint))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(truncated(title)),
                    )
                    .when(dirty, |el| el.child(unsaved_ring(None, cx)))
                    .on_click(cx.listener(move |pane, _, window, cx| {
                        pane.tab_menu_open = false;
                        pane.activate(ix, window, cx);
                    })),
            );
        }
        for (ix, page) in self.pages.iter().enumerate() {
            let selected = self.active_page == Some(ix);
            list = list.child(
                list_row(("tab-menu-page", ix), selected, cx)
                    .mx(px(0.))
                    .h(px(26.))
                    .child(icon(page.icon).size(px(15.)).color(c.text_muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(truncated(page.title.clone())),
                    )
                    .on_click(cx.listener(move |pane, _, window, cx| {
                        pane.tab_menu_open = false;
                        pane.activate_page(ix, window, cx);
                    })),
            );
        }
        // Hangs under the button, right edges aligned.
        Some(
            div()
                .absolute()
                .top(CONTROL_H_PANEL + px(4.))
                .right_0()
                .child(
                    deferred(
                        anchored()
                            .anchor(gpui::Corner::TopRight)
                            .snap_to_window_with_margin(px(8.))
                            .child(list),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(left: f32, width: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(left), px(0.)), gpui::size(px(width), px(26.)))
    }

    #[test]
    fn the_active_tab_is_scrolled_into_view() {
        let strip = b(100., 300.);
        // In view: left alone.
        assert_eq!(reveal_offset(strip, b(100., 80.), px(0.), px(200.)), None);
        assert_eq!(
            reveal_offset(strip, b(450., 80.), px(-200.), px(200.)),
            None
        );
        // Cut at the right: scrolled so its right edge meets the strip's.
        assert_eq!(
            reveal_offset(strip, b(360., 80.), px(0.), px(200.)),
            Some(px(-40.))
        );
        // Cut at the left (scrolled too far, e.g. from a layout at another
        // scale): scrolled back so its left edge meets the strip's.
        assert_eq!(
            reveal_offset(strip, b(100., 80.), px(-10.), px(10.)),
            Some(px(0.))
        );
        assert_eq!(
            reveal_offset(strip, b(200., 80.), px(-150.), px(200.)),
            Some(px(-100.))
        );
        // Never past the end of the tabs.
        assert_eq!(
            reveal_offset(strip, b(500., 80.), px(0.), px(120.)),
            Some(px(-120.))
        );
        // Wider than the strip: shown from its left edge, once.
        assert_eq!(
            reveal_offset(strip, b(150., 400.), px(0.), px(300.)),
            Some(px(-50.))
        );
        assert_eq!(
            reveal_offset(strip, b(150., 400.), px(-50.), px(300.)),
            None
        );
    }

    #[test]
    fn overflow_needs_a_whole_pixel_of_scroll_range() {
        assert!(!tabs_overflow(px(0.)));
        assert!(!tabs_overflow(px(0.5)));
        assert!(tabs_overflow(px(1.)));
        assert!(tabs_overflow(px(240.)));
    }

    #[test]
    fn the_breadcrumb_hides_in_narrow_panes() {
        assert!(shows_breadcrumb(px(520.)));
        assert!(shows_breadcrumb(px(1200.)));
        assert!(!shows_breadcrumb(px(519.)));
        assert!(!shows_breadcrumb(px(0.)));
    }
}
