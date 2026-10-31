//! Form widgets: `button`, `input`, `select`, `checkbox`.
//!
//! Inputs are uncontrolled (§7): the text lives in a [`TextField`] (or, for
//! a multi-line input with a `language`, a small highlighted [`Editor`]).
//! An input counts as edited while its text differs from the plugin's last
//! `value`; edited texts ride along with the next event. `notify-changes`
//! inputs also send a debounced `change`.

use std::time::Duration;

use arcode_buffer::{Buffer, Point};
use arcode_editor::{Editor, EditorEvent};
use arcode_plugin_host::bindings::NodeId;
use arcode_plugin_host::bindings::ui::{ButtonProps, CheckboxProps, EventKind, InputProps, Widget};
use arcode_ui_kit::{ButtonStyle, IconName, button, icon, theme};
use gpui::{
    AnyElement, App, AppContext, Context, Entity, Focusable, KeyDownEvent, MouseButton,
    SharedString, Subscription, Task, Window, div, prelude::*, px,
};

use super::PanelView;
use crate::plugins::commands::icon_for;
use crate::text_field::{TextChanged, TextField};

/// `change` events wait this long after the last keystroke.
const CHANGE_DEBOUNCE: Duration = Duration::from_millis(300);

/// Where an input's text lives.
pub(super) enum InputKind {
    Field(Entity<TextField>),
    Code(Entity<Editor>),
}

/// One input's host-side state.
pub(super) struct InputState {
    pub(super) kind: InputKind,
    /// The plugin's last `value`.
    plugin_value: String,
    /// The text differs from `plugin_value` and was not sent yet.
    pub(super) edited: bool,
    change_task: Option<Task<()>>,
    _sub: Subscription,
}

impl InputState {
    pub(super) fn text(&self, cx: &App) -> String {
        match &self.kind {
            InputKind::Field(f) => f.read(cx).text().to_owned(),
            InputKind::Code(e) => e.read(cx).buffer().text().to_string(),
        }
    }

    fn set_text(&mut self, text: &str, cx: &mut App) {
        self.plugin_value = text.to_owned();
        self.edited = false;
        match &self.kind {
            InputKind::Field(f) => f.update(cx, |f, cx| f.set_text(text, cx)),
            InputKind::Code(e) => e.update(cx, |e, cx| {
                let last = e.line_count().saturating_sub(1);
                let end = Point::new(last, e.buffer().line_len(last));
                e.apply_edits(vec![(Point::new(0, 0)..end, text.to_owned())], cx);
            }),
        }
    }
}

fn input_props(view: &PanelView, id: NodeId) -> Option<InputProps> {
    match view.snapshot.node(id).map(|n| &n.widget) {
        Some(Widget::Input(p)) => Some(p.clone()),
        _ => None,
    }
}

impl PanelView {
    /// Texts of inputs edited since the previous event (and marks them
    /// sent).
    pub(super) fn take_values(&mut self, cx: &App) -> Vec<(NodeId, String)> {
        let mut values: Vec<(NodeId, String)> = self
            .inputs
            .iter_mut()
            .filter(|(_, s)| s.edited)
            .map(|(id, s)| {
                s.edited = false;
                (*id, s.text(cx))
            })
            .collect();
        values.sort_by_key(|(id, _)| *id);
        values
    }

    /// The plugin set a new `value`: it replaces what the user typed.
    pub(super) fn input_changed_by_plugin(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let Some(props) = input_props(self, id) else {
            return;
        };
        if let Some(state) = self.inputs.get_mut(&id)
            && state.text(cx) != props.value
        {
            state.set_text(&props.value, cx);
        } else if let Some(state) = self.inputs.get_mut(&id) {
            state.plugin_value = props.value;
            state.edited = false;
        }
    }

