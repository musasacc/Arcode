//! A pane: a strip of editor tabs above the active editor.

use std::collections::HashMap;
use std::path::Path;

use arcode_editor::{Editor, EditorEvent};
use arcode_ui_kit::{IconName, icon_button, theme};
use gpui::{
    App, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable, PromptLevel,
    SharedString, Subscription, Window, div, prelude::*, px,
};

use crate::actions::{
    self, CloseTab, Find, FindNext, FindPrevious, Replace, SplitRight, shortcut_label,
};
use crate::find_bar::FindBar;

mod detach;
mod find;
mod page;
mod tab;

pub use page::PageTab;

/// Height of the tab strip.
pub const TAB_BAR_HEIGHT: f32 = 34.;

/// Events emitted by a [`Pane`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneEvent {
    /// The pane (or one of its editors) received focus.
    Focused,
    /// The active editor changed, or the pane became empty.
    ActiveItemChanged,
    /// The user asked to close this (empty) pane.
    CloseRequested,
    /// A tab was closed; the editor is about to be dropped.
    ItemClosed(Entity<Editor>),
}

/// How a tab without a file (e.g. a diff) is shown and found again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabLabel {
    /// The tab text.
    pub title: SharedString,
    /// Identifies the tab for [`Pane::position_of_key`]; unlike the title
    /// it can tell apart files of the same name.
    pub key: SharedString,
    /// Shown on hover (e.g. the full path).
    pub tooltip: SharedString,
}

impl TabLabel {
    /// A label whose key and tooltip are the title.
    pub fn new(title: impl Into<SharedString>) -> Self {
        let title = title.into();
        Self {
            key: title.clone(),
            tooltip: title.clone(),
            title,
        }
    }
}

/// A group of editor tabs.
pub struct Pane {
    focus_handle: FocusHandle,
    items: Vec<Entity<Editor>>,
    active: usize,
    /// Whether the pane may be closed (there is more than one).
    closable: bool,
    subscriptions: Vec<(EntityId, Subscription)>,
    /// Tab titles for editors without a file (e.g. diff tabs).
    titles: HashMap<EntityId, TabLabel>,
    /// The find bar (created on first use) and whether it is shown.
    find: Option<(Entity<FindBar>, Subscription)>,
    find_open: bool,
    /// Non-editor tabs (the Settings page) and the active one, if a page
    /// rather than an editor is shown.
    pages: Vec<PageTab>,
    active_page: Option<usize>,
    /// Scrolls the tab bar to show the active tab.
    tab_scroll: gpui::ScrollHandle,
    _focus_sub: Option<Subscription>,
}

impl EventEmitter<PaneEvent> for Pane {}

