//! Read-only inline diff tabs: both versions interleaved in an editor,
//! removed lines tinted red with a minus in the gutter, added lines green.

use arcode_buffer::Buffer;
use arcode_editor::{
    BackgroundHighlight, ChangeKind, ColorKey, Editor, GutterDecoration, GutterLane, GutterMark,
};
use arcode_git::{DiffLineKind, InlineDiff, inline_diff};
use arcode_ui_kit::IconName;
use gpui::{AppContext, Context, Entity};

/// Set key of the diff decorations.
pub const DIFF_SET: &str = "git-diff";

/// Highlights and gutter marks for `diff`.
pub fn diff_decorations(diff: &InlineDiff) -> (Vec<BackgroundHighlight>, Vec<GutterDecoration>) {
    let mut highlights = Vec::new();
    let mut marks = Vec::new();
    let mut ix = 0;
    while ix < diff.kinds.len() {
        let kind = diff.kinds[ix];
        let start = ix;
        while ix < diff.kinds.len() && diff.kinds[ix] == kind {
            ix += 1;
        }
        let (key, mark) = match kind {
            DiffLineKind::Context => continue,
            DiffLineKind::Added => ("diff_added", GutterMark::Change(ChangeKind::Added)),
            DiffLineKind::Removed => (
                "diff_deleted",
                GutterMark::Icon {
                    icon: IconName::Minus,
                    color: ColorKey::new("diff_deleted"),
                },
            ),
        };
        highlights.push(BackgroundHighlight::lines(
            start..ix,
            ColorKey::new(key).opacity(0.18),
        ));
        marks.extend((start..ix).map(|line| GutterDecoration {
            line,
            lane: GutterLane::Changes,
            mark: mark.clone(),
            tag: line as u64,
        }));
    }
    (highlights, marks)
}

/// A summary like `+3 −1` for a diff.
pub fn diff_stats(diff: &InlineDiff) -> (usize, usize) {
    let added = diff
        .kinds
        .iter()
        .filter(|k| **k == DiffLineKind::Added)
        .count();
    let removed = diff
        .kinds
        .iter()
        .filter(|k| **k == DiffLineKind::Removed)
        .count();
    (added, removed)
}

/// Creates a read-only editor showing `old` against `new`. Building the
/// diff is linear in the text size; call this with texts read off the main
/// thread.
pub fn diff_editor<T: 'static>(old: &str, new: &str, cx: &mut Context<T>) -> Entity<Editor> {
    let diff = inline_diff(old, new);
    let (highlights, marks) = diff_decorations(&diff);
    let first_change = diff.change_starts().first().copied();
    // No path: the tab can never be saved over a real file.
    let buffer = Buffer::new(&diff.text);
    cx.new(|cx| {
        let mut editor = Editor::new(buffer, cx);
        editor.set_read_only(true, cx);
        editor.set_background_highlights(DIFF_SET, highlights, cx);
        editor.set_gutter_decorations(DIFF_SET, marks, cx);
        if let Some(line) = first_change {
            editor.scroll_to_line(line, arcode_editor::ScrollAnchor::Center, cx);
        }
        editor
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorations_follow_line_kinds() {
        let diff = inline_diff("a\nb\nc\n", "a\nB\nc\nd\n");
        let (highlights, marks) = diff_decorations(&diff);
        assert_eq!(highlights.len(), 3, "removed b, added B, added d");
        assert_eq!(marks.len(), 3);
        assert!(matches!(marks[0].mark, GutterMark::Icon { .. }));
        assert_eq!(diff_stats(&diff), (2, 1));
    }
}
