//! Consent prompts in the workspace (`plugins::consent`): requests from
//! the app's `ConsentProvider` (and `PluginOutput::Consent`, should a
//! manager use the host's channel provider) are queued and shown one at a
//! time in the modal consent dialog. Every answer goes to the plugin log.
//!
//! GPUI shows one prompt per window. A consent dialog waits while one of
//! the app's prompts is open, and one replaced by an app prompt (an
//! unsaved-changes question, say) counts as dismissed: the plugin's call
//! ends, the next request is shown once the app prompt is answered, and
//! the focus goes back where it was before the consent dialog.

use arcode_plugin_host::cancel::{CancelToken, ReplySender};
use arcode_plugin_host::consent::{ConsentAnswer, ConsentRequest};
use gpui::{AppContext, Context, FocusHandle, Window};

use super::Workspace;
use crate::plugins::consent::{ConsentAsk, describe, model};
use crate::plugins::consent_dialog::{self, ARM_DELAY, ConsentDialog};

impl Workspace {
    /// Starts showing the prompts of the app's consent provider.
    pub(super) fn plugin_consent_loop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(asks) = self.plugins.client.take_consents() else {
            return;
        };
        let task = cx.spawn_in(window, async move |this, cx| {
            while let Ok(ask) = asks.recv().await {
                let alive = this.update_in(cx, |ws, window, cx| ws.plugin_ask(ask, window, cx));
                if alive.is_err() {
                    break;
                }
            }
        });
        self.plugins.tasks.push(task);
    }

    /// A `PluginOutput::Consent` (the host's channel provider): shown like
    /// the provider's prompts.
    pub(super) fn plugin_consent(
        &mut self,
        request: ConsentRequest,
        reply: ReplySender<ConsentAnswer>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = ConsentAsk {
            request,
            cancel: CancelToken::new(),
            reply,
        };
        self.plugin_ask(ask, window, cx);
    }

    /// Queues `ask`; it is shown once no other prompt is.
    pub(super) fn plugin_ask(
        &mut self,
        ask: ConsentAsk,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.plugins.consent_queue.push_back(ask);
        self.plugin_next_consent(window, cx);
    }

    /// Shows the next queued prompt whose call still waits.
    fn plugin_next_consent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.plugins.prompts_open > 0 || self.plugins.consent_queue.is_empty() {
            return;
        }
        if self.plugin_after_app_prompt(None, window, cx) {
            return;
        }
        let ask = loop {
            match self.plugins.consent_queue.pop_front() {
                // Nobody waits for it any more: drop it (dismissed).
                Some(ask) if ask.cancel.is_cancelled() => continue,
                Some(ask) => break ask,
                None => return,
            }
        };
        let ConsentAsk {
            request,
            cancel,
            reply,
        } = ask;
        let source = self
            .plugins
            .client
            .manager()
            .and_then(|m| m.registry().get(&request.plugin).map(|p| p.source));
        let dialog_model = model(&request, source);
        let arm = if cfg!(test) {
            std::time::Duration::ZERO
        } else {
            ARM_DELAY
        };
        let dialog = cx.new(|cx| ConsentDialog::new(dialog_model, Some(reply), cancel, arm, cx));
        let answer = dialog.read(cx).answer_slot();
        let focus_before = window.focused(cx);
        let closed = consent_dialog::show(&dialog, window, cx);
        // Only the prompt layer holds the dialog: replaced, it is dropped
        // (answering "dismissed") and `closed` resolves.
        drop(dialog);
        self.plugins.prompts_open += 1;
        let asked = request.message();
        cx.spawn_in(window, async move |this, cx| {
            closed.await;
            this.update_in(cx, |ws, window, cx| {
                let given = answer.get();
                let shown = given.unwrap_or(ConsentAnswer::Dismissed);
                ws.plugins.log(format!("{asked} -> {}", describe(shown)));
                ws.plugins.prompts_open = ws.plugins.prompts_open.saturating_sub(1);
                ws.plugins_page_refresh(cx);
                if given.is_none() {
                    ws.plugin_after_app_prompt(focus_before, window, cx);
                }
                ws.plugin_next_consent(window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// If one of the app's prompts is open, waits for it to close, then
    /// puts the focus back on `focus` (when nothing has it) and shows the
    /// next consent request. True while it waits.
    fn plugin_after_app_prompt(
        &mut self,
        focus: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(prompt) = crate::prompt::open_in(window, cx) else {
            return false;
        };
        let mut focus = focus;
        cx.observe_release_in(&prompt, window, move |ws, _, window, cx| {
            if let Some(focus) = focus.take()
                && window.focused(cx).is_none()
            {
                window.focus(&focus);
            }
            ws.plugin_next_consent(window, cx);
        })
        .detach();
        true
    }
}
