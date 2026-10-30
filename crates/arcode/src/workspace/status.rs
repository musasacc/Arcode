//! Status bar updates and the LSP / Problems wiring of the workspace.

use std::path::{Path, PathBuf};

use arcode_editor::{Editor, EditorEvent};
use arcode_lsp::lsp_types::Position;
use gpui::{Context, Entity, SharedString, Subscription, Window};

use super::{Workspace, WorkspaceEvent};
use crate::bottom_panel::BottomPanel;
use crate::lsp_bridge::{LspBridge, LspBridgeEvent};
use crate::problems::ProblemsEvent;
use crate::project::Project;

impl Workspace {
    /// Subscribes to the bridge (counts, status, messages) and to the
    /// Problems list (jump to a problem).
    pub(super) fn subscribe_lsp(
        lsp: &Entity<LspBridge>,
        panel: &Entity<BottomPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        let bridge = cx.subscribe(lsp, |ws, _, event: &LspBridgeEvent, cx| match event {
            LspBridgeEvent::DiagnosticsChanged => ws.refresh_diagnostic_counts(cx),
            LspBridgeEvent::StatusChanged => ws.refresh_status(cx),
            LspBridgeEvent::Message(text) => ws.show_toast(text.clone(), cx),
        });
        let problems = panel.read(cx).problems().clone();
        let problems = cx.subscribe_in(
            &problems,
            window,
            |ws, _, event: &ProblemsEvent, window, cx| {
                let ProblemsEvent::Open {
                    path,
                    server,
                    position,
                } = event;
                ws.open_problem(path, server, *position, window, cx);
            },
        );
        vec![bridge, problems]
    }

    /// Opens `path` (or switches to it) and puts the cursor at the
    /// diagnostic's position.
    fn open_problem(
        &mut self,
        path: &Path,
        server: &str,
        position: Position,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file(path, window, cx);
        let Some(editor) = self.editor_for_path(path, cx) else {
            return;
        };
        let point = self
            .lsp
            .read(cx)
            .point_in(editor.read(cx), server, position);
        editor.update(cx, |e, cx| e.go_to(point, window, cx));
    }

    /// The open editor showing `path`, in any pane.
    pub fn editor_for_path(&self, path: &Path, cx: &gpui::App) -> Option<Entity<Editor>> {
        let path: PathBuf = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.editors(cx)
            .into_iter()
            .find(|e| e.read(cx).path() == Some(path.as_path()))
    }

    fn refresh_diagnostic_counts(&mut self, cx: &mut Context<Self>) {
        let counts = self.lsp.read(cx).counts();
        self.status_bar.update(cx, |bar, cx| {
            bar.set_diagnostic_counts(counts.errors, counts.warnings, cx)
        });
    }

    /// Re-subscribes to the active editor and refreshes the status bar.
    pub(super) fn active_editor_changed(&mut self, cx: &mut Context<Self>) {
        let editor = self.active_editor(cx);
        self.active_editor_sub = editor.as_ref().map(|editor| {
            cx.subscribe(editor, |ws, _, event: &EditorEvent, cx| {
                if matches!(
                    event,
                    EditorEvent::SelectionsChanged | EditorEvent::Edited { .. }
                ) {
                    ws.refresh_status(cx);
                }
            })
        });
        if let (Some(editor), Some(tree)) = (&editor, &self.file_tree)
            && let Some(path) = editor.read(cx).path().map(Path::to_path_buf)
        {
            tree.update(cx, |tree, cx| tree.reveal(&path, cx));
        }
        self.refresh_status(cx);
        cx.emit(WorkspaceEvent::ActiveEditorChanged(editor));
        cx.notify();
    }

    /// Pushes branch, language, language server status and cursor
    /// position to the status bar.
    pub fn refresh_status(&mut self, cx: &mut Context<Self>) {
        let branch = self
            .project
            .as_ref()
            .and_then(Project::branch)
            .map(SharedString::from);
        let (language, cursor, lsp_status) = match self.active_editor(cx) {
            Some(editor) => {
                let e = editor.read(cx);
                let pos = e.cursor_position();
                let status = self.lsp.read(cx).status_for_language(e.language().id, cx);
                (
                    Some(SharedString::new_static(e.language().name)),
                    Some((pos.line, pos.column)),
                    status,
                )
            }
            None => (None, None, None),
        };
        self.status_bar.update(cx, |bar, cx| {
            bar.set_editor_info(branch, language, cursor, cx);
            bar.set_lsp_status(lsp_status, cx);
        });
    }
}
