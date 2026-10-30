//! An in-app "type a folder path" dialog, used when the platform folder
//! picker is unavailable (e.g. Linux without an XDG desktop portal).

use std::path::PathBuf;

use arcode_ui_kit::{IconName, icon, kbd, theme};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, SharedString,
    Subscription, Window, actions, div, prelude::*, px,
};

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
        let path = expand_home(&text);
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

/// Expands a leading `~` to the home folder.
pub fn expand_home(text: &str) -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest.trim_start_matches(['/', '\\'])),
        _ => PathBuf::from(text),
    }
}

impl Render for PathPrompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .w(px(520.))
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .shadow_md()
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
                    .child(icon(IconName::Folder).size(px(16.)).color(c.accent))
                    .child(self.input.clone())
                    .child(kbd("Enter")),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_color(c.border)
                    .text_xs()
                    .text_color(if self.error.is_some() {
                        c.error
                    } else {
                        c.text_muted
                    })
                    .child(self.error.clone().unwrap_or_else(|| {
                        "The system folder picker is unavailable; type a path instead.".into()
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_home() {
        assert_eq!(expand_home("/tmp"), PathBuf::from("/tmp"));
        if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
            assert_eq!(expand_home("~/x"), PathBuf::from(home).join("x"));
        }
    }
}
