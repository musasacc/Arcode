//! Page tabs: views that are not editors (the Settings page) shown as a
//! tab of the pane. Pages come after the editor tabs; at most one page per
//! key is open in a pane.

use arcode_ui_kit::{IconName, icon, theme};
use gpui::{AnyView, Context, FocusHandle, MouseButton, SharedString, Window, div, prelude::*, px};

use super::{Pane, PaneEvent};

/// A non-editor tab.
#[derive(Clone)]
pub struct PageTab {
    /// Identifies the page (e.g. `"settings"`).
    pub key: SharedString,
    /// The tab text.
    pub title: SharedString,
    /// The tab icon.
    pub icon: IconName,
    /// The page's view.
    pub view: AnyView,
    /// Focused when the page is activated.
    pub focus: FocusHandle,
}

impl Pane {
    /// The open pages in tab order.
    pub fn pages(&self) -> &[PageTab] {
        &self.pages
    }

    /// Whether the pane has any tab (editor or page).
    pub fn has_tabs(&self) -> bool {
        !self.items.is_empty() || !self.pages.is_empty()
    }

    /// The active page, when a page (not an editor) is active.
    pub fn active_page(&self) -> Option<&PageTab> {
        self.active_page.and_then(|ix| self.pages.get(ix))
    }

    /// Index of the page with `key`.
    pub fn page_position(&self, key: &str) -> Option<usize> {
        self.pages.iter().position(|p| p.key == key)
    }

    /// Opens `page` (or activates the page with its key) and focuses it.
    pub fn add_page(&mut self, page: PageTab, window: &mut Window, cx: &mut Context<Self>) {
        let ix = match self.page_position(&page.key) {
            Some(ix) => ix,
            None => {
                self.pages.push(page);
                self.pages.len() - 1
            }
        };
        self.activate_page(ix, window, cx);
    }

    /// Retitles the page with `key` (e.g. a plugin panel's `set-title`).
    pub fn set_page_title(&mut self, key: &str, title: SharedString, cx: &mut Context<Self>) {
        if let Some(page) = self.pages.iter_mut().find(|p| p.key == key)
            && page.title != title
        {
            page.title = title;
            cx.notify();
        }
    }

    /// Activates the page at `ix`.
    pub fn activate_page(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.pages.get(ix) else {
            return;
        };
        window.focus(&page.focus);
        self.active_page = Some(ix);
        self.tab_scroll.scroll_to_item(self.items.len() + ix);
        self.sync_find_target(cx);
        cx.emit(PaneEvent::ActiveItemChanged);
        cx.notify();
    }

    /// Closes the page at `ix`; the editor tab that was active before is
    /// shown again.
    pub fn close_page(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.pages.len() {
            return;
        }
        self.pages.remove(ix);
        self.active_page = match self.active_page {
            Some(active) if active == ix => None,
            Some(active) if active > ix => Some(active - 1),
            other => other,
        };
        if self.active_page.is_none() {
            self.focus(window, cx);
        }
        self.sync_find_target(cx);
        cx.emit(PaneEvent::ActiveItemChanged);
        cx.notify();
    }

    pub(super) fn render_page_tab(
        &self,
        ix: usize,
        page: &PageTab,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let active = self.active_page == Some(ix);
        let group = SharedString::from(format!("page-tab-{}", page.key));
        div()
            .id(SharedString::from(format!("page-tab-{}", page.key)))
            .group(group.clone())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap_1p5()
            .h_full()
            .pl_3()
            .pr_1p5()
            .border_r_1()
            .border_color(c.border)
            .cursor_pointer()
            .bg(if active { c.tab_active } else { c.tab_inactive })
            .text_color(if active { c.text } else { c.text_muted })
            .when(!active, |el| el.hover(|s| s.bg(c.hover)))
            .child(icon(page.icon).size(px(14.)).color(c.accent))
            .child(page.title.clone())
            .child(
                div()
                    .id(SharedString::from(format!("close-page-{}", page.key)))
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .hover(|s| s.bg(c.hover))
                    .when(!active, |el| {
                        el.opacity(0.).group_hover(group, |s| s.opacity(1.))
                    })
                    .child(icon(IconName::X).size(px(12.)).color(c.text_muted))
                    .on_click(cx.listener(move |pane, _, window, cx| {
                        cx.stop_propagation();
                        pane.close_page(ix, window, cx);
                    })),
            )
            .when(active, |el| {
                el.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(2.))
                        .bg(c.accent),
                )
            })
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |pane, _, window, cx| pane.close_page(ix, window, cx)),
            )
            .on_click(cx.listener(move |pane, _, window, cx| pane.activate_page(ix, window, cx)))
    }
}
