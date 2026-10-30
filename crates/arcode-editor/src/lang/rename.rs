//! Rename (F2): asks the provider which symbol a rename would change
//! (`prepareRename`), shows the inline field at it with the current name
//! selected, and on Enter hands the new name to the provider, which applies
//! the workspace edit (all files). Escape, a click, a cursor move, an edit
//! or the editor losing focus cancel it.

use arcode_buffer::Point;
use gpui::{AppContext, ClipboardItem, Context, Entity, Keystroke, Task};

use super::rename_field::{FieldKey, RenameField, RenameView};
use super::{RENAME_POPOVER, RenameTarget};
use crate::Editor;
use crate::ext::EditorPopover;

/// The rename in progress.
#[derive(Default)]
pub(crate) struct RenameState {
    /// The open field.
    pub field: Option<Entity<RenameView>>,
    /// Where the rename was started and the name it started with.
    pub target: Option<(Point, String)>,
    /// `prepareRename` in flight.
    pub task: Option<Task<()>>,
}

impl Editor {
    /// Starts renaming the symbol at the cursor (F2).
    pub fn rename_symbol(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            self.show_notice("The file is read-only", cx);
            return;
        }
        let Some(provider) = self.lang.provider.clone() else {
            self.show_notice("Rename needs a language server", cx);
            return;
        };
        let at = self.cursor_position();
        let version = self.buffer.version();
        let Some(prepare) = provider.prepare_rename(self, at, cx) else {
            match self.word_at(at) {
                Some((range, word)) => self.open_rename_field(
                    at,
                    RenameTarget {
                        range,
                        placeholder: word,
                    },
                    cx,
                ),
                None => self.show_notice("Nothing to rename here", cx),
            }
            return;
        };
        let task = cx.spawn(async move |this, cx| {
            let answer = prepare.await;
            this.update(cx, |e, cx| {
                e.lang.rename.task = None;
                if e.buffer.version() != version || e.cursor_position() != at {
                    return;
                }
                match answer {
                    Ok(Some(target)) => e.open_rename_field(at, target, cx),
                    Ok(None) => e.show_notice("This symbol cannot be renamed", cx),
                    Err(err) => e.show_notice(format!("Cannot rename: {err}"), cx),
                }
            })
            .ok();
        });
        self.lang.rename.task = Some(task);
    }

    fn open_rename_field(&mut self, at: Point, target: RenameTarget, cx: &mut Context<Self>) {
        let field = RenameField::new(&target.placeholder);
        let view = cx.new(|_| RenameView { field });
        self.lang.rename.field = Some(view.clone());
        self.lang.rename.target = Some((at, target.placeholder));
        let popover = EditorPopover::new(target.range.start, view.into())
            .on_key(|editor, ks, _, cx| editor.update(cx, |e, cx| e.rename_key(ks, cx)));
        self.show_popover(RENAME_POPOVER, popover, cx);
    }

    /// The text in the open rename field.
    pub fn rename_text(&self, cx: &gpui::App) -> Option<String> {
        let view = self.lang.rename.field.as_ref()?;
        Some(view.read(cx).field.text().to_string())
    }

    /// Keys while the rename field is open; `true` consumes the key.
    fn rename_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(view) = self.lang.rename.field.clone() else {
            return false;
        };
        let clipboard = cx.read_from_clipboard().and_then(|c| c.text());
        let outcome = view.update(cx, |v, cx| {
            let outcome = v.field.key(ks, || clipboard);
            if outcome == FieldKey::Edited {
                cx.notify();
            }
            outcome
        });
        match outcome {
            FieldKey::Edited | FieldKey::Consumed => true,
            FieldKey::Copy(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                true
            }
            FieldKey::Cancel => {
                self.close_rename(cx);
                true
            }
            FieldKey::Submit => {
                let name = view.read(cx).field.text().trim().to_string();
                self.commit_rename(name, cx);
                true
            }
            FieldKey::Ignored => false,
        }
    }

    pub(super) fn close_rename(&mut self, cx: &mut Context<Self>) {
        self.lang.rename.field = None;
        self.lang.rename.target = None;
        self.hide_popover(RENAME_POPOVER, cx);
    }

    /// Applies the rename to `name` (closes the field first).
    pub fn commit_rename(&mut self, name: String, cx: &mut Context<Self>) {
        let target = self.lang.rename.target.clone();
        self.close_rename(cx);
        let Some((at, old)) = target else {
            return;
        };
        if name.is_empty() || name == old {
            return;
        }
        let Some(provider) = self.lang.provider.clone() else {
            return;
        };
        let Some(task) = provider.rename(self, at, name, cx) else {
            self.show_notice("Rename is not available here", cx);
            return;
        };
        cx.spawn(async move |this, cx| {
            if let Err(err) = task.await {
                log::warn!("rename failed: {err}");
                this.update(cx, |e, cx| e.show_notice(format!("Rename failed: {err}"), cx))
                    .ok();
            }
        })
        .detach();
    }
}
