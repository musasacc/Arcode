//! The pane's tab strip (docs/DESIGN.md 6.6): pill tabs on the panel with
//! a hairline under them, then the breadcrumb, Split and the tab list.
//!
//! Tabs keep their size and scroll horizontally when they do not fit; the
//! breadcrumb gives way from the left and hides in narrow panes.

use arcode_ui_kit::metrics::{CONTROL_H_PANEL, TAB_STRIP_H};
use arcode_ui_kit::{
    IconName, icon, icon_button, list_row, popover, theme, truncated, unsaved_ring,
};
use gpui::{
    AnyElement, Context, Pixels, Window, anchored, canvas, deferred, div, prelude::*, px,
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
            tabs.push(self.render_page_tab(ix, page, focused, cx).into_any_element());
        }
        // Activating a tab scrolls it into view with the strip's width of
        // the frame before; the breadcrumb may have changed width since, so
        // scroll once more on the next frame.
        let active_tab = match self.active_page {
            Some(page) => self.items.len() + page,
            None => self.active,
        };
        if self.scrolled_to != Some(active_tab) {
            self.scrolled_to = Some(active_tab);
            cx.on_next_frame(window, move |pane, _, cx| {
                pane.tab_scroll.scroll_to_item(active_tab);
                cx.notify();
            });
        }
        let breadcrumb = if self.wide {
            self.render_breadcrumb(cx)
        } else {
            None
        };
        let c = theme(cx).colors.clone();
        // Measures the strip so the breadcrumb can hide in narrow panes; a
        // new frame is asked for only when the answer flips.
        let pane = cx.entity().downgrade();
        let wide = self.wide;
        let measure = canvas(
            move |bounds, _, cx| {
                let now = shows_breadcrumb(bounds.size.width);
                if now != wide {
                    pane.update(cx, |pane, cx| {
                        pane.wide = now;
                        cx.notify();
                    })
                    .ok();
                }
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
                    .child(div().flex_1().min_w_0().overflow_hidden().child(truncated(title)))
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

    #[test]
    fn the_breadcrumb_hides_in_narrow_panes() {
        assert!(shows_breadcrumb(px(520.)));
        assert!(shows_breadcrumb(px(1200.)));
        assert!(!shows_breadcrumb(px(519.)));
        assert!(!shows_breadcrumb(px(0.)));
    }
}
