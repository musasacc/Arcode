//! Following an editor to another file (renamed or moved) or language:
//! its document is closed and opened again.

use std::path::{Path, PathBuf};

use arcode_editor::Editor;
use gpui::{Context, Entity};

use super::{LspBridge, LspBridgeEvent};

impl LspBridge {
    /// The editor now shows another file (renamed or moved), or its
    /// language gained or lost all servers in the settings: the document
    /// is closed and opened again, so diagnostics and completion follow
    /// the file. Diagnostics of a previous path are dropped (no server
    /// reports on a closed file). Always reopens; the
    /// [`arcode_editor::EditorEvent::PathChanged`] handler uses
    /// [`Self::editor_path_event`], which skips a path already followed.
    pub fn editor_path_changed(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        self.follow(editor, false, cx);
    }

    /// [`arcode_editor::EditorEvent::PathChanged`]: like
    /// [`Self::editor_path_changed`], but nothing happens when the bridge
    /// already follows the new path (a caller such as a refactoring or a
    /// language change reopened it before the event was delivered).
    pub(super) fn editor_path_event(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let new = editor.read(cx).path().map(Path::to_path_buf);
        let old = self.docs.get(&editor.entity_id()).map(|d| d.path.clone());
        if old.is_some() && old == new {
            return;
        }
        self.follow(editor, false, cx);
    }

    /// The editor's language changed (language picker, file association):
    /// like [`Self::editor_path_changed`], and the diagnostics the old
    /// language's servers published for the file are dropped too (the new
    /// servers publish their own after `didOpen`).
    pub fn editor_language_changed(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        self.follow(editor, true, cx);
    }

    /// Drops the diagnostics of the path the bridge tracked when the
    /// editor moved away from it (and of the current path when
    /// `clear_current`), then closes and reopens the document.
    fn follow(&mut self, editor: &Entity<Editor>, clear_current: bool, cx: &mut Context<Self>) {
        let new = editor.read(cx).path().map(Path::to_path_buf);
        let old = self.docs.get(&editor.entity_id()).map(|d| d.path.clone());
        let mut stale: Vec<PathBuf> = old
            .into_iter()
            .filter(|o| Some(o) != new.as_ref())
            .collect();
        if clear_current {
            stale.extend(new);
        }
        let mut cleared = false;
        for path in stale {
            if !self.store.for_path(&path).is_empty() {
                self.store.clear_path(&path);
                cleared = true;
            }
        }
        if cleared {
            cx.emit(LspBridgeEvent::DiagnosticsChanged);
            cx.notify();
        }
        let was_synced = self.docs.get(&editor.entity_id()).is_some_and(|d| d.synced);
        self.forget(editor.entity_id());
        self.register_editor(editor, cx);
        self.schedule_idle_stop(cx);
        let synced = self.docs.get(&editor.entity_id()).is_some_and(|d| d.synced);
        if was_synced && !synced {
            // Renamed to (or now) a language without servers: drop the
            // providers `register_editor` installed for the old one.
            editor.update(cx, |e, cx| {
                e.set_completion_provider(None);
                e.set_hover_provider(None);
                e.set_language_services(None, cx);
            });
        }
    }
}

#[cfg(test)]
impl LspBridge {
    /// The path the bridge tracks for `editor`.
    fn doc_path(&self, editor: &Entity<Editor>) -> Option<&Path> {
        self.docs.get(&editor.entity_id()).map(|d| d.path.as_path())
    }
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
