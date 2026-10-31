//! Moving a tab to another pane without closing its editor.

use arcode_editor::Editor;
use gpui::{Context, Entity, Window};

use super::{Pane, PaneEvent, TabLabel};

impl Pane {
    /// Index of the active editor tab (none while a page is shown).
    pub fn active_index(&self) -> Option<usize> {
        self.active_editor().map(|_| self.active)
    }

    /// Takes the tab at `index` out of the pane, keeping its editor alive
    /// for another pane, and returns the editor with its custom label.
    pub fn detach(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(Entity<Editor>, Option<TabLabel>)> {
        if index >= self.items.len() {
            return None;
        }
        let editor = self.items.remove(index);
        self.subscriptions
            .retain(|(id, _)| *id != editor.entity_id());
        let label = self.titles.remove(&editor.entity_id());
        if self.active > index || self.active >= self.items.len() {
            self.active = self.active.saturating_sub(1);
        }
        if self.items.is_empty() && self.pages.is_empty() {
            window.focus(&self.focus_handle);
        }
        self.sync_find_target(cx);
        cx.emit(PaneEvent::ActiveItemChanged);
        cx.notify();
        Some((editor, label))
    }

    /// Adds a tab detached from another pane and activates it.
    pub fn attach(
        &mut self,
        editor: Entity<Editor>,
        label: Option<TabLabel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match label {
            Some(label) => self.add_titled_editor(editor, label, window, cx),
            None => self.add_editor(editor, window, cx),
        }
    }
}
