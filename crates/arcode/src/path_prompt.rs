//! An in-app "type a folder path" dialog, used when the platform folder
//! picker is unavailable (e.g. Linux without an XDG desktop portal).

use std::path::PathBuf;

use arcode_ui_kit::{ButtonStyle, IconName, button, icon, input_frame, theme};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, SharedString,
    Subscription, Window, actions, div, prelude::*, px,
};

use crate::picker_frame::{dialog_buttons, dialog_frame, dialog_title};
use crate::text_field::{TextChanged, TextField};

actions!(path_prompt, [Confirm, Dismiss]);

const CONTEXT: &str = "PathPrompt";

/// Registers Enter/Escape for the dialog.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm, Some(CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
    ]);
}

/// Events emitted by the dialog.
pub enum PathPromptEvent {
    /// An existing folder was entered.
    Confirmed(PathBuf),
    /// The dialog was cancelled.
    Dismissed,
}

/// The dialog view.
pub struct PathPrompt {
    title: SharedString,
    input: Entity<TextField>,
    error: Option<SharedString>,
    _subscription: Subscription,
}

impl EventEmitter<PathPromptEvent> for PathPrompt {}

impl Focusable for PathPrompt {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl PathPrompt {
    /// Creates the dialog with `initial` as the starting path.
    pub fn new(title: impl Into<SharedString>, initial: &str, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            let mut field = TextField::new("Folder path", cx);
            field.set_text(initial, cx);
            field
        });
        let sub = cx.subscribe(&input, |prompt, _, _: &TextChanged, cx| {
            if prompt.error.take().is_some() {
                cx.notify();
            }
        });
        Self {
            title: title.into(),
            input,
            error: None,
            _subscription: sub,
        }
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().trim().to_string();
        let path = expand_home(typed_path(&text));
        if path.is_dir() {
            cx.emit(PathPromptEvent::Confirmed(path));
        } else {
            self.error = Some(format!("{} is not a folder", path.display()).into());
            cx.notify();
        }
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(PathPromptEvent::Dismissed);
    }
}

/// The path the user meant when they typed an absolute path or `~` after
/// the prefilled folder: `/home/me//tmp/x` is `/tmp/x`, `/home/me/~/x` is
/// `~/x`. A leading `//` (a UNC path) is kept.
pub fn typed_path(text: &str) -> &str {
    let mut start = text
        .match_indices("//")
        .filter(|(i, _)| *i > 0)
        .last()
        .map_or(0, |(i, _)| i + 1);
    if let Some(i) = text[start..].rfind("/~") {
        start += i + 1;
    }
    &text[start..]
}

/// Expands a leading `~` to the home folder.
pub fn expand_home(text: &str) -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest.trim_start_matches(['/', '\\'])),
        _ => PathBuf::from(text),
    }
}

impl Render for PathPrompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let focused = self.input.focus_handle(cx).is_focused(window);
        let hint = div()
            .text_color(if self.error.is_some() {
                c.error
            } else {
                c.text_muted
            })
            .child(self.error.clone().unwrap_or_else(|| {
                "The system folder picker is unavailable; type a path instead.".into()
            }));
        let buttons = [
            button("path-prompt-cancel", "Cancel")
                .style(ButtonStyle::Ghost)
                .on_click(cx.listener(|p, _, window, cx| p.dismiss(&Dismiss, window, cx)))
                .into_any_element(),
            button("path-prompt-open", "Open")
                .style(ButtonStyle::Primary)
                .on_click(cx.listener(|p, _, window, cx| p.confirm(&Confirm, window, cx)))
                .into_any_element(),
        ];
        dialog_frame(px(520.), cx)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .child(dialog_title(self.title.clone(), cx))
            .child(
                div().px(px(16.)).child(
                    input_frame(focused, true, cx)
                        .child(icon(IconName::Folder).size(px(15.)).color(c.accent))
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
            .child(dialog_buttons(Some(hint.into_any_element()), buttons, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_a_path_after_the_prefill_restarts_it() {
        assert_eq!(typed_path("/home/me/"), "/home/me/");
        assert_eq!(typed_path("/home/me//tmp/x"), "/tmp/x");
        assert_eq!(typed_path("/home/me/~/x"), "~/x");
        assert_eq!(typed_path("//server/share"), "//server/share");
        assert_eq!(typed_path("~/a"), "~/a");
    }

    #[test]
    fn expands_home() {
        assert_eq!(expand_home("/tmp"), PathBuf::from("/tmp"));
        if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
            assert_eq!(expand_home("~/x"), PathBuf::from(home).join("x"));
        }
    }
}
