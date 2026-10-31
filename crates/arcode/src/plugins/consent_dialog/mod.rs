//! The consent dialog: a window-modal prompt for one [`ConsentAsk`]
//! (docs/PLUGINS.md §4 "Prompts are host chrome").
//!
//! It is shown through GPUI's prompt layer ([`show`]), so it covers the
//! whole window, takes the keyboard (no shortcut of the workspace reaches
//! anything while it is open) and gives focus back when it closes. Keys:
//! Tab / Shift+Tab and the arrow keys move between the controls, Enter or
//! Space presses the focused one, Esc dismisses (denied this time, asked
//! again next time). The answer buttons only take effect [`ARM_DELAY`]
//! after the dialog appears, so a click or Enter meant for what was under
//! it (the Send button that started the request) cannot answer it.
//!
//! The dialog sends the answer itself; it closes on its own when the
//! plugin's call is cancelled (nobody waits for the answer any more). Key
//! repeats never answer it: a key still held down from before it appeared
//! (Enter after typing, Ctrl+Enter "Send") must not press "Allow once".
//! If it goes away unanswered (another prompt replaced it), dropping it
//! answers "dismissed", so the plugin never waits for a dialog that is gone.
//!
//! [`ConsentAsk`]: super::consent::ConsentAsk

mod render;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use arcode_plugin_host::cancel::{CancelToken, ReplySender};
use arcode_plugin_host::consent::ConsentAnswer;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, PromptLevel,
    PromptResponse, Task, Window,
};

use super::consent::{Choice, DialogModel};

/// How long the answer buttons ignore clicks and keys after the dialog
/// appears.
pub const ARM_DELAY: Duration = Duration::from_millis(500);

/// How often an open dialog checks whether its call was cancelled.
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// A focusable control of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// The "also allow changing files" checkbox.
    Write,
    /// An answer button.
    Answer(Choice),
}

/// The dialog view.
pub struct ConsentDialog {
    model: DialogModel,
    focus: FocusHandle,
    focused: Control,
    write: bool,
    armed: bool,
    reply: Option<ReplySender<ConsentAnswer>>,
    /// The answer, shared with whoever waits for the dialog to close.
    answer: Rc<Cell<Option<ConsentAnswer>>>,
    _tasks: Vec<Task<()>>,
}

impl EventEmitter<PromptResponse> for ConsentDialog {}

impl Focusable for ConsentDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ConsentDialog {
    /// A dialog answering `reply`, armed after `arm` (zero in tests), that
    /// closes itself once `cancel` fires.
    pub fn new(
        model: DialogModel,
        reply: Option<ReplySender<ConsentAnswer>>,
        cancel: CancelToken,
        arm: Duration,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut tasks = Vec::new();
        if !arm.is_zero() {
            tasks.push(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(arm).await;
                this.update(cx, |d, cx| {
                    d.armed = true;
                    cx.notify();
                })
                .ok();
            }));
        }
        tasks.push(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CANCEL_POLL).await;
                if cancel.is_cancelled() {
                    this.update(cx, |d, cx| d.finish(ConsentAnswer::Dismissed, cx))
                        .ok();
                    return;
                }
            }
        }));
        Self {
            model,
            focus: cx.focus_handle(),
            focused: Control::Answer(Choice::Once),
            write: false,
            armed: arm.is_zero(),
            reply,
            answer: Rc::default(),
            _tasks: tasks,
        }
    }

    /// The model shown.
    pub fn model(&self) -> &DialogModel {
        &self.model
    }

    /// The answer given, once closed.
    pub fn answer(&self) -> Option<ConsentAnswer> {
        self.answer.get()
    }

    /// The answer as it will be given, readable after the dialog is gone.
    pub fn answer_slot(&self) -> Rc<Cell<Option<ConsentAnswer>>> {
        self.answer.clone()
    }

    /// The focused control.
    pub fn focused(&self) -> Control {
        self.focused
    }

    /// Whether the write checkbox is ticked.
    pub fn write(&self) -> bool {
        self.write
    }

    /// The controls in focus order.
    pub fn controls(&self) -> Vec<Control> {
        let mut out = Vec::with_capacity(5);
        if self.model.write_option {
            out.push(Control::Write);
        }
        out.extend(Choice::ALL.map(Control::Answer));
        out
    }

    /// Moves the focus by `delta` controls (wrapping).
    pub fn move_focus(&mut self, delta: isize, cx: &mut Context<Self>) {
        let controls = self.controls();
        let at = controls
            .iter()
            .position(|c| *c == self.focused)
            .unwrap_or(0) as isize;
        let n = controls.len() as isize;
        self.focused = controls[(at + delta).rem_euclid(n) as usize];
        cx.notify();
    }

    /// Presses `control` (ignored until armed).
    pub fn press(&mut self, control: Control, cx: &mut Context<Self>) {
        self.focused = control;
        if !self.armed {
            cx.notify();
            return;
        }
        match control {
            Control::Write => {
                self.write = !self.write;
                cx.notify();
            }
            Control::Answer(choice) => self.finish(choice.answer(self.write), cx),
        }
    }

    /// Answers and closes (once).
    pub fn finish(&mut self, answer: ConsentAnswer, cx: &mut Context<Self>) {
        if self.answer.get().is_some() {
            return;
        }
        self.answer.set(Some(answer));
        if let Some(reply) = self.reply.take() {
            reply.send(answer);
        }
        self._tasks.clear();
        cx.emit(PromptResponse(0));
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let shift = key.modifiers.shift;
        // A repeat of a key pressed before the dialog appeared.
        if event.is_held && matches!(key.key.as_str(), "enter" | "space") {
            cx.stop_propagation();
            return;
        }
        match key.key.as_str() {
            "escape" => self.finish(ConsentAnswer::Dismissed, cx),
            "tab" if shift => self.move_focus(-1, cx),
            "tab" | "right" | "down" => self.move_focus(1, cx),
            "left" | "up" => self.move_focus(-1, cx),
            "enter" | "space" => self.press(self.focused, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Drop for ConsentDialog {
    fn drop(&mut self) {
        // Gone unanswered (replaced by another prompt, window closed).
        if let Some(reply) = self.reply.take() {
            reply.send(ConsentAnswer::Dismissed);
        }
    }
}

/// Shows `dialog` as the window's modal prompt. The future resolves when
/// it closes (answered, dismissed or replaced by another prompt).
pub fn show(
    dialog: &Entity<ConsentDialog>,
    window: &mut Window,
    cx: &mut App,
) -> impl std::future::Future<Output = ()> + use<> {
    let view = dialog.clone();
    cx.set_prompt_builder(move |_, _, _, _, handle, window, cx| {
        handle.with_view(view.clone(), window, cx)
    });
    let closed = window.prompt(PromptLevel::Warning, "", None, &["Close"], cx);
    // Back to the app's prompts (themed on Linux, native elsewhere).
    crate::prompt::restore(cx);
    async move {
        closed.await.ok();
    }
}
