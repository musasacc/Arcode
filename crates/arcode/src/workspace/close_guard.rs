//! Asking before something is lost: unsaved editors, and commands still
//! running in the terminals (closing or switching the project, closing the
//! window and quitting all end the terminals). Whether a shell runs a
//! command is checked on the background executor.

use gpui::{App, AppContext, Context, Entity, PromptLevel, Task, Window};

use super::Workspace;

/// The question to ask before closing, when something would be lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CloseWarning {
    /// What would be lost, then `question`.
    pub message: String,
    /// What happens on going ahead.
    pub detail: &'static str,
}

/// What has unsaved changes: editors, and the theme editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Unsaved {
    /// Editors with unsaved changes.
    pub files: usize,
    /// The theme editor has unsaved changes.
    pub theme: bool,
}

impl Unsaved {
    pub(crate) fn is_empty(self) -> bool {
        self.files == 0 && !self.theme
    }

    pub(crate) fn add(&mut self, other: Unsaved) {
        self.files += other.files;
        self.theme |= other.theme;
    }

    fn describe(self) -> Option<String> {
        let files = self.files;
        match (files, self.theme) {
            (0, false) => None,
            (_, false) => Some(format!("{files} file(s) have unsaved changes")),
            (0, true) => Some("The theme editor has unsaved changes".to_owned()),
            (_, true) => Some(format!(
                "{files} file(s) and the theme editor have unsaved changes"
            )),
        }
    }
}

/// The warning for `unsaved` changes and `busy` terminals running a
/// command, or `None` when nothing would be lost.
pub(crate) fn close_warning(unsaved: Unsaved, busy: usize, question: &str) -> Option<CloseWarning> {
    let files = unsaved.describe();
    let jobs = match busy {
        0 => None,
        1 => Some("1 terminal is running a command".to_owned()),
        n => Some(format!("{n} terminals are running a command")),
    };
    let (what, detail) = match (files, jobs) {
        (None, None) => return None,
        (Some(f), None) => (f, "Unsaved changes are lost."),
        (None, Some(j)) => (j, "Running commands are stopped."),
        (Some(f), Some(j)) => (
            format!("{f} and {j}"),
            "Unsaved changes are lost and running commands are stopped.",
        ),
    };
    Some(CloseWarning {
        message: format!("{what}. {question}"),
        detail,
    })
}

/// Counts the shells (by process id) that are running a command.
pub(crate) fn count_busy_shells(pids: Vec<u32>, cx: &App) -> Task<usize> {
    if pids.is_empty() {
        return Task::ready(0);
    }
    cx.background_spawn(async move {
        pids.into_iter()
            .filter(|pid| arcode_project::procs::is_running_command(*pid))
            .count()
    })
}

impl Workspace {
    /// What in this window has unsaved changes.
    pub(crate) fn unsaved(&self, cx: &App) -> Unsaved {
        Unsaved {
            files: self.dirty_count(cx),
            theme: self.theme_edits_unsaved(cx),
        }
    }

    /// Process ids of the shells running in this window's terminals.
    pub(crate) fn shell_pids(&self, cx: &App) -> Vec<u32> {
        self.bottom_panel
            .read(cx)
            .terminals()
            .read(cx)
            .shell_pids(cx)
    }

    /// Runs `then` right away, or after the user agreed to discard unsaved
    /// changes in open editors and the theme editor and to stop commands
    /// running in the terminals.
    pub(super) fn confirm_discard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let dirty = self.unsaved(cx);
        let pids = self.shell_pids(cx);
        if dirty.is_empty() && pids.is_empty() {
            then(self, window, cx);
            return;
        }
        let busy = count_busy_shells(pids, cx);
        cx.spawn_in(window, async move |this, cx| {
            let busy = busy.await;
            let (question, confirm) = if busy == 0 {
                ("Discard them?", "Discard")
            } else {
                ("Continue anyway?", "Continue")
            };
            let Some(warning) = close_warning(dirty, busy, question) else {
                this.update_in(cx, |ws, window, cx| then(ws, window, cx))
                    .ok();
                return;
            };
            let Ok(answer) = this.update_in(cx, |_, window, cx| {
                window.prompt(
                    PromptLevel::Warning,
                    &warning.message,
                    Some(warning.detail),
                    &[confirm, "Cancel"],
                    cx,
                )
            }) else {
                return;
            };
            if answer.await.ok() == Some(0) {
                this.update_in(cx, |ws, window, cx| then(ws, window, cx))
                    .ok();
            }
        })
        .detach();
    }
}

/// Closes the window of `ws`, first asking when editors have unsaved
/// changes or terminals run a command. Returns `true` when the window may
/// close right away; otherwise it is closed later (if the user agrees).
pub(crate) fn request_close(ws: Entity<Workspace>, window: &mut Window, cx: &mut App) -> bool {
    let (dirty, pids) = {
        let ws = ws.read(cx);
        (ws.unsaved(cx), ws.shell_pids(cx))
    };
    if dirty.is_empty() && pids.is_empty() {
        crate::windows::window_closing(&ws, window, cx);
        return true;
    }
    let busy = count_busy_shells(pids, cx);
    window
        .spawn(cx, async move |cx| {
            let busy = busy.await;
            let close = move |window: &mut Window, cx: &mut App| {
                crate::windows::window_closing(&ws, window, cx);
                window.remove_window();
            };
            let Some(warning) = close_warning(dirty, busy, "Close anyway?") else {
                cx.update(close).ok();
                return;
            };
            let Ok(answer) = cx.update(|window, cx| {
                window.prompt(
                    PromptLevel::Warning,
                    &warning.message,
                    Some(warning.detail),
                    &["Close Anyway", "Cancel"],
                    cx,
                )
            }) else {
                return;
            };
            if answer.await.ok() == Some(0) {
                cx.update(close).ok();
            }
        })
        .detach();
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(files: usize) -> Unsaved {
        Unsaved {
            files,
            theme: false,
        }
    }

    #[test]
    fn warnings_name_what_would_be_lost() {
        assert_eq!(close_warning(files(0), 0, "Close anyway?"), None);
        let w = close_warning(files(2), 0, "Close anyway?").unwrap();
        assert_eq!(w.message, "2 file(s) have unsaved changes. Close anyway?");
        let w = close_warning(files(0), 1, "Close anyway?").unwrap();
        assert_eq!(w.message, "1 terminal is running a command. Close anyway?");
        assert_eq!(w.detail, "Running commands are stopped.");
        let w = close_warning(files(1), 3, "Quit anyway?").unwrap();
        assert_eq!(
            w.message,
            "1 file(s) have unsaved changes and 3 terminals are running a command. Quit anyway?"
        );
    }

    #[test]
    fn warnings_name_unsaved_theme_edits() {
        let theme = Unsaved {
            files: 0,
            theme: true,
        };
        let w = close_warning(theme, 0, "Discard them?").unwrap();
        assert_eq!(
            w.message,
            "The theme editor has unsaved changes. Discard them?"
        );
        assert_eq!(w.detail, "Unsaved changes are lost.");
        let mut both = files(2);
        both.add(theme);
        let w = close_warning(both, 1, "Quit anyway?").unwrap();
        assert_eq!(
            w.message,
            "2 file(s) and the theme editor have unsaved changes and 1 terminal is running a command. Quit anyway?"
        );
    }
}
