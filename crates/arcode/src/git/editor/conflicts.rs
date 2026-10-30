//! Merge conflict regions: highlights, the Accept Current | Accept
//! Incoming | Accept Both | Compare actions, and conflict navigation.

use std::ops::Range;

use arcode_buffer::Point;
use arcode_editor::{
    BackgroundHighlight, CodeLens, ColorKey, Editor, LensItem, LensPlacement, ScrollAnchor,
};
use arcode_git::Conflict;
use gpui::{Context, Entity, EntityId};

use super::{CONFLICT_SET, GitEditors, GitEditorsEvent};

/// What a conflict action does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictAction {
    /// Keep our side (`HEAD`).
    AcceptCurrent,
    /// Keep their side (the merged branch).
    AcceptIncoming,
    /// Keep ours followed by theirs.
    AcceptBoth,
    /// Open a diff of ours against theirs.
    Compare,
}

impl ConflictAction {
    const ALL: [ConflictAction; 4] = [
        ConflictAction::AcceptCurrent,
        ConflictAction::AcceptIncoming,
        ConflictAction::AcceptBoth,
        ConflictAction::Compare,
    ];

    fn label(self) -> &'static str {
        match self {
            ConflictAction::AcceptCurrent => "Accept Current",
            ConflictAction::AcceptIncoming => "Accept Incoming",
            ConflictAction::AcceptBoth => "Accept Both",
            ConflictAction::Compare => "Compare",
        }
    }
}

/// Lens ids encode the conflict index and the action.
fn lens_id(conflict: usize, action: usize) -> u64 {
    (conflict * 8 + action) as u64
}

/// The highlights of `conflicts`: marker lines stronger, our side in the
/// "added" color, theirs in the "info" color, the base in "warning".
pub fn conflict_highlights(conflicts: &[Conflict]) -> Vec<BackgroundHighlight> {
    let mut out = Vec::new();
    for c in conflicts {
        let marker = |line: usize, key: &'static str| {
            BackgroundHighlight::lines(line..line + 1, ColorKey::new(key).opacity(0.35))
        };
        out.push(marker(c.start_line, "diff_added"));
        out.push(BackgroundHighlight::lines(
            c.ours.clone(),
            ColorKey::new("diff_added").opacity(0.15),
        ));
        if let Some(base) = &c.base {
            out.push(marker(base.start.saturating_sub(1), "warning"));
            out.push(BackgroundHighlight::lines(
                base.clone(),
                ColorKey::new("warning").opacity(0.12),
            ));
        }
        out.push(marker(c.theirs.start.saturating_sub(1), "text_muted"));
        out.push(BackgroundHighlight::lines(
            c.theirs.clone(),
            ColorKey::new("info").opacity(0.15),
        ));
        out.push(marker(c.end_line, "info"));
    }
    out
}

/// The action lenses above each conflict, with a `n/total` counter.
pub fn conflict_lenses(conflicts: &[Conflict]) -> Vec<CodeLens> {
    let total = conflicts.len();
    conflicts
        .iter()
        .enumerate()
        .map(|(ix, c)| {
            let mut items: Vec<LensItem> = ConflictAction::ALL
                .iter()
                .enumerate()
                .map(|(a, action)| LensItem {
                    id: lens_id(ix, a),
                    label: action.label().into(),
                })
                .collect();
            items.push(LensItem {
                id: lens_id(ix, 7),
                label: format!("Conflict {} of {total}", ix + 1).into(),
            });
            CodeLens {
                line: c.start_line,
                placement: LensPlacement::Above,
                items,
            }
        })
        .collect()
}

/// The replacement text for resolving `conflict` with `action` (lines of
/// `text`, each with its `\n`), or `None` for [`ConflictAction::Compare`].
pub fn resolution_text(text: &str, conflict: &Conflict, action: ConflictAction) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let take = |r: &Range<usize>| -> String {
        lines[r.start.min(lines.len())..r.end.min(lines.len())].concat()
    };
    let mut out = match action {
        ConflictAction::AcceptCurrent => take(&conflict.ours),
        ConflictAction::AcceptIncoming => take(&conflict.theirs),
        ConflictAction::AcceptBoth => {
            let mut ours = take(&conflict.ours);
            if !ours.is_empty() && !ours.ends_with('\n') {
                ours.push('\n');
            }
            ours + &take(&conflict.theirs)
        }
        ConflictAction::Compare => return None,
    };
    // The region ends with the `>>>>>>>` line's break; keep one if the
    // kept text lost its own (the side was the file's unterminated end).
    let region_ends_with_break = lines
        .get(conflict.end_line)
        .is_some_and(|l| l.ends_with('\n'));
    if region_ends_with_break && !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

