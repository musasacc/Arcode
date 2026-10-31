//! A one-line text dialog (title, input, hint): Enter confirms, Escape
//! cancels. Used to edit the run command.

use arcode_ui_kit::Elevated;
use arcode_ui_kit::{IconName, icon, kbd, theme};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    SharedString, Window, actions, div, prelude::*, px,
};

use crate::text_field::TextField;

actions!(text_prompt, [Confirm, Dismiss]);

const CONTEXT: &str = "TextPrompt";

/// Registers Enter/Escape for the dialog.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm, Some(CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
    ]);
}

/// What the dialog decided.
pub enum TextPromptEvent {
    /// The entered text (trimmed, not empty).
    Confirmed(String),
    /// Cancelled.
    Dismissed,
}

/// The dialog view.
pub struct TextPrompt {
    title: SharedString,
    hint: SharedString,
    glyph: IconName,
    input: Entity<TextField>,
}

impl EventEmitter<TextPromptEvent> for TextPrompt {}

impl Focusable for TextPrompt {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl TextPrompt {
    /// A dialog with `initial` text (selected, so typing replaces it).
    pub fn new(
        title: impl Into<SharedString>,
        hint: impl Into<SharedString>,
        glyph: IconName,
        initial: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut field = TextField::new("", cx);
            field.set_text(initial, cx);
            field.select_all_text(cx);
            field
        });
        Self {
            title: title.into(),
            hint: hint.into(),
            glyph,
            input,
        }
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().trim().to_string();
        if !text.is_empty() {
            cx.emit(TextPromptEvent::Confirmed(text));
        }
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextPromptEvent::Dismissed);
    }
}

impl Render for TextPrompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .w(px(560.))
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .elevated(cx)
            .overflow_hidden()
            .child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .text_sm()
                    .text_color(c.text_muted)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(40.))
                    .px_3()
                    .child(icon(self.glyph).size(px(16.)).color(c.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            // The field fills the height: without a flex
                            // row of the row's height it is zero tall.
                            .h_full()
                            .flex()
                            .overflow_hidden()
                            .child(self.input.clone()),
                    )
                    .child(kbd("Enter")),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_color(c.border)
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(self.hint.clone()),
            )
    }
}
