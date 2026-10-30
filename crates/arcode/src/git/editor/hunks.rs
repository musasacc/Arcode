//! Change bars: computing hunks, drawing them, the hunk popover and its
//! Stage / Unstage / Revert actions.

use std::time::Duration;

use arcode_buffer::{LineEnding, Point};
use arcode_editor::{
    ChangeKind as BarKind, Editor, EditorPopover, GutterDecoration, GutterLane, GutterMark,
};
use arcode_git::{ChangeKind, GutterHunk, Hunk, revert_hunk};
use arcode_ui_kit::settings;
use gpui::{AppContext, Context, Entity, EntityId};

use super::{GUTTER_SET, GitEditors, GitEditorsEvent};
use crate::git::hunk_popover::HunkPopover;

/// Quiet time after typing before the gutter is recomputed.
const EDIT_DEBOUNCE: Duration = Duration::from_millis(300);

/// What a popover button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkAction {
    /// Stage the change (`git apply --cached`).
    Stage,
    /// Remove the staged change from the index.
    Unstage,
    /// Restore the old lines in the editor (undoable).
    Revert,
}

impl GitEditors {
    /// Recomputes hunks and conflicts of `id`, after a pause when `debounce`.
    pub(crate) fn schedule_analysis(
        &mut self,
        id: EntityId,
        debounce: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.editors.get(&id) else {
            return;
        };
        let Some(editor) = state.editor.upgrade() else {
            return;
        };
        let gutter = settings(cx).git.gutter;
        let repo = state.repo.clone();
        let rel = state.rel_path();
        let task = cx.spawn(async move |this, cx| {
            if debounce {
                cx.background_executor().timer(EDIT_DEBOUNCE).await;
            }
            let Ok((rope, version)) =
                editor.read_with(cx, |e, _| (e.buffer().text().clone(), e.buffer().version()))
            else {
                return;
            };
            let work = cx.background_spawn(async move {
                let text = rope.to_string();
                let conflicts = arcode_git::parse_conflicts(&text);
                let hunks = match (gutter, repo, rel) {
                    (true, Some(repo), Some(rel)) => {
                        let unstaged = arcode_git::diff_against_index(&repo, &rel, &text);
                        let staged = arcode_git::staged_hunks(&repo, &rel).unwrap_or_default();
                        arcode_git::gutter_hunks(&unstaged, &staged)
                    }
                    _ => Vec::new(),
                };
                (hunks, conflicts)
            });
            let (hunks, conflicts) = work.await;
            this.update(cx, |this, cx| {
                this.apply_hunks(id, &editor, hunks, version, cx);
                this.apply_conflicts(id, &editor, conflicts, cx);
            })
            .ok();
        });
        if let Some(state) = self.editors.get_mut(&id) {
            state.hunk_task = Some(task);
        }
    }

    fn apply_hunks(
        &mut self,
        id: EntityId,
        editor: &Entity<Editor>,
        hunks: Vec<GutterHunk>,
        version: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.editors.get_mut(&id) else {
            return;
        };
        let line_count = editor.read(cx).line_count();
        let marks = hunks
            .iter()
            .enumerate()
            .flat_map(|(ix, h)| {
                let kind = match h.kind {
                    ChangeKind::Added => BarKind::Added,
                    ChangeKind::Modified => BarKind::Modified,
                    ChangeKind::Deleted => BarKind::Deleted,
                };
                let lines = if h.lines.is_empty() {
                    let line = h.marker_line(line_count);
                    line..line + 1
                } else {
                    h.lines.clone()
                };
                lines.map(move |line| GutterDecoration {
                    line,
                    lane: GutterLane::Changes,
                    mark: GutterMark::Change(kind),
                    tag: ix as u64,
                })
            })
            .collect();
        state.hunks = hunks;
        state.hunks_version = Some(version);
        editor.update(cx, |e, cx| e.set_gutter_decorations(GUTTER_SET, marks, cx));
    }

