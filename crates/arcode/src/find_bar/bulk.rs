//! Replacing every match of a buffer in one go (find bar Replace All, and
//! the Search view's Replace All for open files).
//!
//! The query is compiled once for all lines. Up to [`MAX_EDITS`] matches
//! become one exact edit each; beyond that, neighbouring matched lines are
//! merged into at most [`MAX_EDITS`] whole-line edits, so the editor never
//! has to apply (and re-map selections, highlights and syntax through)
//! tens of thousands of small edits. Either way it is one undo step.

use std::ops::Range;

use arcode_buffer::Point;
use arcode_editor::Editor;
use arcode_search::{LineReplacer, SearchQuery, search_text};
use gpui::{App, Entity};

/// Most edits handed to the editor for one Replace All.
pub const MAX_EDITS: usize = 256;

impl BulkReplace {
    /// Applies the edits to `editor` as one undo step. Merged whole-line
    /// edits would carry a cursor inside them to their end (hundreds of
    /// lines away), so the cursor is put back where it was. Returns whether
    /// the editor took the edits.
    pub fn apply(self, editor: &Entity<Editor>, cx: &mut App) -> bool {
        if self.edits.is_empty() {
            return false;
        }
        let merged = self.edits.len() < self.count;
        editor.update(cx, |e, cx| {
            let sel = e.selections().primary();
            let cursor = e.offset_to_point(sel.head);
            let applied = e.apply_edits(self.edits, cx);
            if applied && merged {
                e.select_ranges(vec![cursor..cursor], cx);
            }
            applied
        })
    }
}

/// A line number and its replacements (char ranges and new text).
type LineReps = (usize, Vec<(Range<usize>, String)>);

/// Edits replacing every match of a text, and how many matches they cover.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BulkReplace {
    /// Editor edits (buffer points), sorted and non-overlapping.
    pub edits: Vec<(Range<Point>, String)>,
    /// Matches replaced by `edits`.
    pub count: usize,
}

/// Edits replacing every non-empty match of `query` in `text` (lines split
/// on `\n`, as in the editor buffer) by `replacement`, with at most
/// `max_edits` edits. `query.max_results` should be 0 (unlimited).
pub fn replace_all_edits(
    text: &str,
    query: &SearchQuery,
    replacement: &str,
    max_edits: usize,
) -> arcode_search::Result<BulkReplace> {
    let replacer = LineReplacer::new(query, replacement)?;
    let found = search_text(text, query)?;
    let lines: Vec<&str> = text.split('\n').collect();
    let mut per_line: Vec<LineReps> = Vec::new();
    let mut count = 0;
    for line in found.iter().map(|l| l.line) {
        if per_line.last().is_some_and(|(l, _)| *l == line) {
            continue;
        }
        let Some(content) = lines.get(line) else {
            continue;
        };
        let reps = replacer.replacements(content)?;
        if !reps.is_empty() {
            count += reps.len();
            per_line.push((line, reps));
        }
    }
    let edits = if count <= max_edits.max(1) {
        per_line
            .into_iter()
            .flat_map(|(line, reps)| {
                reps.into_iter()
                    .map(move |(r, new)| (Point::new(line, r.start)..Point::new(line, r.end), new))
            })
            .collect()
    } else {
        merged_edits(&lines, &per_line, max_edits.max(1))
    };
    Ok(BulkReplace { edits, count })
}

/// Groups the matched lines into at most `max_edits` runs; each run becomes
/// one edit over its whole lines (unmatched lines in between are copied).
fn merged_edits(
    lines: &[&str],
    per_line: &[LineReps],
    max_edits: usize,
) -> Vec<(Range<Point>, String)> {
    let group = per_line.len().div_ceil(max_edits).max(1);
    per_line
        .chunks(group)
        .filter_map(|chunk| {
            let first = chunk.first()?.0;
            let last = chunk.last()?.0;
            let mut out = String::new();
            let mut matched = chunk.iter().peekable();
            for line in first..=last {
                if line > first {
                    out.push('\n');
                }
                let content = lines.get(line).copied().unwrap_or("");
                match matched.next_if(|(l, _)| *l == line) {
                    Some((_, reps)) => apply_line(content, reps, &mut out),
                    None => out.push_str(content),
                }
            }
            let end = Point::new(last, lines.get(last).map_or(0, |l| l.chars().count()));
            Some((Point::new(first, 0)..end, out))
        })
        .collect()
}

/// Appends `line` with the char ranges of `reps` (sorted, disjoint)
/// replaced to `out`.
fn apply_line(line: &str, reps: &[(Range<usize>, String)], out: &mut String) {
    let mut starts = line.char_indices().map(|(b, _)| b).chain([line.len()]);
    let mut char_pos = 0usize;
    let mut byte_pos = starts.next().unwrap_or(line.len());
    let mut to_byte = |c: usize| {
        while char_pos < c {
            byte_pos = starts.next().unwrap_or(line.len());
            char_pos += 1;
        }
        byte_pos
    };
    let mut copied = 0;
    for (r, new) in reps {
        let start = to_byte(r.start).max(copied);
        let end = to_byte(r.end).max(start);
        out.push_str(&line[copied..start]);
        out.push_str(new);
        copied = end;
    }
    out.push_str(&line[copied..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: usize, column: usize) -> Point {
        Point::new(line, column)
    }

    #[test]
    fn few_matches_become_exact_edits() {
        let q = SearchQuery {
            is_regex: true,
            max_results: 0,
            ..SearchQuery::new(r"(\d+)")
        };
        let r = replace_all_edits("ä1 b22\nx\n3", &q, "<$1>", 10).unwrap();
        assert_eq!(r.count, 3);
        assert_eq!(
            r.edits,
            vec![
                (p(0, 1)..p(0, 2), "<1>".to_owned()),
                (p(0, 4)..p(0, 6), "<22>".to_owned()),
                (p(2, 0)..p(2, 1), "<3>".to_owned()),
            ]
        );
    }

    #[test]
    fn many_matches_are_merged_into_whole_line_edits() {
        let text = "aXb\nnone\nXäX\nX\nend";
        let q = SearchQuery {
            max_results: 0,
            ..SearchQuery::new("X")
        };
        let r = replace_all_edits(text, &q, "yy", 2).unwrap();
        assert_eq!(r.count, 4);
        assert_eq!(
            r.edits,
            vec![
                (p(0, 0)..p(2, 3), "ayyb\nnone\nyyäyy".to_owned()),
                (p(3, 0)..p(3, 1), "yy".to_owned()),
            ]
        );
        let none = replace_all_edits("abc", &q, "yy", 2).unwrap();
        assert_eq!(none, BulkReplace::default());
    }
}
