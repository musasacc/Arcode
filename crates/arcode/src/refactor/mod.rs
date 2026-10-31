//! Applying workspace edits (code actions, renames, a server's
//! `workspace/applyEdit`) across the files of the workspace.
//!
//! Text edits go to the file's open editor, one undo step per file (even
//! when several `TextDocumentEdit`s touch it). A file that is not open is
//! opened in a background tab of the active pane and left unsaved (its tab
//! shows the modified dot), so nothing is written behind the user's back;
//! the tab that was active stays active and focus stays where it was.
//!
//! Resource operations (create, rename, delete) act on disk right away,
//! and open editors follow a rename. Because the disk then already shows
//! part of the edit, an edit with resource operations also saves the
//! files it changed that had no unsaved changes before (like VS Code's
//! `files.refactoring.autoSave`), so the project builds. Undo reverts the
//! text, not the resource operations.
//!
//! A document whose version differs from the one the server computed the
//! edit for, a delete of a file with unsaved changes, or a resource
//! operation bound to fail (checked by simulating them in order: a rename
//! onto an existing file, a missing source...) refuses the whole edit
//! before anything changes; after that the operations run in order
//! and stop at the first failure (`failureHandling: abort`). The outcome
//! goes back to whoever asked through the request's reply; a failure is
//! shown as a toast only for a server's own `workspace/applyEdit` (the
//! editor shows its own notice for code actions and renames).

mod apply;
mod files;
mod preflight;
#[cfg(test)]
mod resource_tests;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;

use gpui::{Context, Entity, Subscription, Window};

use crate::lsp_bridge::edits::EditSource;
use crate::lsp_bridge::{LspBridge, LspBridgeEvent};
use crate::workspace::Workspace;

impl Workspace {
    /// Applies the workspace edits the bridge emits.
    pub(crate) fn subscribe_refactor(
        lsp: &Entity<LspBridge>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(lsp, window, |ws, _, event: &LspBridgeEvent, window, cx| {
            if let LspBridgeEvent::ApplyEdit(request) = event {
                let request = &request.0;
                match ws.apply_edit_request(request, window, cx) {
                    Ok(summary) => {
                        request.reply.send(true, None);
                        if let Some(message) = summary.message(request.label.as_deref()) {
                            ws.show_toast(message, cx);
                        }
                    }
                    Err(reason) => {
                        log::warn!("workspace edit not applied: {reason}");
                        if request.source == EditSource::Server {
                            let what = request.label.as_deref().unwrap_or("Edit");
                            ws.show_toast(format!("{what} failed: {reason}"), cx);
                        }
                        request.reply.send(false, Some(reason));
                    }
                }
            }
        })
    }
}

/// What an applied edit touched.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Summary {
    /// Files whose text changed.
    pub files: usize,
    /// Files opened in background tabs for the edit (left unsaved).
    pub opened: usize,
    /// Files created, renamed or deleted.
    pub resources: usize,
    /// The edited files were saved (the edit had resource operations).
    pub saved: bool,
}

impl Summary {
    /// A toast for edits beyond the active file, if worth one.
    fn message(&self, label: Option<&str>) -> Option<String> {
        if self.files <= 1 && self.opened == 0 && self.resources == 0 {
            return None;
        }
        let label = label.unwrap_or("Edit");
        let mut text = format!(
            "{label}: changed {} file{}",
            self.files,
            if self.files == 1 { "" } else { "s" }
        );
        if self.opened > 0 && !self.saved {
            text.push_str(&format!(" ({} opened unsaved)", self.opened));
        }
        if self.resources > 0 {
            text.push_str(&format!(
                ", {} file operation{}",
                self.resources,
                if self.resources == 1 { "" } else { "s" }
            ));
        }
        if self.saved {
            text.push_str(", saved");
        }
        Some(text)
    }
}