    fn input_edited(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let Some(state) = self.inputs.get_mut(&id) else {
            return;
        };
        let text = state.text(cx);
        state.edited = text != state.plugin_value;
        if !input_props(self, id).is_some_and(|p| p.notify_changes) {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CHANGE_DEBOUNCE).await;
            this.update(cx, |view, cx| {
                let Some(state) = view.inputs.get_mut(&id) else {
                    return;
                };
                let text = state.text(cx);
                state.edited = false;
                view.send(id, EventKind::Change(text), cx);
            })
            .ok();
        });
        if let Some(state) = self.inputs.get_mut(&id) {
            state.change_task = Some(task);
        }
    }

    fn create_input(&mut self, id: NodeId, p: &InputProps, cx: &mut Context<Self>) {
        let (kind, sub) = if p.multiline && p.language.is_some() {
            let language = p
                .language
                .as_deref()
                .and_then(arcode_syntax::language_by_id);
            let editor = cx.new(|cx| {
                let mut e = Editor::new(Buffer::new(&p.value), cx);
                if let Some(language) = language {
                    e.set_language(language, cx);
                }
                e
            });
            let sub = cx.subscribe(&editor, move |view, _, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Edited { .. }) {
                    view.input_edited(id, cx);
                }
            });
            (InputKind::Code(editor), sub)
        } else {
            let placeholder = p.placeholder.clone();
            let field = cx.new(|cx| {
                let mut f = if p.multiline {
                    TextField::multiline(placeholder, 3, 12, cx)
                } else {
                    TextField::new(placeholder, cx)
                };
                f.set_text(&p.value, cx);
                f
            });
            let sub = cx.subscribe(&field, move |view, _, _: &TextChanged, cx| {
                view.input_edited(id, cx)
            });
            (InputKind::Field(field), sub)
        };
        self.inputs.insert(
            id,
            InputState {
                kind,
                plugin_value: p.value.clone(),
                edited: false,
                change_task: None,
                _sub: sub,
            },
        );
    }

    pub(super) fn render_input(
        &mut self,
        id: NodeId,
        p: &InputProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.inputs.contains_key(&id) {
            self.create_input(id, p, cx);
        }
        let c = theme(cx).colors.clone();
        let Some(state) = self.inputs.get(&id) else {
            return div().into_any_element();
        };
        let multiline = p.multiline;
        let submit = p.submit_on_enter;
        let (content, focus) = match &state.kind {
            InputKind::Field(f) => (
                div()
                    .flex_1()
                    .px_2()
                    .py_1()
                    .child(f.clone())
                    .into_any_element(),
                f.focus_handle(cx),
            ),
            InputKind::Code(e) => {
                // An empty editor shows the placeholder over its text area.
                let empty = e.read(cx).buffer().len_chars() == 0;
                let el = div()
                    .relative()
                    .size_full()
                    .child(e.clone())
                    .when(empty, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top(px(2.))
                                .left(px(60.))
                                .text_color(c.text_muted)
                                .child(SharedString::from(p.placeholder.clone())),
                        )
                    });
                (el.into_any_element(), e.focus_handle(cx))
            }
        };
        let focused = focus.contains_focused(window, cx);
        let code = matches!(state.kind, InputKind::Code(_));
        let click_focus = focus.clone();
        div()
            .id(("input", id))
            .flex()
            .w_full()
            .when(code, |el| el.h(px(150.)))
            .when(!code && !multiline, |el| el.h(px(28.)).items_center())
            .rounded_md()
            .border_1()
            .border_color(if focused { c.border_focused } else { c.border })
            .bg(c.surface)
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                window.focus(&click_focus)
            })
            .capture_key_down(cx.listener(move |view, e: &KeyDownEvent, _, cx| {
                let k = &e.keystroke;
                if k.key != "enter" || !submit {
                    return;
                }
                let wanted = if multiline {
                    k.modifiers.secondary()
                } else {
                    !k.modifiers.modified()
                };
                if wanted {
                    cx.stop_propagation();
                    view.send(id, EventKind::Submit, cx);
                }
            }))
            .child(content)
            .into_any_element()
    }

    pub(super) fn render_button(
        &mut self,
        id: NodeId,
        p: &ButtonProps,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !p.enabled {
            let c = theme(cx).colors.clone();
            return div()
                .flex()
                .flex_none()
                .items_center()
                .gap_1p5()
                .h(px(28.))
                .px_3()
                .rounded_md()
                .bg(c.surface_raised)
                .text_color(c.text_disabled)
                .when_some(p.icon.as_deref(), |el, name| {
                    el.child(
                        icon(icon_for(Some(name)))
                            .size(px(14.))
                            .color(c.text_disabled),
                    )
                })
                .child(SharedString::from(p.label.clone()))
                .into_any_element();
        }
        let mut b = button(("button", id), p.label.clone())
            .style(if p.primary {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            })
            .on_click(cx.listener(move |view, _, _, cx| view.send(id, EventKind::Click, cx)));
        if let Some(name) = p.icon.as_deref() {
            b = b.icon(icon_for(Some(name)));
        }
        div().flex_none().child(b).into_any_element()
    }

    pub(super) fn render_checkbox(
        &mut self,
        id: NodeId,
        p: &CheckboxProps,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        let checked = *self.checks.get(&id).unwrap_or(&p.checked);
        div()
            .id(("checkbox", id))
            .flex()
            .flex_none()
            .items_center()
            .gap_1p5()
            .cursor_pointer()
            .child(
                div()
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .border_1()
                    .border_color(if checked { c.accent } else { c.border })
                    .when(checked, |el| el.bg(c.accent))
                    .when(checked, |el| {
                        el.child(icon(IconName::Check).size(px(12.)).color(c.accent_text))
                    }),
            )
            .child(SharedString::from(p.label.clone()))
            .on_click(cx.listener(move |view, _, _, cx| {
                view.checks.insert(id, !checked);
                cx.notify();
                view.send(id, EventKind::Toggle(!checked), cx);
            }))
            .into_any_element()
    }
}
