//! Diagnostics shown inline in the editor and how they follow edits.

use std::ops::Range;

use arcode_buffer::{Edit, Point};
use arcode_ui_kit::{IconName, ThemeColors};
use gpui::Hsla;

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticSeverity {
    /// A compile error or similar.
    Error,
    /// A warning.
    Warning,
    /// Informational message.
    Info,
    /// A hint (e.g. unused code).
    Hint,
}

impl DiagnosticSeverity {
    /// The theme color for this severity.
    pub fn color(self, colors: &ThemeColors) -> Hsla {
        match self {
            DiagnosticSeverity::Error => colors.error,
            DiagnosticSeverity::Warning => colors.warning,
            DiagnosticSeverity::Info => colors.info,
            DiagnosticSeverity::Hint => colors.hint,
        }
    }

    /// The icon used in popovers.
    pub fn icon(self) -> IconName {
        match self {
            DiagnosticSeverity::Error => IconName::CircleX,
            DiagnosticSeverity::Warning => IconName::TriangleAlert,
            DiagnosticSeverity::Info => IconName::Info,
            DiagnosticSeverity::Hint => IconName::CircleAlert,
        }
    }
}

/// A diagnostic attached to a buffer range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorDiagnostic {
    /// The affected range (line, char column).
    pub range: Range<Point>,
    /// Severity, which selects the underline color.
    pub severity: DiagnosticSeverity,
    /// The message shown on hover.
    pub message: String,
    /// Who produced it, e.g. `rust-analyzer`.
    pub source: String,
}

/// Maps `p` (at or after the edit's old end) into post-edit coordinates.
fn shift_point(p: Point, edit: &Edit) -> Point {
    let old_end = edit.old_end_point;
    let new_end = edit.new_end_point;
    if p.line == old_end.line {
        Point::new(new_end.line, new_end.column + (p.column - old_end.column))
    } else {
        Point::new(p.line - old_end.line + new_end.line, p.column)
    }
}

/// Adjusts `diagnostics` for one applied edit.
///
/// Ranges entirely after the edit shift with it; ranges entirely before stay
/// put. A pure insertion strictly inside a range grows it; any other
/// overlap drops the diagnostic until the server sends fresh ones.
pub fn adjust_for_edit(diagnostics: &mut Vec<EditorDiagnostic>, edit: &Edit) {
    let start = edit.start_point;
    let old_end = edit.old_end_point;
    let insertion = start == old_end;
    diagnostics.retain_mut(|d| {
        if d.range.end < start || (d.range.end == start && d.range.start < start) {
            return true;
        }
        if d.range.start >= old_end && !(insertion && d.range.start == start && d.range.is_empty())
        {
            d.range = shift_point(d.range.start, edit)..shift_point(d.range.end, edit);
            return true;
        }
        if insertion && d.range.start < start && start < d.range.end {
            d.range.end = shift_point(d.range.end, edit);
            return true;
        }
        if insertion && d.range.is_empty() && d.range.start == start {
            // Typing at a zero-width diagnostic keeps it before the text.
            return true;
        }
        false
    });
}

/// The most severe diagnostic per line in `lines`, for gutter markers.
pub fn line_severities(
    diagnostics: &[EditorDiagnostic],
    lines: Range<usize>,
) -> Vec<Option<DiagnosticSeverity>> {
    let mut out = vec![None; lines.len()];
    for d in diagnostics {
        let line = d.range.start.line;
        if lines.contains(&line) {
            let slot = &mut out[line - lines.start];
            *slot = Some(slot.map_or(d.severity, |s: DiagnosticSeverity| s.min(d.severity)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_buffer::Buffer;

    fn diag(a: (usize, usize), b: (usize, usize)) -> EditorDiagnostic {
        EditorDiagnostic {
            range: Point::new(a.0, a.1)..Point::new(b.0, b.1),
            severity: DiagnosticSeverity::Error,
            message: "m".into(),
            source: "s".into(),
        }
    }

    fn edit(text: &str, range: Range<usize>, new: &str) -> Edit {
        let mut b = Buffer::new(text);
        b.edit(vec![(range, new.to_string())]).remove(0)
    }

    #[test]
    fn later_lines_shift() {
        let mut ds = vec![diag((2, 1), (2, 3))];
        adjust_for_edit(&mut ds, &edit("a\nb\ncdef", 0..0, "x\n"));
        assert_eq!(ds[0].range, Point::new(3, 1)..Point::new(3, 3));
    }

    #[test]
    fn same_line_after_shifts_columns() {
        let mut ds = vec![diag((0, 4), (0, 6))];
        adjust_for_edit(&mut ds, &edit("abcdefgh", 1..1, "XY"));
        assert_eq!(ds[0].range, Point::new(0, 6)..Point::new(0, 8));
    }

    #[test]
    fn before_stays_and_touched_drops() {
        let mut ds = vec![diag((0, 0), (0, 2)), diag((0, 3), (0, 6))];
        adjust_for_edit(&mut ds, &edit("abcdefgh", 4..5, ""));
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].range, Point::new(0, 0)..Point::new(0, 2));
    }

    #[test]
    fn insertion_inside_grows() {
        let mut ds = vec![diag((0, 1), (0, 5))];
        adjust_for_edit(&mut ds, &edit("abcdefgh", 3..3, "zz"));
        assert_eq!(ds[0].range, Point::new(0, 1)..Point::new(0, 7));
    }

    #[test]
    fn severities_per_line() {
        let mut w = diag((1, 0), (1, 1));
        w.severity = DiagnosticSeverity::Warning;
        let ds = vec![w, diag((1, 2), (1, 3)), diag((5, 0), (5, 0))];
        let s = line_severities(&ds, 0..3);
        assert_eq!(s, vec![None, Some(DiagnosticSeverity::Error), None]);
    }
}
