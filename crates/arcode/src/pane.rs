//! A pane: a strip of editor tabs above the active editor.

use std::collections::HashMap;
use std::path::Path;

use arcode_editor::{Editor, EditorEvent};
use arcode_ui_kit::{IconName, icon_button, kbd, theme};
use gpui::{
    App, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable, PromptLevel,
    SharedString, Subscription, Window, div, prelude::*, px,
};

use crate::actions::{self, CloseTab, SplitRight, shortcut_label};

mod tab;

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
    titles: HashMap<EntityId, SharedString>,
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
            _focus_sub: Some(sub),
        }
    }

    /// The open editors in tab order.
    pub fn items(&self) -> &[Entity<Editor>] {
        &self.items
    }

    /// The active editor.
    pub fn active_editor(&self) -> Option<&Entity<Editor>> {
        self.items.get(self.active)
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

    /// Index of the tab titled `title` by [`Pane::add_titled_editor`].
    pub fn position_of_title(&self, title: &str) -> Option<usize> {
        self.items
            .iter()
            .position(|e| self.titles.get(&e.entity_id()).is_some_and(|t| t == title))
    }

    /// The tab title of `editor`.
    pub fn title_of(&self, editor: &Entity<Editor>, cx: &App) -> SharedString {
        self.titles
            .get(&editor.entity_id())
            .cloned()
            .unwrap_or_else(|| editor.read(cx).title())
    }

    /// Adds an editor shown under `title` (an editor without a file, such
    /// as a diff) and activates it.
    pub fn add_titled_editor(
        &mut self,
        editor: Entity<Editor>,
        title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.titles.insert(editor.entity_id(), title);
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
        window.focus(&editor.focus_handle(cx));
        cx.emit(PaneEvent::ActiveItemChanged);
        cx.notify();
    }

    /// Focuses the active editor, or the pane itself when empty.
    pub fn focus(&self, window: &mut Window, cx: &App) {
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

    fn remove(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.items.remove(index);
        self.subscriptions
            .retain(|(id, _)| *id != editor.entity_id());
        self.titles.remove(&editor.entity_id());
        if self.active > index || self.active >= self.items.len() {
            self.active = self.active.saturating_sub(1);
        }
        if self.focus_handle.contains_focused(window, cx) || self.items.is_empty() {
            self.focus(window, cx);
        }
        cx.emit(PaneEvent::ActiveItemChanged);
        cx.notify();
    }

    fn close_active(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            if self.closable {
                cx.emit(PaneEvent::CloseRequested);
            }
        } else {
            self.close(self.active, window, cx);
        }
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let hint = |label: &'static str, keys: &str| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_6()
                .w(px(240.))
                .child(label)
                .child(kbd(shortcut_label(keys)))
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .text_sm()
            .text_color(c.text_muted)
            .child(hint("Find file", "secondary-p"))
            .child(hint("Split editor", "secondary-\\"))
            .child(hint("Toggle sidebar", "secondary-b"))
            .child(hint("Toggle panel", "secondary-j"))
    }
}

impl Render for Pane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let tabs: Vec<_> = self
            .items
            .clone()
            .iter()
            .enumerate()
            .map(|(ix, e)| self.render_tab(ix, e, cx).into_any_element())
            .collect();
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
                    .when(self.closable && self.items.is_empty(), |el| {
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
        let body = match self.active_editor() {
            Some(editor) => div()
                .flex_1()
                .min_h_0()
                .child(editor.clone())
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
