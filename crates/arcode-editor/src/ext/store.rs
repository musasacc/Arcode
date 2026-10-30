//! Storage for decorations, popovers and interceptors, and how positions
//! follow buffer edits.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use arcode_buffer::{Edit, Point};
use gpui::SharedString;

use super::{
    BackgroundHighlight, CodeLens, CursorShape, EditorPopover, GutterDecoration, GutterLane,
    HighlightRange, InterceptorId, KeyInterceptor, LensPlacement, VirtualText, VirtualTextPosition,
};

/// A hovered code lens item: set, line and item id.
pub(crate) type LensTarget = (SharedString, usize, u64);

/// Everything owners attached to one editor.
#[derive(Default)]
pub(crate) struct ExtState {
    pub gutter: BTreeMap<SharedString, Vec<GutterDecoration>>,
    pub lanes_shown: BTreeSet<GutterLane>,
    pub highlights: BTreeMap<SharedString, Vec<BackgroundHighlight>>,
    pub virtual_text: BTreeMap<SharedString, Vec<VirtualText>>,
    pub lenses: BTreeMap<SharedString, Vec<CodeLens>>,
    pub popovers: Vec<(SharedString, EditorPopover)>,
    pub interceptors: Vec<(InterceptorId, i32, KeyInterceptor)>,
    pub cursor_shape: CursorShape,
    pub hovered_lens: Option<LensTarget>,
    pub next_id: u64,
}

/// Where `line` ends up after `edit`, or `None` if it was removed.
pub(crate) fn map_line(line: usize, edit: &Edit) -> Option<usize> {
    let (start, old_end, new_end) = (edit.start_point, edit.old_end_point, edit.new_end_point);
    if line < start.line {
        return Some(line);
    }
    if line > old_end.line {
        return Some(line - old_end.line + new_end.line);
    }
    if start.line == old_end.line {
        // Text inserted at column 0 pushes the line's content down.
        if start.column == 0 && start == old_end {
            return Some(new_end.line);
        }
        return Some(line);
    }
    // A multi-line replacement: the text after it on the old end line
    // continues on the new end line; the start line survives unless the
    // edit began at its column 0; lines in between are gone.
    if line == old_end.line {
        Some(new_end.line)
    } else if line == start.line && start.column > 0 {
        Some(line)
    } else {
        None
    }
}

/// The buffer line a virtual text is on.
fn text_line(v: &VirtualText) -> usize {
    match v.position {
        VirtualTextPosition::EndOfLine(l) => l,
        VirtualTextPosition::Inline(p) => p.line,
    }
}

/// Where `p` ends up after `edit`; `None` if it was inside replaced text.
pub(crate) fn map_point(p: Point, edit: &Edit) -> Option<Point> {
    let (start, old_end, new_end) = (edit.start_point, edit.old_end_point, edit.new_end_point);
    if p < start {
        return Some(p);
    }
    if p >= old_end {
        return Some(if p.line == old_end.line {
            Point::new(new_end.line, new_end.column + (p.column - old_end.column))
        } else {
            Point::new(p.line - old_end.line + new_end.line, p.column)
        });
    }
    None
}

/// Where the range `r` ends up after `edit`; `None` if the edit replaced
/// text across one of its ends (or all of it). Text inserted exactly at an
/// end stays outside the range: the start has a right bias, the end a left
/// bias, so a search match on `foo` does not grow into `foox`.
pub(crate) fn map_range(r: &Range<Point>, edit: &Edit) -> Option<Range<Point>> {
    let (start, old_end) = (edit.start_point, edit.old_end_point);
    if old_end <= r.start {
        // Entirely before the range (or inserted at its start).
        return Some(map_point(r.start, edit)?..map_point(r.end, edit)?);
    }
    if start >= r.end {
        // Entirely after the range (or inserted at its end).
        return Some(r.clone());
    }
    if r.start <= start && old_end <= r.end {
        // Inside the range: the start stays, the end follows the edit. A
        // range whose whole text was deleted is dropped.
        let end = map_point(r.end, edit)?;
        return (end > r.start || r.is_empty()).then_some(r.start..end);
    }
    None
}

/// Where the whole-line range `lines` ends up after `edit`; `None` when
/// every line of it was deleted.
pub(crate) fn map_lines(lines: &Range<usize>, edit: &Edit) -> Option<Range<usize>> {
    let (start, old_end) = (edit.start_point.line, edit.old_end_point.line);
    let last = lines.end.saturating_sub(1).max(lines.start);
    // First surviving line: the start itself, or else the edit's old end
    // line (whose tail continues the text) if it is still in the range.
    let first = map_line(lines.start, edit)
        .or_else(|| (old_end <= last).then(|| map_line(old_end, edit)).flatten())?;
    // Last surviving line: the last itself, or else the edit's start line
    // if it is in the range and survives, or else the line before it.
    let end = map_line(last, edit).or_else(|| {
        if start < lines.start {
            return None;
        }
        map_line(start, edit).or_else(|| (start > lines.start).then(|| start - 1))
    })?;
    Some(first..end.max(first) + 1)
}

