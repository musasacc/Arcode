//! A pane: a strip of editor tabs above the active editor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use arcode_editor::{Editor, EditorEvent};
use gpui::{
    App, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable, PromptLevel,
    SharedString, Subscription, Window, div, prelude::*,
};

use crate::actions::{self, CloseTab, Find, FindNext, FindPrevious, Replace};
use crate::find_bar::FindBar;

pub(crate) mod breadcrumb;
mod close_save;
mod detach;
mod disk_bar;
mod find;
mod page;
#[cfg(test)]
mod review_tabs;
mod strip;
mod tab;

pub use disk_bar::{CompareWithDisk, KeepMyChanges, ReloadFromDisk};
pub use page::PageTab;

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
    /// Tabs whose close waits for a save refused over a changed file.
    close_after_save: Vec<(EntityId, Subscription)>,
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
    /// The project root (breadcrumb paths are relative to it).
    root: Option<PathBuf>,
    /// The active editor's outline for the breadcrumb.
    crumbs: breadcrumb::Crumbs,
    /// The pane is wide enough for the breadcrumb (measured when drawn).
    wide: bool,
    /// The tabs overflow the strip (a hairline then ends the tab area).
    tabs_overflow: bool,
    /// The list of open tabs under the `…` button is shown.
    tab_menu_open: bool,
    /// Whether the workspace frames this pane (the ember mark on its
    /// active tab); `None` until told, then focus decides.
    framed: Option<bool>,
    /// The active tab the strip last scrolled into view and the strip's
    /// bounds then (see `strip::Pane::reveal_active_tab`).
    scrolled_to: Option<(usize, gpui::Bounds<gpui::Pixels>)>,
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
            close_after_save: Vec::new(),
            titles: HashMap::new(),
            find: None,
            find_open: false,
            pages: Vec::new(),
            active_page: None,
            tab_scroll: gpui::ScrollHandle::new(),
            root: None,
            crumbs: Default::default(),
            wide: true,
            tabs_overflow: false,
            tab_menu_open: false,
            framed: None,
            scrolled_to: None,
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

    /// Sets whether this pane is the framed panel's active pane (read at
    /// the next render, which follows in the same frame: no notify).
    pub fn set_framed(&mut self, framed: bool) {
        self.framed = Some(framed);
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
            if matches!(
                event,
                EditorEvent::DirtyChanged(_)
                    | EditorEvent::Saved
                    | EditorEvent::DiskConflictChanged
                    | EditorEvent::PathChanged
            ) {
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
                    if !pane.close_on_overwrite(&editor, window, cx) {
                        log::error!("save failed: {err}");
                    }
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
        self.close_after_save
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_bar = self.render_tab_strip(window, cx);
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
                .children(self.render_disk_bar(editor, cx))
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
            .on_key_down(cx.listener(|pane, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && pane.tab_menu_open {
                    pane.tab_menu_open = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            // No fill: the editor panel's rounded fill shows through.
            .size_full()
            .flex()
            .flex_col()
            .child(tab_bar)
            .child(body)
    }
}