impl GitEditors {
    pub(crate) fn apply_conflicts(
        &mut self,
        id: EntityId,
        editor: &Entity<Editor>,
        conflicts: Vec<Conflict>,
        version: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.editors.get_mut(&id) else {
            return;
        };
        if state.conflicts_version.is_some_and(|v| v > version) {
            // A slower background parse of an older text: already superseded.
            return;
        }
        state.conflicts_version = Some(version);
        if state.conflicts == conflicts {
            return;
        }
        let had = !state.conflicts.is_empty();
        let highlights = conflict_highlights(&conflicts);
        let lenses = conflict_lenses(&conflicts);
        let resolved_all = had && conflicts.is_empty();
        state.conflicts = conflicts;
        editor.update(cx, |e, cx| {
            e.set_background_highlights(CONFLICT_SET, highlights, cx);
            e.set_code_lenses(CONFLICT_SET, lenses, cx);
        });
        if resolved_all {
            cx.emit(GitEditorsEvent::Message(
                "All conflicts resolved. Save and stage the file to mark it resolved.".into(),
            ));
        }
        cx.emit(GitEditorsEvent::ConflictsChanged);
    }

    pub(crate) fn conflict_lens_clicked(
        &mut self,
        editor: &Entity<Editor>,
        line: usize,
        lens: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(action) = ConflictAction::ALL.get((lens % 8) as usize).copied() else {
            return;
        };
        // The lens may belong to conflicts parsed before the latest edit
        // (the re-parse is debounced); the editor keeps the lens on its
        // line, so find the conflict that starts there now.
        self.sync_conflicts(editor, cx);
        let ix = self
            .conflicts(editor.entity_id())
            .iter()
            .position(|c| c.start_line == line);
        if let Some(ix) = ix {
            self.resolve_conflict(editor, ix, action, cx);
        }
    }

    /// Re-parses the conflicts of `editor` right away if the buffer changed
    /// since they were last parsed.
    fn sync_conflicts(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let id = editor.entity_id();
        let version = editor.read(cx).buffer().version();
        let Some(state) = self.editors.get(&id) else {
            return;
        };
        if state.conflicts_version != Some(version) {
            let text = editor.read(cx).buffer().text().to_string();
            let conflicts = arcode_git::parse_conflicts(&text);
            self.apply_conflicts(id, editor, conflicts, version, cx);
        }
    }

    /// Resolves conflict `ix` of `editor` (or opens the comparison). Refused
    /// when the buffer changed since the conflicts were parsed, as `ix`
    /// and the line numbers may no longer match the text.
    pub fn resolve_conflict(
        &mut self,
        editor: &Entity<Editor>,
        ix: usize,
        action: ConflictAction,
        cx: &mut Context<Self>,
    ) {
        let id = editor.entity_id();
        let version = editor.read(cx).buffer().version();
        let Some(state) = self.editors.get(&id) else {
            return;
        };
        if state.conflicts_version != Some(version) {
            cx.emit(GitEditorsEvent::Message(
                "The file changed; try again in a moment".into(),
            ));
            self.sync_conflicts(editor, cx);
            return;
        }
        let Some(conflict) = state.conflicts.get(ix).cloned() else {
            return;
        };
        let text = editor.read(cx).buffer().text().to_string();
        if action == ConflictAction::Compare {
            let ours = resolution_text(&text, &conflict, ConflictAction::AcceptCurrent);
            let theirs = resolution_text(&text, &conflict, ConflictAction::AcceptIncoming);
            let path = editor.read(cx).path().map(|p| p.to_path_buf());
            let name = path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            cx.emit(GitEditorsEvent::OpenDiff {
                title: format!(
                    "{name}: {} ↔ {}",
                    label_or(&conflict.ours_label, "Current"),
                    label_or(&conflict.theirs_label, "Incoming")
                ),
                path,
                old: ours.unwrap_or_default(),
                new: theirs.unwrap_or_default(),
            });
            return;
        }
        let Some(replacement) = resolution_text(&text, &conflict, action) else {
            return;
        };
        editor.update(cx, |e, cx| {
            let range = line_range(e, conflict.lines());
            e.apply_edits(vec![(range, replacement)], cx);
        });
        // Update the lenses now, so a quick second click acts on the
        // conflicts as they are after this one.
        self.sync_conflicts(editor, cx);
    }

