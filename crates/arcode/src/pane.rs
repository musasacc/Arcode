//! A pane: a strip of editor tabs above the active editor.

use std::path::Path;

use arcode_editor::{Editor, EditorEvent};
use arcode_ui_kit::{IconName, Tooltip, icon, icon_button, kbd, theme};
use gpui::{
    AnyView, App, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable, MouseButton,
    PromptLevel, SharedString, Subscription, Window, div, prelude::*, px,
};

use crate::actions::{self, CloseTab, SplitRight, shortcut_label};
use crate::file_icons::file_icon;

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
        let sub = cx.on_focus_in(&focus_handle, window, |_, _, cx| cx.emit(PaneEvent::Focused));
        Self {
            focus_handle,
            items: Vec::new(),
            active: 0,
            closable: false,
            subscriptions: Vec::new(),
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

    /// Adds `editor` after the active tab and activates it.
    pub fn add_editor(&mut self, editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        let sub = cx.subscribe(&editor, |_, _, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::DirtyChanged(_) | EditorEvent::Saved) {
                cx.notify();
            }
        });
        self.subscriptions.push((editor.entity_id(), sub));
        let index = if self.items.is_empty() { 0 } else { self.active + 1 };
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
        self.subscriptions.retain(|(id, _)| *id != editor.entity_id());
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

    fn render_tab(&self, index: usize, editor: &Entity<Editor>, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let e = editor.read(cx);
        let title = e.title();
        let dirty = e.is_dirty();
        let tooltip: SharedString = e
            .path()
            .map(|p| p.display().to_string().into())
            .unwrap_or_else(|| title.clone());
        let active = index == self.active;
        let (icon_name, icon_color) = file_icon(&title, &c);
        let group = SharedString::from(format!("tab-{}", editor.entity_id()));
        let close_button = div()
            .id(("close-tab", index))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .hover(|s| s.bg(c.hover))
            .when(!active, |el| el.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)))
            .when(dirty, |el| el.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)))
            .child(icon(IconName::X).size(px(12.)).color(c.text_muted))
            .on_click(cx.listener(move |pane, _, window, cx| {
                cx.stop_propagation();
                pane.close(index, window, cx);
            }));
        let dirty_dot = div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .group_hover(group.clone(), |s| s.opacity(0.))
            .child(div().size(px(8.)).rounded_full().bg(c.accent_alt));
        div()
            .id(("tab", index))
            .group(group)
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
            .tooltip(move |_, cx| -> AnyView { Tooltip::text(tooltip.clone(), cx) })
            .child(icon(icon_name).size(px(14.)).color(icon_color))
            .child(div().whitespace_nowrap().child(title))
            .child(
                div()
                    .relative()
                    .size(px(18.))
                    .flex_none()
                    .when(dirty, |el| el.child(dirty_dot))
                    .child(close_button),
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
                cx.listener(move |pane, _, window, cx| pane.close(index, window, cx)),
            )
            .on_click(cx.listener(move |pane, _, window, cx| pane.activate(index, window, cx)))
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
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(SplitRight), cx)),
                    )
                    .when(self.closable && self.items.is_empty(), |el| {
                        el.child(
                            icon_button("close-pane", IconName::X)
                                .size(px(24.))
                                .tooltip("Close pane")
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(PaneEvent::CloseRequested))),
                        )
                    }),
            );
        let body = match self.active_editor() {
            Some(editor) => div().flex_1().min_h_0().child(editor.clone()).into_any_element(),
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
