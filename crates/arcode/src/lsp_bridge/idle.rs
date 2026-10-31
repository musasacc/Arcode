//! Stopping language servers that no open file uses any more (the last
//! file of their language or project was closed, or a library source the
//! debugger stepped into), so they do not keep their memory.

use std::time::Duration;

use gpui::Context;

use super::LspBridge;

/// How long a server without open files keeps running, so closing a file
/// and opening another soon after does not restart (and re-index) it.
const IDLE_STOP_DELAY: Duration = Duration::from_secs(30);

impl LspBridge {
    /// Checks for idle servers after [`IDLE_STOP_DELAY`] (a single timer;
    /// a later close restarts it).
    pub(super) fn schedule_idle_stop(&mut self, cx: &mut Context<Self>) {
        if self.manager.is_none() {
            return;
        }
        self.idle_stop = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(IDLE_STOP_DELAY).await;
            let _ = this.update(cx, |bridge, cx| bridge.stop_idle_servers(cx));
        }));
    }

    /// Stops every server instance no open document uses now.
    pub fn stop_idle_servers(&mut self, cx: &mut Context<Self>) {
        self.idle_stop = None;
        let Some(manager) = self.manager.clone() else {
            return;
        };
        let stopped = manager.stop_idle_servers();
        for name in &stopped {
            log::info!("{name}: no open file uses it, stopped");
            self.log_line(name, "--- no open file uses it, stopped");
        }
        if !stopped.is_empty() {
            self.flush_logs(cx);
        }
    }
}
