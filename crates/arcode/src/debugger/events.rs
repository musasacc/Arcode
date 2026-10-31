//! Reacting to the adapter's events.

use arcode_dap::{DebugEvent, SessionState};
use gpui::Context;

use super::console::LineKind;
use super::{Debugger, DebuggerEvent};

impl Debugger {
    /// Applies one event of the running session.
    pub(super) fn on_dap_event(&mut self, event: DebugEvent, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        // After the session ended, an adapter that crashes or logs while
        // shutting down is only noise.
        if session.is_terminated()
            && matches!(
                event,
                DebugEvent::AdapterLog(_)
                    | DebugEvent::AdapterExited(_)
                    | DebugEvent::StateChanged(_)
            )
        {
            return;
        }
        match event {
            DebugEvent::StateChanged(state) => self.state_changed(state, cx),
            DebugEvent::Capabilities(caps) => {
                if let Some(s) = &mut self.session {
                    s.capabilities = caps;
                }
            }
            DebugEvent::Initialized | DebugEvent::Continued(_) | DebugEvent::Process(_) => {}
            DebugEvent::Stopped(body) => {
                let reason = match (&body.description, &body.text) {
                    (Some(d), Some(t)) => format!("{d}: {t}"),
                    (Some(d), None) => d.clone(),
                    (None, Some(t)) => format!("{}: {t}", body.reason),
                    (None, None) => return,
                };
                if body.reason == "exception" {
                    self.info(LineKind::Error, reason, cx);
                }
            }
            DebugEvent::Exited(body) => {
                if let Some(s) = &mut self.session {
                    s.exit_code = Some(body.exit_code);
                }
                self.info(
                    LineKind::Info,
                    format!("Program exited with code {}", body.exit_code),
                    cx,
                );
            }
            DebugEvent::Terminated(_) => {}
            DebugEvent::Output(body) => {
                if body.category.as_deref() == Some("telemetry") {
                    return;
                }
                let kind = LineKind::from_category(body.category.as_deref());
                self.console.output(kind, &body.output);
                self.console_changed(cx);
            }
            DebugEvent::Breakpoint(body) => {
                let Some(adapter_id) = body.breakpoint.id else {
                    return;
                };
                let ours = self
                    .session
                    .as_ref()
                    .and_then(|s| s.adapter_ids.get(&adapter_id).copied());
                if let Some(id) = ours
                    && let Some(path) = self.book.get(id).map(|b| b.path.clone())
                {
                    self.book.apply_verification(&[id], &[body.breakpoint]);
                    for (editor, p) in self.open_editors(cx) {
                        if p == path {
                            self.refresh_editor(&editor, cx);
                        }
                    }
                    cx.notify();
                }
            }
            DebugEvent::BreakpointsSet { path, breakpoints } => {
                // Only the answer to the startup set is applied here; later
                // sets are applied by `send_breakpoints` with their ids.
                let ids = self
                    .session
                    .as_mut()
                    .and_then(|s| s.startup_ids.remove(&path));
                if let Some(ids) = ids
                    && ids.len() == breakpoints.len()
                {
                    self.apply_breakpoint_answer(&path, &ids, &breakpoints, cx);
                }
            }
            DebugEvent::Thread(_) => {
                if self.session.as_ref().is_some_and(|s| s.is_stopped()) {
                    self.load_threads(None, cx);
                }
            }
            DebugEvent::RunInTerminal(request) => {
                let title = self
                    .session
                    .as_ref()
                    .map_or_else(|| "Debug".to_string(), |s| format!("Debug: {}", s.name));
                cx.emit(DebuggerEvent::RunInTerminal { title, request });
            }
            DebugEvent::AdapterLog(line) => {
                self.console.line(LineKind::Info, line);
                self.console_changed(cx);
            }
            DebugEvent::AdapterExited(reason) => {
                self.info(
                    LineKind::Info,
                    format!("Debug adapter exited: {reason}"),
                    cx,
                );
                self.end_session(cx);
            }
            DebugEvent::AdapterFailed(message) => {
                self.info(LineKind::Error, message.clone(), cx);
                cx.emit(DebuggerEvent::Message(format!(
                    "The debug session could not start: {message}"
                )));
                self.end_session(cx);
            }
        }
    }

    fn state_changed(&mut self, state: SessionState, cx: &mut Context<Self>) {
        let Some(session) = &mut self.session else {
            return;
        };
        session.state = state.clone();
        session.stepping = false;
        match state {
            SessionState::Stopped { thread_id, .. } => {
                session.forget_stop();
                if thread_id.is_some() {
                    session.thread = thread_id;
                }
                self.load_threads(thread_id, cx);
            }
            SessionState::Running => {
                session.forget_stop();
                if self.stopped_at.take().is_some() {
                    self.refresh_all_editors(cx);
                }
                for watch in &mut self.watches {
                    watch.1 = super::session::WatchValue::Pending;
                }
            }
            SessionState::Terminated => {
                self.info(LineKind::Info, "Debugging ended".into(), cx);
                self.end_session(cx);
            }
            SessionState::Initializing => {}
        }
        cx.notify();
    }
}
