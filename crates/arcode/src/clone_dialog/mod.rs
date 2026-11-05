//! The Clone Repository dialog: a repository address, a parent folder and
//! the folder name (taken from the address until the user edits it).
//! Clone runs the system `git clone` (see [`job`]) only because the user
//! clicked it, shows its progress and can be cancelled; the workspace then
//! opens the clone. "Clone in Terminal" runs the same command in the
//! integrated terminal instead, where git can ask for credentials.

mod job;
mod render;
mod request;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use arcode_git::CloneProgress;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    PathPromptOptions, SharedString, Subscription, Task, Window, actions,
};

use crate::text_field::{TextChanged, TextField};
pub use job::{JobError, JobEvent};
pub use request::{
    CloneRequest, check_request, failure_message, is_local_path, proposed_folder, terminal_command,
};
use request::{is_problem, needs_terminal};

actions!(
    clone_dialog,
    [StartClone, Dismiss, NextField, PreviousField]
);

const CONTEXT: &str = "CloneDialog";

/// Registers Enter / Escape and Tab / Shift-Tab (between the fields)
/// for the dialog.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", StartClone, Some(CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
        KeyBinding::new("tab", NextField, Some(CONTEXT)),
        KeyBinding::new("shift-tab", PreviousField, Some(CONTEXT)),
    ]);
}

/// Events emitted by the dialog.
#[derive(Clone, Debug, PartialEq)]
pub enum CloneEvent {
    /// The repository was cloned into this folder.
    Cloned(PathBuf),
    /// Run the clone in the integrated terminal instead.
    InTerminal(CloneRequest),
    /// The dialog was closed.
    Dismissed,
}

/// A clone in progress.
struct Running {
    job: job::CloneJob,
    progress: Option<CloneProgress>,
    _events: Task<()>,
}

/// The dialog view.
pub struct CloneDialog {
    url: Entity<TextField>,
    location: Entity<TextField>,
    folder: Entity<TextField>,
    /// The folder name last proposed from the address: the folder field
    /// follows the address while it still shows this.
    proposed: String,
    running: Option<Running>,
    error: Option<SharedString>,
    /// The message only tells what happened (a cancelled clone): it is
    /// not drawn as an error.
    notice_only: bool,
    /// The error is one Clone in Terminal solves (it is then highlighted).
    terminal_hint: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<CloneEvent> for CloneDialog {}

impl Focusable for CloneDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.url.focus_handle(cx)
    }
}

impl CloneDialog {
    /// Creates the dialog proposing `parent` as the location.
    pub fn new(parent: PathBuf, cx: &mut Context<Self>) -> Self {
        let url = cx
            .new(|cx| TextField::new("https://host/owner/repo.git or git@host:owner/repo.git", cx));
        let location = cx.new(|cx| {
            let mut field = TextField::new("Parent folder", cx);
            let home = std::env::home_dir();
            field.set_text(&crate::welcome::tilde_path(&parent, home.as_deref()), cx);
            field
        });
        let folder = cx.new(|cx| TextField::new("Folder name", cx));
        let subs = vec![
            cx.subscribe(&url, |dialog: &mut Self, _, _: &TextChanged, cx| {
                dialog.url_changed(cx)
            }),
            cx.subscribe(&location, Self::clear_error),
            cx.subscribe(&folder, Self::clear_error),
        ];
        Self {
            url,
            location,
            folder,
            proposed: String::new(),
            running: None,
            error: None,
            notice_only: false,
            terminal_hint: false,
            _subs: subs,
        }
    }

    fn clear_error(&mut self, _: Entity<TextField>, _: &TextChanged, cx: &mut Context<Self>) {
        if self.error.take().is_some() {
            self.terminal_hint = false;
            cx.notify();
        }
    }

    fn url_changed(&mut self, cx: &mut Context<Self>) {
        let proposed = proposed_folder(self.url.read(cx).text());
        let current = self.folder.read(cx).text().to_owned();
        if current == self.proposed || current.trim().is_empty() {
            self.folder.update(cx, |f, cx| f.set_text(&proposed, cx));
        }
        self.proposed = proposed;
        self.error = None;
        self.terminal_hint = false;
        cx.notify();
    }

