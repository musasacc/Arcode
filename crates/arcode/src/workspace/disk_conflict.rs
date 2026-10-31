//! An editor with unsaved edits whose file changed on disk: it is marked
//! (tab dot and tooltip, the bar above it), and the bar's choices land
//! here: Reload (discard my changes), Compare (a diff of the file against
//! the buffer) and Keep Mine. A save over a changed file asks first.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use arcode_buffer::{Buffer, DiskCheck, DiskStamp};
use arcode_editor::{DiskConflict, Editor, EditorEvent};
use gpui::{App, AppContext, Context, Entity, Focusable, PromptLevel, Window};

use super::Workspace;
use super::reload::{DiskRead, read_disk, reload_editor};
use crate::pane::{CompareWithDisk, KeepMyChanges, ReloadFromDisk, TabLabel};

/// Compares `editor`'s file with what its buffer is based on (off the main
/// thread) and marks the editor: changed, deleted, or neither.
pub(super) fn check_disk_conflict(editor: &Entity<Editor>, cx: &mut App) {
    let e = editor.read(cx);
    let Some(path) = e.path().map(Path::to_path_buf) else {
        return;
    };
    let stamp = e.disk_stamp();
    let check = cx.background_spawn(async move {
        match stamp {
            Some(stamp) => stamp.check(&path),
            None if path.exists() => DiskCheck::Same,
            None => DiskCheck::Missing,
        }
    });
    let weak = editor.downgrade();
    cx.spawn(async move |cx| {
        let check = check.await;
        let clean = weak
            .update(cx, |e, cx| {
                if !e.is_dirty() {
                    return true;
                }
                let conflict = match check {
                    DiskCheck::Same => None,
                    DiskCheck::Changed => Some(DiskConflict::Changed),
                    DiskCheck::Missing => Some(DiskConflict::Deleted),
                };
                e.set_disk_conflict(conflict, cx);
                false
            })
            .unwrap_or(false);
        // Saved or undone meanwhile: it can follow the file.
        if clean && let Some(editor) = weak.upgrade() {
            let _ = cx.update(|cx| reload_editor(&editor, cx));
        }
    })
    .detach();
}

/// Replaces `editor`'s text (unsaved edits included) with its file, as
/// one undo step.
pub(crate) fn reload_discarding(editor: &Entity<Editor>, cx: &mut App) {
    let Some((version, read)) = read_disk(editor, cx) else {
        return;
    };
    let weak = editor.downgrade();
    cx.spawn(async move |cx| {
        let retry = match read.await {
            Some(DiskRead::Edits(edits, ending, stamp)) => weak
                .update(cx, |e, cx| {
                    !e.discard_and_reload(version, edits, ending, stamp, cx)
                })
                .unwrap_or(false),
            Some(DiskRead::Same(stamp)) => {
                weak.update(cx, |e, cx| {
                    // Unsaved edits that match the file: just clean.
                    e.keep_mine(stamp, cx);
                    e.set_disk_conflict(None, cx);
                })
                .ok();
                false
            }
            Some(DiskRead::Missing) => {
                weak.update(cx, |e, cx| {
                    e.set_disk_conflict(Some(DiskConflict::Deleted), cx)
                })
                .ok();
                false
            }
            None => false,
        };
        if retry && let Some(editor) = weak.upgrade() {
            let _ = cx.update(|cx| reload_discarding(&editor, cx));
        }
    })
    .detach();
}

/// Keeps `editor`'s text: what the file holds now becomes what the buffer
/// is based on, so the next save writes without asking.
pub(crate) fn keep_mine(editor: &Entity<Editor>, cx: &mut App) {
    let Some(path) = editor.read(cx).path().map(Path::to_path_buf) else {
        return;
    };
    let stamp = cx.background_spawn(async move { DiskStamp::read(&path).ok() });
    let weak = editor.downgrade();
    cx.spawn(async move |cx| {
        let stamp = stamp.await;
        weak.update(cx, |e, cx| e.keep_mine(stamp, cx)).ok();
    })
    .detach();
}

