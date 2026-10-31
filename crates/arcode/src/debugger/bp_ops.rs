//! Changing breakpoints: toggle, options, enable, remove, renames.

use std::path::Path;

use gpui::Context;

use super::{Debugger, DebuggerEvent};

impl Debugger {
    /// Toggles a breakpoint at `path:line`.
    pub fn toggle_breakpoint(&mut self, path: &Path, line: usize, cx: &mut Context<Self>) {
        self.book.toggle(path, line);
        self.breakpoints_changed(path, cx);
    }

    /// Sets condition, hit count and log message at `path:line`.
    pub fn set_breakpoint_options(
        &mut self,
        path: &Path,
        line: usize,
        condition: Option<String>,
        hit_condition: Option<String>,
        log_message: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.book
            .set_options(path, line, condition, hit_condition, log_message);
        self.breakpoints_changed(path, cx);
    }

    /// Checks or unchecks a breakpoint.
    pub fn set_breakpoint_enabled(&mut self, id: u64, enabled: bool, cx: &mut Context<Self>) {
        if let Some(path) = self.book.set_enabled(id, enabled) {
            self.breakpoints_changed(&path, cx);
        }
    }

    /// Removes a breakpoint.
    pub fn remove_breakpoint(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(path) = self.book.remove(id) {
            self.breakpoints_changed(&path, cx);
        }
    }

    /// Removes every breakpoint.
    pub fn remove_all_breakpoints(&mut self, cx: &mut Context<Self>) {
        for path in self.book.clear() {
            self.breakpoints_changed(&path, cx);
        }
    }

    /// A file or folder was renamed on disk.
    pub fn path_renamed(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        if self.book.rename(from, to) {
            cx.emit(DebuggerEvent::BreakpointsChanged);
            self.refresh_all_editors(cx);
        }
        // The new name may give another language (or none): run buttons
        // are found again.
        for (editor, path) in self.open_editors(cx) {
            if path.starts_with(to) {
                self.language_changed(&editor, cx);
            }
        }
    }
}
