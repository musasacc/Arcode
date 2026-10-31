//! Closing a tab whose "Save" was refused because its file changed on
//! disk: the workspace asks whether to overwrite, and the tab closes once
//! the editor saves (or stays open when the user declines).

use arcode_editor::{DiskConflict, Editor, EditorEvent};
use gpui::{Context, Entity, EntityId, Window};

use super::Pane;

impl Pane {
    /// After a refused save while closing `editor`'s tab: when the refusal
    /// was a disk conflict (the workspace now asks whether to overwrite),
    /// the tab closes on the editor's next save and `true` is returned.
    pub(super) fn close_on_overwrite(
        &mut self,
        editor: &Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if editor.read(cx).disk_conflict() != Some(DiskConflict::Changed) {
            return false;
        }
        let id = editor.entity_id();
        let sub = cx.subscribe_in(editor, window, move |pane, editor, event, window, cx| {
            if *event != EditorEvent::Saved {
                return;
            }
            pane.close_after_save.retain(|(i, _)| *i != id);
            if let Some(ix) = pane.items.iter().position(|e| e == editor) {
                pane.remove(ix, window, cx);
            }
        });
        self.close_after_save.retain(|(i, _)| *i != id);
        self.close_after_save.push((id, sub));
        true
    }

    /// The user declined to overwrite the file of the editor `id`: a close
    /// waiting for its save is dropped, the tab stays open.
    pub fn cancel_close_after_save(&mut self, id: EntityId) {
        self.close_after_save.retain(|(i, _)| *i != id);
    }
}