impl ExtState {
    /// Allocates a fresh id.
    pub fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Moves every position through `edit`, dropping what it deleted.
    pub fn adjust_for_edit(&mut self, edit: &Edit) {
        for marks in self.gutter.values_mut() {
            marks.retain_mut(|m| map_line(m.line, edit).map(|l| m.line = l).is_some());
        }
        for list in self.highlights.values_mut() {
            list.retain_mut(|h| match &mut h.range {
                HighlightRange::Lines(lines) => map_lines(lines, edit).map(|n| *lines = n).is_some(),
                HighlightRange::Text(r) => map_range(r, edit).map(|n| *r = n).is_some(),
            });
        }
        for list in self.virtual_text.values_mut() {
            list.retain_mut(|v| match &mut v.position {
                VirtualTextPosition::EndOfLine(l) => map_line(*l, edit).map(|n| *l = n).is_some(),
                VirtualTextPosition::Inline(p) => map_point(*p, edit).map(|n| *p = n).is_some(),
            });
        }
        for list in self.lenses.values_mut() {
            list.retain_mut(|c| map_line(c.line, edit).map(|l| c.line = l).is_some());
        }
        for (_, popover) in &mut self.popovers {
            popover.anchor = map_point(popover.anchor, edit).unwrap_or(edit.start_point);
        }
        self.hovered_lens = None;
    }

    /// Lanes that take space: forced visible or holding decorations.
    pub fn active_lanes(&self) -> Vec<GutterLane> {
        let mut lanes: BTreeSet<GutterLane> = self.lanes_shown.clone();
        for marks in self.gutter.values() {
            lanes.extend(marks.iter().map(|m| m.lane));
        }
        lanes.into_iter().collect()
    }

    /// Gutter decorations on `lines`, in set order.
    pub fn gutter_in(&self, lines: Range<usize>) -> Vec<&GutterDecoration> {
        let mut out = Vec::new();
        for marks in self.gutter.values() {
            let from = marks.partition_point(|m| m.line < lines.start);
            out.extend(marks[from..].iter().take_while(|m| m.line < lines.end));
        }
        out
    }

    /// Virtual texts of every set on `line` (each set is sorted by line,
    /// so this is a binary search per set).
    fn virtual_text_on(&self, line: usize) -> impl Iterator<Item = &VirtualText> {
        self.virtual_text.values().flat_map(move |list| {
            let from = list.partition_point(|v| text_line(v) < line);
            list[from..]
                .iter()
                .take_while(move |v| text_line(v) == line)
        })
    }

    /// Inline virtual texts on `line`, sorted by column.
    pub fn inline_text(&self, line: usize) -> Vec<(usize, &VirtualText)> {
        let mut out: Vec<(usize, &VirtualText)> = self
            .virtual_text_on(line)
            .filter_map(|v| match v.position {
                VirtualTextPosition::Inline(p) => Some((p.column, v)),
                VirtualTextPosition::EndOfLine(_) => None,
            })
            .collect();
        out.sort_by_key(|(col, _)| *col);
        out
    }

    /// End-of-line virtual texts on `line`.
    pub fn eol_text(&self, line: usize) -> impl Iterator<Item = &VirtualText> {
        self.virtual_text_on(line)
            .filter(|v| matches!(v.position, VirtualTextPosition::EndOfLine(_)))
    }

    /// Code lenses on `line` with `placement`, with their set keys.
    pub fn lenses_on(
        &self,
        line: usize,
        placement: LensPlacement,
    ) -> impl Iterator<Item = (&SharedString, &CodeLens)> {
        self.lenses.iter().flat_map(move |(set, list)| {
            let from = list.partition_point(|c| c.line < line);
            list[from..]
                .iter()
                .take_while(move |c| c.line == line)
                .filter(move |c| c.placement == placement)
                .map(move |c| (set, c))
        })
    }

    /// Lines that have a lens row above them.
    pub fn block_lines(&self) -> Vec<usize> {
        self.lenses
            .values()
            .flatten()
            .filter(|c| c.placement == LensPlacement::Above && !c.items.is_empty())
            .map(|c| c.line)
            .collect()
    }

    /// Whether any line has inline or end-of-line decorations to shape.
    pub fn has_line_extras(&self) -> bool {
        self.virtual_text.values().any(|v| !v.is_empty())
            || self.lenses.values().any(|v| !v.is_empty())
    }

