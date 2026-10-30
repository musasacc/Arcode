//! Signature help: asked for after `(` and `,` (or the provider's own
//! trigger characters) and on Ctrl+Shift+Space, updated while typing inside
//! the call, and shown above the cursor with the active parameter
//! highlighted.

use std::time::Duration;

use gpui::{AppContext, Context, Entity, Task};

use super::signature_view::SignatureView;
use super::{SIGNATURE_POPOVER, SignatureHelp, SignatureRequest};
use crate::Editor;
use crate::ext::EditorPopover;

/// Signature help state of one editor.
#[derive(Default)]
pub(crate) struct SignatureState {
    pub help: Option<SignatureHelp>,
    /// Signature shown (the user can cycle through overloads).
    pub shown: usize,
    pub seq: u64,
    pub task: Option<Task<()>>,
    pub view: Option<Entity<SignatureView>>,
}

impl Editor {
    /// Asks for signature help at the cursor now (Ctrl+Shift+Space).
    pub fn show_signature_help(&mut self, cx: &mut Context<Self>) {
        self.request_signature(None, Duration::ZERO, cx);
    }

    /// Whether signature help is shown.
    pub fn signature_help_visible(&self) -> bool {
        self.completion.signature.help.is_some()
    }

    /// The signature help shown, if any.
    pub fn signature_help(&self) -> Option<&SignatureHelp> {
        self.completion.signature.help.as_ref()
    }

    /// Closes signature help.
    pub fn hide_signature_help(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.completion.signature;
        state.help = None;
        state.task = None;
        state.seq += 1;
        self.hide_popover(SIGNATURE_POPOVER, cx);
    }

    /// Opens signature help after a trigger character, or updates it while
    /// it is shown.
    pub(crate) fn signature_typed(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(provider) = self.completion.provider.clone() else {
            return;
        };
        let triggers = provider.signature_triggers(self, cx);
        let open = self.signature_help_visible();
        let delay = Duration::from_millis(super::settings(cx).delay_ms);
        if triggers.trigger.iter().any(|t| t == text)
            || (open && triggers.retrigger.iter().any(|t| t == text))
        {
            self.request_signature(Some(text.to_string()), delay, cx);
        }
    }

    /// Updates shown signature help after the cursor moved or the text
    /// changed (the active parameter may differ).
    pub(crate) fn signature_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.signature_help_visible() {
            return;
        }
        if self.selections.len() != 1 {
            self.hide_signature_help(cx);
            return;
        }
        let delay = Duration::from_millis(super::settings(cx).delay_ms);
        self.request_signature(None, delay, cx);
    }

    fn request_signature(
        &mut self,
        trigger_character: Option<String>,
        delay: Duration,
        cx: &mut Context<Self>,
    ) {
        if self.completion.provider.is_none() {
            return;
        }
        self.completion.signature.seq += 1;
        let id = self.completion.signature.seq;
        self.completion.signature.task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let task = this
                .update(cx, |editor, cx| {
                    let provider = editor.completion.provider.clone()?;
                    let request = SignatureRequest {
                        trigger_character,
                        is_retrigger: editor.signature_help_visible(),
                        active: editor.completion.signature.help.clone(),
                    };
                    provider.signature_help(editor, &request, cx)
                })
                .ok()
                .flatten();
            let help = match task {
                Some(task) => task.await.unwrap_or_else(|err| {
                    log::debug!("signature help failed: {err:#}");
                    None
                }),
                None => None,
            };
            this.update(cx, |editor, cx| editor.signature_result(id, help, cx))
                .ok();
        }));
    }

    /// Shows the answer to request `id` (older answers are dropped).
    pub(crate) fn signature_result(
        &mut self,
        id: u64,
        help: Option<SignatureHelp>,
        cx: &mut Context<Self>,
    ) {
        if id != self.completion.signature.seq {
            return;
        }
        self.completion.signature.task = None;
        let Some(help) = help.filter(|h| !h.signatures.is_empty()) else {
            self.hide_signature_help(cx);
            return;
        };
        let Some(cursor) = self.single_cursor() else {
            self.hide_signature_help(cx);
            return;
        };
        let state = &mut self.completion.signature;
        let same = state.help.as_ref().map(|h| h.signatures.len()) == Some(help.signatures.len());
        if !same {
            state.shown = help.active_signature.min(help.signatures.len() - 1);
        }
        state.help = Some(help);
        self.signature_push(cx);
        if !self.has_popover(SIGNATURE_POPOVER) {
            let view = self.completion.signature.view.clone().expect("pushed");
            let at = self.offset_to_point(cursor);
            let popover = EditorPopover::new(at, view.into())
                .above()
                .on_key(|editor, ks, _, cx| editor.update(cx, |e, cx| e.signature_key(ks, cx)));
            self.show_popover(SIGNATURE_POPOVER, popover, cx);
        }
    }

    fn signature_push(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.completion.signature;
        let Some(help) = state.help.clone() else {
            return;
        };
        let shown = state.shown;
        let view = state
            .view
            .get_or_insert_with(|| cx.new(|_| SignatureView::default()))
            .clone();
        view.update(cx, |v, cx| v.set_help(help, shown, cx));
    }

    /// Escape closes; up/down cycle overloads while no list is shown.
    fn signature_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let m = &ks.modifiers;
        if m.control || m.alt || m.platform || m.shift {
            return false;
        }
        match ks.key.as_str() {
            "escape" => {
                self.hide_signature_help(cx);
                true
            }
            "up" | "down" if !self.completion_visible() => {
                let count = self
                    .completion
                    .signature
                    .help
                    .as_ref()
                    .map_or(0, |h| h.signatures.len());
                if count < 2 {
                    return false;
                }
                let state = &mut self.completion.signature;
                state.shown = if ks.key == "down" {
                    (state.shown + 1) % count
                } else {
                    (state.shown + count - 1) % count
                };
                self.signature_push(cx);
                true
            }
            _ => false,
        }
    }
}