    /// Sets the address field (tests, scripting).
    pub fn set_url(&mut self, url: &str, cx: &mut Context<Self>) {
        self.url.update(cx, |f, cx| f.set_text(url, cx));
    }

    /// Sets the location field (tests, scripting).
    pub fn set_location(&mut self, location: &str, cx: &mut Context<Self>) {
        self.location.update(cx, |f, cx| f.set_text(location, cx));
    }

    /// Sets the folder name field (tests, scripting).
    pub fn set_folder(&mut self, folder: &str, cx: &mut Context<Self>) {
        self.folder.update(cx, |f, cx| f.set_text(folder, cx));
    }

    /// The address field's text.
    pub fn url(&self, cx: &App) -> String {
        self.url.read(cx).text().to_owned()
    }

    /// The folder name field's text.
    pub fn folder(&self, cx: &App) -> String {
        self.folder.read(cx).text().to_owned()
    }

    /// The error shown, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_ref().map(|e| e.as_ref())
    }

    /// Whether the message shown is drawn as an error.
    pub fn shows_error(&self) -> bool {
        self.error.is_some() && !self.notice_only
    }

    /// Moves the keyboard to the next (`delta` 1) or previous (`delta` -1)
    /// field, wrapping around.
    fn move_focus(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let fields = [&self.url, &self.location, &self.folder];
        let current = fields
            .iter()
            .position(|f| f.focus_handle(cx).is_focused(window))
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(fields.len() as isize) as usize;
        window.focus(&fields[next].focus_handle(cx));
        cx.notify();
    }

    fn next_field(&mut self, _: &NextField, window: &mut Window, cx: &mut Context<Self>) {
        self.move_focus(1, window, cx);
    }

    fn previous_field(&mut self, _: &PreviousField, window: &mut Window, cx: &mut Context<Self>) {
        self.move_focus(-1, window, cx);
    }

    /// Whether a clone is running.
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    fn request(&self, cx: &App) -> Result<CloneRequest, String> {
        check_request(
            self.url.read(cx).text(),
            self.location.read(cx).text(),
            self.folder.read(cx).text(),
        )
    }

    fn show_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        self.notice_only = false;
        cx.notify();
    }

    /// Starts the clone, or shows why it cannot.
    pub fn start(&mut self, _: &StartClone, _: &mut Window, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let request = match self.request(cx) {
            Ok(request) => request,
            Err(message) => return self.show_error(message, cx),
        };
        let target = request.target();
        log::info!("cloning {} into {}", request.url, target.display());
        let (job, events) = job::CloneJob::start(request.url, target.clone());
        let task = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let done = matches!(event, JobEvent::Finished(_));
                let alive = this
                    .update(cx, |dialog, cx| dialog.on_event(event, &target, cx))
                    .is_ok();
                if done || !alive {
                    break;
                }
            }
        });
        self.error = None;
        self.terminal_hint = false;
        self.running = Some(Running {
            job,
            progress: None,
            _events: task,
        });
        cx.notify();
    }

    fn on_event(&mut self, event: JobEvent, target: &Path, cx: &mut Context<Self>) {
        match event {
            JobEvent::Progress(progress) => {
                if let Some(running) = &mut self.running {
                    running.progress = Some(progress);
                    cx.notify();
                }
            }
            JobEvent::Finished(result) => {
                self.running = None;
                match result {
                    Ok(()) => cx.emit(CloneEvent::Cloned(target.to_path_buf())),
                    Err(error) => {
                        log::info!("clone ended: {error:?}");
                        self.terminal_hint = needs_terminal(&error);
                        self.notice_only = !is_problem(&error);
                        self.error = Some(failure_message(&error).into());
                    }
                }
                cx.notify();
            }
        }
    }

    /// Stops a running clone (the dialog stays open), else closes the
    /// dialog.
    pub fn cancel(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        match &self.running {
            Some(running) => running.job.cancel(),
            None => cx.emit(CloneEvent::Dismissed),
        }
    }

    /// Hands the clone to the integrated terminal.
    fn clone_in_terminal(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        match self.request(cx) {
            Ok(request) => cx.emit(CloneEvent::InTerminal(request)),
            Err(message) => self.show_error(message, cx),
        }
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose Folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |dialog, cx| {
                    dialog.set_location(&path.display().to_string(), cx)
                })
                .ok();
            }
        })
        .detach();
    }
}