impl Focusable for Pane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Pane {
    /// Creates an empty pane.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let sub = cx.on_focus_in(&focus_handle, window, |_, _, cx| {
            cx.emit(PaneEvent::Focused)
        });
        Self {
            focus_handle,
            items: Vec::new(),
            active: 0,
            closable: false,
            subscriptions: Vec::new(),
            titles: HashMap::new(),
            find: None,
            find_open: false,
            pages: Vec::new(),
            active_page: None,
            tab_scroll: gpui::ScrollHandle::new(),
            _focus_sub: Some(sub),
        }
    }

    /// The open editors in tab order.
    pub fn items(&self) -> &[Entity<Editor>] {
        &self.items
    }

    /// The active editor (none while a page is shown).
    pub fn active_editor(&self) -> Option<&Entity<Editor>> {
        match self.active_page {
            Some(_) => None,
            None => self.items.get(self.active),
        }
    }

    /// Sets whether the close-pane button is offered.
    pub fn set_closable(&mut self, closable: bool, cx: &mut Context<Self>) {
        if self.closable != closable {
            self.closable = closable;
            cx.notify();
        }
    }

    /// Index of the editor showing `path`.
    pub fn position_of(&self, path: &Path, cx: &App) -> Option<usize> {
        self.items
            .iter()
            .position(|e| e.read(cx).path().is_some_and(|p| p == path))
    }

    /// Index of the tab with `key` added by [`Pane::add_titled_editor`].
    pub fn position_of_key(&self, key: &str) -> Option<usize> {
        self.items.iter().position(|e| {
            self.titles
                .get(&e.entity_id())
                .is_some_and(|t| t.key == key)
        })
    }

    /// The tab title of `editor`.
    pub fn title_of(&self, editor: &Entity<Editor>, cx: &App) -> SharedString {
        self.titles
            .get(&editor.entity_id())
            .map(|t| t.title.clone())
            .unwrap_or_else(|| editor.read(cx).title())
    }

    /// Adds an editor shown under `label` (an editor without a file, such
    /// as a diff) and activates it.
    pub fn add_titled_editor(
        &mut self,
        editor: Entity<Editor>,
        label: TabLabel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.titles.insert(editor.entity_id(), label);
        self.add_editor(editor, window, cx);
    }

    /// Adds `editor` after the active tab and activates it.
    pub fn add_editor(
        &mut self,
        editor: Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sub = cx.subscribe(&editor, |_, _, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::DirtyChanged(_) | EditorEvent::Saved) {
                cx.notify();
            }
        });
        self.subscriptions.push((editor.entity_id(), sub));
        let index = if self.items.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.items.insert(index, editor);
        self.activate(index, window, cx);
    }

    /// Activates the tab at `index` and focuses its editor.
    pub fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.items.get(index) else {
            return;
        };
        self.active = index;
        self.active_page = None;
        self.tab_scroll.scroll_to_item(index);
        window.focus(&editor.focus_handle(cx));
        self.sync_find_target(cx);
        cx.emit(PaneEvent::ActiveItemChanged);
        cx.notify();
    }

    /// Focuses the active editor, or the pane itself when empty.
    pub fn focus(&self, window: &mut Window, cx: &App) {
        if let Some(page) = self.active_page() {
            return window.focus(&page.focus);
        }
        match self.active_editor() {
            Some(editor) => window.focus(&editor.focus_handle(cx)),
            None => window.focus(&self.focus_handle),
        }
    }

    /// Closes the tab at `index`, asking first if it has unsaved changes.
    pub fn close(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.items.get(index).cloned() else {
            return;
        };
        if !editor.read(cx).is_dirty() {
            self.remove(index, window, cx);
            return;
        }
        let message = format!("Save changes to {}?", editor.read(cx).title());
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some("Your changes will be lost if you don't save them."),
            &["Save", "Don't Save", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(choice) = answer.await else { return };
            if choice == 2 {
                return;
            }
            this.update_in(cx, |pane, window, cx| {
                if choice == 0
                    && let Err(err) = editor.update(cx, |e, cx| e.save(cx))
                {
                    log::error!("save failed: {err}");
                    return;
                }
                if let Some(ix) = pane.items.iter().position(|e| e == &editor) {
                    pane.remove(ix, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Closes the tab at `index` without asking, discarding unsaved
    /// changes (Vim's `:q!`).
    pub fn close_discarding(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.items.len() {
            self.remove(index, window, cx);
        }
    }

    fn remove(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.items.remove(index);
        self.subscriptions
            .retain(|(id, _)| *id != editor.entity_id());
        cx.emit(PaneEvent::ItemClosed(editor.clone()));
        self.titles.remove(&editor.entity_id());
        if self.active > index || self.active >= self.items.len() {
            self.active = self.active.saturating_sub(1);
        }
        if self.focus_handle.contains_focused(window, cx) || self.items.is_empty() {
            self.focus(window, cx);
        }
        self.sync_find_target(cx);
        cx.emit(PaneEvent::ActiveItemChanged);
        cx.notify();
    }

    fn close_active(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.active_page {
            self.close_page(ix, window, cx);
        } else if self.items.is_empty() {
            if self.closable {
                cx.emit(PaneEvent::CloseRequested);
            }
        } else {
            self.close(self.active, window, cx);
        }
    }
}

impl Render for Pane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let mut tabs: Vec<_> = self
            .items
            .clone()
            .iter()
            .enumerate()
            .map(|(ix, e)| self.render_tab(ix, e, cx).into_any_element())
            .collect();
        for (ix, page) in self.pages.clone().iter().enumerate() {
            tabs.push(self.render_page_tab(ix, page, cx).into_any_element());
        }
        let tab_bar = div()
            .flex()
            .flex_none()
            .h(px(TAB_BAR_HEIGHT))
            .bg(c.tab_inactive)
            .border_b_1()
            .border_color(c.border)
            .child(
                div()
                    .id("tabs")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_scroll)
                    .children(tabs),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_0p5()
                    .px_1()
                    .child(
                        icon_button("split-right", IconName::Columns)
                            .size(px(24.))
                            .tooltip(format!("Split right ({})", shortcut_label("secondary-\\")))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(SplitRight), cx)
                            }),
                    )
                    .when(self.closable && !self.has_tabs(), |el| {
                        el.child(
                            icon_button("close-pane", IconName::X)
                                .size(px(24.))
                                .tooltip("Close pane")
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(PaneEvent::CloseRequested)),
                                ),
                        )
                    }),
            );
        let page = self.active_page().map(|p| p.view.clone());
        let body = match self.active_editor() {
            _ if page.is_some() => div().flex_1().min_h_0().children(page).into_any_element(),
            Some(editor) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .when_some(
                    self.find
                        .as_ref()
                        .filter(|_| self.find_open)
                        .map(|(bar, _)| bar.clone()),
                    |el, bar| el.child(bar),
                )
                .child(div().flex_1().min_h_0().child(editor.clone()))
                .into_any_element(),
            None => div()
                .flex_1()
                .min_h_0()
                .child(self.render_empty(cx))
                .into_any_element(),
        };
        div()
            .key_context(actions::PANE_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::close_active))
            .on_action(cx.listener(|pane, _: &Find, window, cx| pane.open_find(false, window, cx)))
            .on_action(
                cx.listener(|pane, _: &Replace, window, cx| pane.open_find(true, window, cx)),
            )
            .on_action(cx.listener(|pane, _: &FindNext, _, cx| pane.find_step(true, cx)))
            .capture_action(cx.listener(|pane, _: &arcode_editor::Cancel, _, cx| {
                // Escape in the editor also closes the find bar (the editor
                // still gets the action).
                pane.close_find(cx);
            }))
            .on_action(cx.listener(|pane, _: &FindPrevious, _, cx| pane.find_step(false, cx)))
            .on_any_mouse_down(cx.listener(|pane, _, window, _| {
                if pane.items.is_empty() {
                    window.focus(&pane.focus_handle);
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(c.background)
            .child(tab_bar)
            .child(body)
    }
}