    /// Moves the cursor of `editor` to the next (or previous) conflict,
    /// wrapping around. Returns false when it has none.
    pub fn go_to_conflict(
        &mut self,
        editor: &Entity<Editor>,
        forward: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let conflicts = self.conflicts(editor.entity_id());
        let cursor = editor.read(cx).cursor_position().line;
        let starts: Vec<usize> = conflicts.iter().map(|c| c.start_line).collect();
        let Some(target) = next_line(&starts, cursor, forward) else {
            return false;
        };
        editor.update(cx, |e, cx| {
            let at = Point::new(target, 0);
            e.select_ranges(vec![at..at], cx);
            e.scroll_to_line(target, ScrollAnchor::Center, cx);
        });
        true
    }
}

/// The next start after `cursor` (or the previous one before it), wrapping.
pub fn next_line(starts: &[usize], cursor: usize, forward: bool) -> Option<usize> {
    if forward {
        starts
            .iter()
            .copied()
            .find(|&s| s > cursor)
            .or(starts.first().copied())
    } else {
        starts
            .iter()
            .rev()
            .copied()
            .find(|&s| s < cursor)
            .or(starts.last().copied())
    }
}

fn label_or<'a>(label: &'a str, fallback: &'a str) -> &'a str {
    if label.is_empty() { fallback } else { label }
}

/// The buffer range covering whole `lines` (to the end of the buffer when
/// the range reaches past the last line).
fn line_range(editor: &Editor, lines: Range<usize>) -> Range<Point> {
    let count = editor.line_count();
    let start = Point::new(lines.start, 0);
    let end = if lines.end < count {
        Point::new(lines.end, 0)
    } else {
        let len = editor.buffer().text().len_chars();
        editor.offset_to_point(len)
    };
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "a\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> feature\nz\n";

    #[test]
    fn resolutions_keep_the_chosen_side() {
        let c = &arcode_git::parse_conflicts(TEXT)[0];
        let r = |a| resolution_text(TEXT, c, a).unwrap();
        assert_eq!(r(ConflictAction::AcceptCurrent), "ours\n");
        assert_eq!(r(ConflictAction::AcceptIncoming), "theirs\n");
        assert_eq!(r(ConflictAction::AcceptBoth), "ours\ntheirs\n");
        assert!(resolution_text(TEXT, c, ConflictAction::Compare).is_none());
    }

    #[test]
    fn lenses_and_highlights_cover_each_conflict() {
        let conflicts = arcode_git::parse_conflicts(&format!("{TEXT}{TEXT}"));
        let lenses = conflict_lenses(&conflicts);
        assert_eq!(lenses.len(), 2);
        assert_eq!(
            lenses[1].items.last().unwrap().label.as_ref(),
            "Conflict 2 of 2"
        );
        assert_eq!(lenses[1].items[1].id, lens_id(1, 1));
        assert_eq!(conflict_highlights(&conflicts).len(), 10);
    }

    #[test]
    fn navigation_wraps() {
        assert_eq!(next_line(&[3, 10], 0, true), Some(3));
        assert_eq!(next_line(&[3, 10], 3, true), Some(10));
        assert_eq!(next_line(&[3, 10], 12, true), Some(3));
        assert_eq!(next_line(&[3, 10], 10, false), Some(3));
        assert_eq!(next_line(&[3, 10], 2, false), Some(10));
        assert_eq!(next_line(&[], 2, false), None);
    }
}