    /// Interceptors to try for a keystroke, highest priority first:
    /// handlers of popovers that are drawn (`drawn(anchor line)`), newest
    /// first, then installed interceptors. A popover scrolled out of view
    /// is not drawn, so its list must not take Enter or the arrows.
    pub fn key_handlers(&self, drawn: impl Fn(usize) -> bool) -> Vec<KeyInterceptor> {
        let mut out: Vec<KeyInterceptor> = self
            .popovers
            .iter()
            .rev()
            .filter(|(_, p)| drawn(p.anchor.line))
            .filter_map(|(_, p)| p.on_key.clone())
            .collect();
        let mut installed: Vec<&(InterceptorId, i32, KeyInterceptor)> =
            self.interceptors.iter().collect();
        installed.sort_by_key(|(id, prio, _)| (std::cmp::Reverse(*prio), id.0));
        out.extend(installed.into_iter().map(|(_, _, f)| f.clone()));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_buffer::Buffer;

    fn edit(text: &str, range: Range<usize>, new: &str) -> Edit {
        let mut b = Buffer::new(text);
        b.edit(vec![(range, new.to_string())]).remove(0)
    }

    #[test]
    fn lines_follow_inserted_and_deleted_lines() {
        let text = "a\nb\nc\nd\n";
        // Insert a line at the start of line 1.
        let e = edit(text, 2..2, "x\n");
        assert_eq!(map_line(0, &e), Some(0));
        assert_eq!(map_line(1, &e), Some(2));
        assert_eq!(map_line(3, &e), Some(4));
        // Delete lines 1..3 ("b\nc\n").
        let e = edit(text, 2..6, "");
        assert_eq!(map_line(1, &e), None);
        assert_eq!(map_line(2, &e), None);
        assert_eq!(map_line(3, &e), Some(1));
        // Join lines 0 and 1 by deleting the newline.
        let e = edit(text, 1..2, "");
        assert_eq!(map_line(0, &e), Some(0));
        assert_eq!(map_line(1, &e), Some(0));
        assert_eq!(map_line(2, &e), Some(1));
    }

    #[test]
    fn points_shift_or_drop() {
        let e = edit("let x = 1;\n", 4..5, "long");
        assert_eq!(map_point(Point::new(0, 2), &e), Some(Point::new(0, 2)));
        assert_eq!(map_point(Point::new(0, 5), &e), Some(Point::new(0, 8)));
        assert_eq!(map_point(Point::new(0, 4), &e), None);
        let e = edit("abcdef", 1..4, "");
        assert_eq!(map_point(Point::new(0, 2), &e), None);
        assert_eq!(map_point(Point::new(0, 5), &e), Some(Point::new(0, 2)));
    }

    #[test]
    fn line_highlights_shrink_and_grow() {
        let mut s = ExtState::default();
        s.highlights.insert(
            "conflict".into(),
            vec![BackgroundHighlight::lines(1..3, "warning".into())],
        );
        s.adjust_for_edit(&edit("a\nb\nc\nd\n", 3..3, "\n\n"));
        let HighlightRange::Lines(r) = &s.highlights["conflict"][0].range else {
            panic!("lines");
        };
        assert_eq!(r.clone(), 1..5);
    }

    #[test]
    fn line_ranges_drop_only_when_fully_deleted() {
        let text = "a\nb\nc\nd\ne\nf\n";
        // Delete lines 1..5: the whole range 2..4 is gone.
        let e = edit(text, 2..10, "");
        assert_eq!(map_lines(&(2..4), &e), None);
        // A range sticking out on either side keeps its surviving lines.
        assert_eq!(map_lines(&(0..3), &e), Some(0..1));
        assert_eq!(map_lines(&(3..6), &e), Some(1..2));
        assert_eq!(map_lines(&(0..6), &e), Some(0..2));
        // Delete the middle of line 1 to the middle of line 3: lines 1..4
        // join into line 1.
        let e = edit(text, 3..7, "");
        assert_eq!(map_lines(&(2..3), &e), None);
        assert_eq!(map_lines(&(1..4), &e), Some(1..2));
        assert_eq!(map_lines(&(3..5), &e), Some(1..3));
    }

    #[test]
    fn text_ranges_keep_their_ends_on_insertions() {
        let text = "foo bar\n";
        let r = Point::new(0, 0)..Point::new(0, 3);
        // Typing at the end or start does not grow the range.
        assert_eq!(map_range(&r, &edit(text, 3..3, "x")), Some(r.clone()));
        let shifted = Point::new(0, 1)..Point::new(0, 4);
        assert_eq!(map_range(&r, &edit(text, 0..0, "x")), Some(shifted));
        // Typing inside grows it; deleting right after it keeps it.
        let grown = Point::new(0, 0)..Point::new(0, 4);
        assert_eq!(map_range(&r, &edit(text, 1..1, "x")), Some(grown));
        assert_eq!(map_range(&r, &edit(text, 3..7, "")), Some(r.clone()));
        // Replacing text across an end drops it.
        assert_eq!(map_range(&r, &edit(text, 2..5, "")), None);
        assert_eq!(map_range(&r, &edit(text, 0..3, "")), None);
    }
}