    /// Shows the popover of hunk `ix`, or hides it when it is open.
    pub(crate) fn toggle_hunk_popover(
        &mut self,
        editor: &Entity<Editor>,
        ix: usize,
        cx: &mut Context<Self>,
    ) {
        let id = editor.entity_id();
        let Some(hunk) = self.editors.get(&id).and_then(|s| s.hunks.get(ix)).cloned() else {
            return;
        };
        let line_count = editor.read(cx).line_count();
        let anchor = Point::new(
            hunk.marker_line(line_count) + hunk.lines.len().max(1) - 1,
            0,
        );
        let same = editor
            .read(cx)
            .popover(GUTTER_SET)
            .is_some_and(|p| p.anchor == anchor);
        if same {
            editor.update(cx, |e, cx| e.hide_popover(GUTTER_SET, cx));
            return;
        }
        let this = cx.entity().downgrade();
        let view = cx.new(|_| HunkPopover::new(hunk, id, ix, this));
        let popover = EditorPopover::new(anchor, view.into()).on_key(|editor, key, _, cx| {
            if key.key == "escape" {
                editor.update(cx, |e, cx| e.hide_popover(GUTTER_SET, cx));
                return true;
            }
            false
        });
        editor.update(cx, |e, cx| e.show_popover(GUTTER_SET, popover, cx));
    }

    /// Runs a popover button for hunk `ix` of editor `id`.
    pub fn hunk_action(
        &mut self,
        id: EntityId,
        ix: usize,
        action: HunkAction,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.editors.get(&id) else {
            return;
        };
        let (Some(editor), Some(repo), Some(rel)) =
            (state.editor.upgrade(), state.repo.clone(), state.rel_path())
        else {
            return;
        };
        let Some(hunk) = state.hunks.get(ix).cloned() else {
            return;
        };
        editor.update(cx, |e, cx| e.hide_popover(GUTTER_SET, cx));
        let current = editor.read(cx).buffer().version();
        if state.hunks_version != Some(current) {
            cx.emit(GitEditorsEvent::Message(
                "The file changed; try again in a moment".into(),
            ));
            self.schedule_analysis(id, false, cx);
            return;
        }
        let buffer = editor.read(cx).buffer();
        let text = buffer.text().to_string();
        match action {
            HunkAction::Stage if !hunk.staged => {
                let disk_text = if buffer.line_ending() == LineEnding::CrLf {
                    text.replace('\n', "\r\n")
                } else {
                    text
                };
                let inner = hunk.hunk;
                self.store.update(cx, |s, cx| {
                    s.run_in(
                        repo,
                        move |repo| arcode_git::stage_hunk(repo, &rel, &inner, &disk_text),
                        |_, _, _| {},
                        cx,
                    )
                });
            }
            HunkAction::Unstage if hunk.staged => {
                let inner = hunk.hunk;
                self.store.update(cx, |s, cx| {
                    s.run_in(
                        repo,
                        move |repo| arcode_git::unstage_hunk(repo, &rel, &inner),
                        |_, _, _| {},
                        cx,
                    )
                });
            }
            HunkAction::Revert => {
                revert_in_editor(&editor, &hunk, &text, cx);
                if hunk.staged {
                    let inner = hunk.hunk;
                    self.store.update(cx, |s, cx| {
                        s.run_in(
                            repo,
                            move |repo| arcode_git::unstage_hunk(repo, &rel, &inner),
                            |_, _, _| {},
                            cx,
                        )
                    });
                }
            }
            _ => {}
        }
    }
}

/// Replaces the hunk's lines in the editor with its old text (one undo
/// step). `text` is the buffer text the hunk was computed against.
fn revert_in_editor(editor: &Entity<Editor>, hunk: &GutterHunk, text: &str, cx: &mut gpui::App) {
    // Express the hunk in buffer lines so `revert_hunk` finds them.
    let in_buffer = Hunk {
        new_start: hunk.lines.start,
        new_lines: hunk.lines.len(),
        ..hunk.hunk.clone()
    };
    let revert = revert_hunk(&in_buffer, text);
    let start = text[..revert.bytes.start].chars().count();
    let end = start + text[revert.bytes.clone()].chars().count();
    editor.update(cx, |e, cx| {
        let range = e.offset_to_point(start)..e.offset_to_point(end);
        e.apply_edits(vec![(range, revert.text.clone())], cx);
    });
}