impl Workspace {
    /// Opens a diff of `editor`'s file on disk against its text.
    pub(crate) fn compare_with_disk(
        &mut self,
        editor: &Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let e = editor.read(cx);
        let Some(path) = e.path().map(Path::to_path_buf) else {
            return;
        };
        let mine = e.buffer().text().clone();
        let title = format!("{} (On Disk ↔ Yours)", e.title());
        let label = TabLabel {
            key: format!("{title}\u{0}{}", path.display()).into(),
            tooltip: format!("{title} · {}", path.display()).into(),
            title: title.into(),
        };
        let read = cx.background_spawn(async move {
            let disk = Buffer::from_file(&path)
                .map(|b| b.to_string())
                .unwrap_or_default();
            (disk, mine.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let (disk, mine) = read.await;
            this.update_in(cx, |ws, window, cx| {
                ws.open_diff_tab(label, &disk, &mine, window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// Asks before saving over a changed file when `editor` reports a
    /// refused save (one question at a time per editor), and lets an
    /// editor marked as changed on disk follow the file once its edits are
    /// undone.
    pub(super) fn track_disk_conflicts(
        &mut self,
        editor: &Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asking = Rc::new(Cell::new(false));
        let sub = cx.subscribe_in(
            editor,
            window,
            move |ws, editor, event, window, cx| match event {
                EditorEvent::SaveConflict if !asking.replace(true) => {
                    ws.ask_to_overwrite(editor.clone(), asking.clone(), window, cx);
                }
                EditorEvent::DirtyChanged(false)
                    if editor.read(cx).disk_conflict() == Some(DiskConflict::Changed) =>
                {
                    reload_editor(editor, cx);
                }
                _ => {}
            },
        );
        self.subs.tie(editor.downgrade().into(), sub);
    }

    fn ask_to_overwrite(
        &mut self,
        editor: Entity<Editor>,
        asking: Rc<Cell<bool>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = format!("{} changed on disk", editor.read(cx).title());
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(
                "Another program changed the file after you opened or saved it. \
                 Overwriting replaces that version with yours.",
            ),
            &["Overwrite", "Compare", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let choice = answer.await.ok();
            asking.set(false);
            this.update_in(cx, |ws, window, cx| {
                if choice == Some(0) {
                    return editor.update(cx, |e, cx| e.save_overwriting(cx));
                }
                // Not overwritten: a tab closing on this save stays open.
                for pane in ws.panes().to_vec() {
                    pane.update(cx, |p, _| p.cancel_close_after_save(editor.entity_id()));
                }
                if choice == Some(1) {
                    ws.compare_with_disk(&editor, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The editor the disk bar's actions are for: the focused one (the bar
    /// focuses its editor), else the active one.
    fn disk_target(&self, window: &Window, cx: &App) -> Option<Entity<Editor>> {
        self.editors(cx)
            .into_iter()
            .find(|e| e.focus_handle(cx).contains_focused(window, cx))
            .or_else(|| self.active_editor(cx))
    }

    /// Registers Reload from Disk, Compare with Disk and Keep My Changes.
    pub(super) fn register_disk_actions<E: gpui::InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(cx.listener(|ws, _: &ReloadFromDisk, window, cx| {
            if let Some(editor) = ws.disk_target(window, cx) {
                reload_discarding(&editor, cx);
            }
        }))
        .on_action(cx.listener(|ws, _: &CompareWithDisk, window, cx| {
            if let Some(editor) = ws.disk_target(window, cx) {
                ws.compare_with_disk(&editor, window, cx);
            }
        }))
        .on_action(cx.listener(|ws, _: &KeepMyChanges, window, cx| {
            if let Some(editor) = ws.disk_target(window, cx) {
                keep_mine(&editor, cx);
            }
        }))
    }
}

#[cfg(test)]
#[path = "disk_conflict_tests.rs"]
mod tests;
