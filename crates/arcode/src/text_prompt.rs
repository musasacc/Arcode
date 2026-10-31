//! A one-line text dialog (title, input, hint): Enter confirms, Escape
//! cancels. Used to edit the run command.

use arcode_ui_kit::{ButtonStyle, IconName, button, icon, input_frame, theme};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    SharedString, Window, actions, div, prelude::*, px,
};

use crate::picker_frame::{dialog_buttons, dialog_frame, dialog_title};
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let focused = self.input.focus_handle(cx).is_focused(window);
        let buttons = [
            button("text-prompt-cancel", "Cancel")
                .style(ButtonStyle::Ghost)
                .on_click(cx.listener(|p, _, window, cx| p.dismiss(&Dismiss, window, cx)))
                .into_any_element(),
            button("text-prompt-ok", "OK")
                .style(ButtonStyle::Primary)
                .on_click(cx.listener(|p, _, window, cx| p.confirm(&Confirm, window, cx)))
                .into_any_element(),
        ];
        dialog_frame(px(560.), cx)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .child(dialog_title(self.title.clone(), cx))
            .child(
                div().px(px(16.)).child(
                    input_frame(focused, true, cx)
                        .child(icon(self.glyph).size(px(15.)).color(c.accent))
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
                        ),
                ),
            )
            .child(dialog_buttons(
                Some(self.hint.clone().into_any_element()),
                buttons,
                cx,
            ))
    }
}
