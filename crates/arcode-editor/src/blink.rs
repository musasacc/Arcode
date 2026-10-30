//! Optional cursor blinking (`settings.editor.cursor_blink`).
//!
//! A timer toggles visibility and notifies only while the editor is
//! focused in an active window; otherwise no task runs at all.

use std::time::Duration;

use gpui::{Context, Window};

use crate::Editor;

const BLINK_INTERVAL: Duration = Duration::from_millis(530);

impl Editor {
    /// Shows the cursor and restarts the blink phase (call on every edit or
    /// cursor move).
    pub(crate) fn reset_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_visible = true;
        self.blink_epoch = self.blink_epoch.wrapping_add(1);
        if self.blink_task.is_some() {
            self.start_blink(cx);
        }
    }

    /// Starts or stops the blink timer to match focus, window activity and
    /// settings. Called from `render`.
    pub(crate) fn sync_blink(&mut self, window: &Window, cx: &mut Context<Self>) {
        let wanted = self.style(cx).cursor_blink
            && window.is_window_active()
            && self.focus_handle.is_focused(window);
        match (wanted, self.blink_task.is_some()) {
            (true, false) => self.start_blink(cx),
            (false, true) => {
                self.blink_task = None;
                self.cursor_visible = true;
            }
            _ => {}
        }
    }

    fn start_blink(&mut self, cx: &mut Context<Self>) {
        let epoch = self.blink_epoch;
        self.blink_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK_INTERVAL).await;
                let alive = this.update(cx, |editor, cx| {
                    if editor.blink_epoch != epoch {
                        return false;
                    }
                    editor.cursor_visible = !editor.cursor_visible;
                    cx.notify();
                    true
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        }));
    }
}
