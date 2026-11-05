//! Page tabs: views that are not editors (the Settings page) shown as a
//! tab of the pane. Pages come after the editor tabs; at most one page per
//! key is open in a pane. A page with unsaved work (the theme editor)
//! registers a close guard, asked before its tab closes.

use std::collections::HashMap;
use std::rc::Rc;

use arcode_ui_kit::{IconName, pill_tab};
use gpui::{
    AnyView, App, Context, EntityId, FocusHandle, Global, MouseButton, SharedString, Window,
    prelude::*,
};

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

/// Asked before a page's tab closes: `true` lets it close now; `false`
/// keeps it open (the page asks the user and closes itself later).
pub type CloseGuard = Rc<dyn Fn(&mut Window, &mut App) -> bool>;

/// The close guards, by the page view's entity.
#[derive(Default)]
struct CloseGuards(HashMap<EntityId, CloseGuard>);

impl Global for CloseGuards {}

/// Sets (or, with `None`, removes) the close guard of the page whose view
/// is `view`.
pub fn set_close_guard(view: EntityId, guard: Option<CloseGuard>, cx: &mut App) {
    let guards = &mut cx.default_global::<CloseGuards>().0;
    match guard {
        Some(guard) => guards.insert(view, guard),
        None => guards.remove(&view),
    };
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
        self.sync_find_target(cx);
        cx.emit(PaneEvent::ActiveItemChanged);
        cx.notify();
    }

    /// Closes the page at `ix` unless its close guard keeps it open; the
    /// editor tab that was active before is shown again.
    pub fn close_page(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.pages.get(ix) else {
            return;
        };
        let guard = cx
            .try_global::<CloseGuards>()
            .and_then(|g| g.0.get(&page.view.entity_id()))
            .cloned();
        if guard.is_some_and(|guard| !guard(window, cx)) {
            return;
        }
        self.close_page_now(ix, window, cx);
    }

    /// Closes the page at `ix` without asking its close guard.
    pub fn close_page_now(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
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
        focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.active_page == Some(ix);
        pill_tab(
            SharedString::from(format!("page-tab-{}", page.key)),
            page.title.clone(),
        )
        .icon(page.icon, None)
        .active(active)
        .focused(focused)
        .on_click(cx.listener(move |pane, _, window, cx| pane.activate_page(ix, window, cx)))
        .on_close(cx.listener(move |pane, _, window, cx| pane.close_page(ix, window, cx)))
        .build(cx)
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |pane, _, window, cx| pane.close_page(ix, window, cx)),
        )
    }
}
