//! Reloading open editors whose files changed on disk (an external edit,
//! a git checkout, a formatter run). Clean editors follow the file: the
//! file is read and diffed on the background executor and the change is
//! applied as one undo step, so cursors and scrolling stay where they
//! were. Editors with unsaved edits keep them.

use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use arcode_buffer::{Buffer, Point};
use arcode_editor::Editor;
use gpui::{App, AppContext, Context, Entity};

use super::Workspace;

impl Workspace {
    /// Reloads every clean editor showing one of `paths` (all of them on
    /// `overflow`, when the watcher lost track of which files changed).
    pub(super) fn reload_changed_editors(
        &mut self,
        paths: &[PathBuf],
        overflow: bool,
        cx: &mut Context<Self>,
    ) {
        let changed: HashSet<&Path> = paths.iter().map(PathBuf::as_path).collect();
        for editor in self.editors(cx) {
            let e = editor.read(cx);
            let wanted = e.path().is_some_and(|p| overflow || changed.contains(p));
            if wanted && !e.is_dirty() {
                reload_editor(&editor, cx);
            }
        }
    }
}

/// Reads `editor`'s file in the background and applies the difference if
/// the editor is still clean and unchanged by then (it tries again when it
/// is clean but changed meanwhile, e.g. by an earlier reload).
pub(crate) fn reload_editor(editor: &Entity<Editor>, cx: &mut App) {
    let e = editor.read(cx);
    let Some(path) = e.path().map(Path::to_path_buf) else {
        return;
    };
    let rope = e.buffer().text().clone();
    let version = e.buffer().version();
    let line_ending = e.buffer().line_ending();
    let weak = editor.downgrade();
    cx.spawn(async move |cx| {
        let work = cx.background_spawn(async move {
            let disk = Buffer::from_file(&path).ok()?;
            let new = disk.to_string();
            let old = rope.to_string();
            if old == new && disk.line_ending() == line_ending {
                return None;
            }
            Some((reload_edits(&old, &new), disk.line_ending()))
        });
        let Some((edits, ending)) = work.await else {
            return;
        };
        let retry = weak
            .update(cx, |e, cx| {
                !e.apply_disk_reload(version, edits, ending, cx) && !e.is_dirty()
            })
            .unwrap_or(false);
        if retry && let Some(editor) = weak.upgrade() {
            let _ = cx.update(|cx| reload_editor(&editor, cx));
        }
    })
    .detach();
}

/// Edits (point ranges in `old`, with their replacement) that turn `old`
/// into `new`, both with `\n` line breaks: one per changed block of lines,
/// so text between the blocks (and cursors in it) stays put.
pub(crate) fn reload_edits(old: &str, new: &str) -> Vec<(Range<Point>, String)> {
    let byte_edits = line_edits(old, new)
        .filter(|edits| apply(old, edits).as_deref() == Some(new))
        .unwrap_or_else(|| vec![prefix_suffix_edit(old, new)]);
    let starts = line_starts(old);
    let point = |byte: usize| {
        let line = starts.partition_point(|&s| s <= byte).saturating_sub(1);
        Point::new(line, old[starts[line]..byte].chars().count())
    };
    byte_edits
        .into_iter()
        .filter(|(range, text)| !range.is_empty() || !text.is_empty())
        .map(|(range, text)| (point(range.start)..point(range.end), text))
        .collect()
}

/// The changed line blocks from the git line diff as byte edits, or
/// `None` when the text has line breaks the buffer does not split on.
fn line_edits(old: &str, new: &str) -> Option<Vec<(Range<usize>, String)>> {
    let odd = |t: &str| t.contains(['\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}']);
    if odd(old) || odd(new) {
        return None;
    }
    let (os, ns) = (line_starts(old), line_starts(new));
    let at =
        |starts: &[usize], text: &str, line: usize| starts.get(line).copied().unwrap_or(text.len());
    let edits = arcode_git::line_diff(old, new)
        .into_iter()
        .map(|c| {
            let range = at(&os, old, c.old_lines.start)..at(&os, old, c.old_lines.end);
            let text = &new[at(&ns, new, c.new_lines.start)..at(&ns, new, c.new_lines.end)];
            (range, text.to_string())
        })
        .collect();
    Some(edits)
}

/// One edit replacing everything between the common prefix and suffix.
fn prefix_suffix_edit(old: &str, new: &str) -> (Range<usize>, String) {
    let prefix = old
        .char_indices()
        .zip(new.chars())
        .find(|((_, a), b)| a != b)
        .map_or(old.len().min(new.len()), |((i, _), _)| i);
    let (a, b) = (&old[prefix..], &new[prefix..]);
    let suffix: usize = a
        .chars()
        .rev()
        .zip(b.chars().rev())
        .take_while(|(x, y)| x == y)
        .map(|(x, _)| x.len_utf8())
        .sum();
    (
        prefix..old.len() - suffix,
        new[prefix..new.len() - suffix].to_string(),
    )
}

/// Applies sorted, non-overlapping byte edits to `text` (`None` if they
/// are not).
fn apply(text: &str, edits: &[(Range<usize>, String)]) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (range, insert) in edits {
        if range.start < at || range.end < range.start || range.end > text.len() {
            return None;
        }
        out.push_str(&text[at..range.start]);
        out.push_str(insert);
        at = range.end;
    }
    out.push_str(&text[at..]);
    Some(out)
}

/// Byte offset where each `\n`-separated line starts.
fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

#[cfg(test)]
#[path = "reload_tests.rs"]
mod tests;
