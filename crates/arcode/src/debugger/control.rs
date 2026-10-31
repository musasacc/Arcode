//! Starting, stepping and stopping sessions.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arcode_dap::{DebugSession, SessionState};
use arcode_ui_kit::settings;
use gpui::{AppContext, Context};

use super::console::LineKind;
use super::launch::LaunchChoice;
use super::prepare::{self, Cancel, PrepareInput};
use super::session::Session;
use super::{Debugger, DebuggerEvent, Preparing};

/// A stepping command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Continue (F5).
    Continue,
    /// Step over (F10).
    Over,
    /// Step into (F11).
    Into,
    /// Step out (Shift+F11).
    Out,
    /// Pause (F6).
    Pause,
}

impl Debugger {
    /// Starts the selected launch configuration.
    pub fn start_selected(&mut self, file: Option<PathBuf>, cx: &mut Context<Self>) {
        match self.choices.get(self.selected()).cloned() {
            Some(choice) => self.start(choice, file, cx),
            None => cx.emit(DebuggerEvent::Message(
                "Nothing to debug: no launch configuration was found. Add one with \
                 \"Debug: Open launch.toml\"."
                    .into(),
            )),
        }
    }

    /// Prepares (finds the adapter, builds) and starts `choice`. A running
    /// session is stopped first.
    pub fn start(&mut self, choice: LaunchChoice, file: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            cx.emit(DebuggerEvent::Message("Open a project to debug it".into()));
            return;
        };
        self.stop_session(true, cx);
        self.session = None;
        self.console.clear();
        self.info(LineKind::Info, format!("Starting {}…", choice.name), cx);
        let input = PrepareInput {
            root,
            file,
            settings: settings(cx).debug.clone(),
        };
        let cancel = Cancel::default();
        let (log_tx, log_rx) = async_channel::unbounded::<String>();
        let kind = choice.kind.clone();
        let job_cancel = cancel.clone();
        let job = cx.background_spawn(async move {
            let log = move |line: String| {
                let _ = log_tx.try_send(line);
            };
            prepare::prepare(&kind, &input, &job_cancel, &log)
        });
        let name = choice.name.clone();
        let task = cx.spawn(async move |this, cx| {
            // Build output arrives while the job runs; the channel closes
            // when the job (and its logger) is done.
            while let Ok(line) = log_rx.recv().await {
                let lines: Vec<String> = std::iter::once(line)
                    .chain(std::iter::from_fn(|| log_rx.try_recv().ok()))
                    .collect();
                this.update(cx, |d, cx| {
                    for line in lines {
                        d.console.line(LineKind::Info, line);
                    }
                    d.console_changed(cx);
                })
                .ok();
            }
            let result = job.await;
            this.update(cx, |d, cx| {
                d.preparing = None;
                match result {
                    Ok(prepared) => d.launch(choice, prepared.config, cx),
                    Err(message) => {
                        d.info(LineKind::Error, message.clone(), cx);
                        if message != "Cancelled" {
                            cx.emit(DebuggerEvent::Message(message));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        });
        self.preparing = Some(Preparing {
            name,
            cancel,
            _task: task,
        });
        cx.emit(DebuggerEvent::Started);
        cx.notify();
    }

    fn launch(
        &mut self,
        choice: LaunchChoice,
        mut config: arcode_dap::DebugConfig,
        cx: &mut Context<Self>,
    ) {
        // The breakpoints as they are now: they may have changed while the
        // program was built. The adapter's answers to this first set are
        // matched with these ids.
        let mut startup_ids = std::collections::HashMap::new();
        config.breakpoints.clear();
        for path in self.book.files() {
            let (ids, list) = self.book.source_breakpoints(&path);
            if !list.is_empty() {
                startup_ids.insert(path.clone(), ids);
                config.breakpoints.insert(path, list);
            }
        }
        let adapter = config.adapter.name.clone();
        self.info(LineKind::Info, format!("Debug adapter: {adapter}"), cx);
        let (handle, events) = DebugSession::start(config);
        let pump = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let more: Vec<_> = std::iter::once(event)
                    .chain(std::iter::from_fn(|| events.try_recv().ok()))
                    .collect();
                let alive = this
                    .update(cx, |d, cx| {
                        for event in more {
                            d.on_dap_event(event, cx);
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        });
        self.session = Some(Session {
            name: choice.name.clone(),
            choice,
            handle,
            state: SessionState::Initializing,
            capabilities: Default::default(),
            threads: Vec::new(),
            thread: None,
            frames: Vec::new(),
            frame: None,
            scopes: Vec::new(),
            children: Default::default(),
            loading: Default::default(),
            adapter_ids: Default::default(),
            startup_ids,
            stepping: false,
            generation: 0,
            exit_code: None,
            _events: pump,
        });
        cx.notify();
    }

    /// F5: continues a paused session, or starts the selected
    /// configuration when nothing runs.
    pub fn start_or_continue(&mut self, file: Option<PathBuf>, cx: &mut Context<Self>) {
        match &self.session {
            Some(s) if !s.is_terminated() => self.step(Step::Continue, cx),
            _ if self.preparing.is_some() => {}
            _ => self.start_selected(file, cx),
        }
    }

    /// Continue, step or pause the current thread.
    pub fn step(&mut self, step: Step, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let stopped = session.is_stopped();
        // One resume at a time: until the adapter reports that the program
        // runs (or the request fails), further steps are dropped.
        if (step == Step::Pause) == stopped || session.is_terminated() || session.stepping {
            return;
        }
        let Some(thread) = session.current_thread() else {
            return;
        };
        let handle = session.handle.clone();
        if let Some(session) = &mut self.session
            && step != Step::Pause
        {
            session.stepping = true;
        }
        let request: std::pin::Pin<Box<dyn Future<Output = arcode_dap::Result<()>> + Send>> =
            match step {
                Step::Continue => {
                    let f = handle.continue_(thread);
                    Box::pin(async move { f.await.map(|_| ()) })
                }
                Step::Over => Box::pin(handle.next(thread)),
                Step::Into => Box::pin(handle.step_in(thread)),
                Step::Out => Box::pin(handle.step_out(thread)),
                Step::Pause => Box::pin(handle.pause(thread)),
            };
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |d, cx| {
                if let Some(s) = &mut d.session {
                    s.stepping = false;
                }
                if let Err(e) = result {
                    d.info(LineKind::Error, e.to_string(), cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Stops the session (or the build). With `quiet` nothing is printed.
    pub fn stop_session(&mut self, quiet: bool, cx: &mut Context<Self>) {
        if let Some(preparing) = self.preparing.take() {
            preparing.cancel.cancel();
        }
        if let Some(session) = &self.session
            && !session.is_terminated()
        {
            let stop = session.handle.stop();
            cx.background_spawn(async move {
                let _ = stop.await;
            })
            .detach();
            if !quiet {
                self.info(LineKind::Info, "Stopping…".into(), cx);
            }
        }
        if quiet {
            self.end_session(cx);
        }
        cx.notify();
    }

    /// Stops and starts the same configuration again (rebuilding it).
    pub fn restart(&mut self, file: Option<PathBuf>, cx: &mut Context<Self>) {
        let choice = self.session.as_ref().map(|s| s.choice.clone());
        if let Some(choice) = choice {
            self.start(choice, file, cx);
        }
    }

    /// Forgets the session: clears the stopped line and verification.
    pub(super) fn end_session(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &mut self.session {
            session.state = SessionState::Terminated;
            session.forget_stop();
            session.threads.clear();
            session.thread = None;
        }
        for watch in &mut self.watches {
            watch.1 = super::session::WatchValue::Pending;
        }
        self.stopped_at = None;
        self.book.clear_verification();
        self.refresh_all_editors(cx);
        cx.notify();
    }

    /// Sends `path`'s breakpoints to the running session.
    pub(super) fn send_breakpoints(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref().filter(|s| !s.is_terminated()) else {
            return;
        };
        let (ids, list) = self.book.source_breakpoints(path);
        let request = session.handle.set_breakpoints(path, list);
        // This request's answer is applied below with the right ids; the
        // startup answer for the file (if it is still coming) is stale.
        if let Some(session) = &mut self.session {
            session.startup_ids.remove(path);
        }
        let path: Arc<Path> = path.into();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |d, cx| match result {
                Ok(answer) => d.apply_breakpoint_answer(&path, &ids, &answer, cx),
                Err(e) => d.info(LineKind::Error, format!("Breakpoints: {e}"), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Stores the adapter's view of breakpoints sent for `path`.
    pub(super) fn apply_breakpoint_answer(
        &mut self,
        path: &Path,
        ids: &[u64],
        answer: &[arcode_dap::Breakpoint],
        cx: &mut Context<Self>,
    ) {
        self.book.apply_verification(ids, answer);
        if let Some(session) = &mut self.session {
            for (id, bp) in ids.iter().zip(answer) {
                if let Some(adapter_id) = bp.id {
                    session.adapter_ids.insert(adapter_id, *id);
                }
            }
        }
        for (editor, p) in self.open_editors(cx) {
            if p == path {
                self.refresh_editor(&editor, cx);
            }
        }
        cx.notify();
    }
}
