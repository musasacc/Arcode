//! Diagnostics shown inline in the editor and how they follow edits.

use std::ops::Range;

use arcode_buffer::{Edit, Point};
use arcode_config::InlineDiagnostics;
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
    /// The diagnostic's code, e.g. `E0308` (shown in the inline lens).
    pub code: Option<String>,
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

/// The diagnostic whose inline lens is drawn after each line in `lines`
/// (`editor.inline_diagnostics`): the most severe one starting on the line
/// that `mode` shows. Among equals it is the one with the shortest first
/// message line (it fits the lens best), then the leftmost, then by
/// source, so the choice never depends on which server published first.
pub(crate) fn lens_diagnostics(
    diagnostics: &[EditorDiagnostic],
    lines: Range<usize>,
    mode: InlineDiagnostics,
) -> Vec<Option<&EditorDiagnostic>> {
    let mut out: Vec<Option<&EditorDiagnostic>> = vec![None; lines.len()];
    if mode == InlineDiagnostics::Off {
        return out;
    }
    for d in diagnostics {
        let line = d.range.start.line;
        if !lines.contains(&line) || !mode.shows(d.severity == DiagnosticSeverity::Error) {
            continue;
        }
        let slot = &mut out[line - lines.start];
        if slot.is_none_or(|s| d.lens_rank() < s.lens_rank()) {
            *slot = Some(d);
        }
    }
    out
}

impl EditorDiagnostic {
    /// Sort key for [`lens_diagnostics`]: lower wins the lens.
    fn lens_rank(&self) -> (DiagnosticSeverity, usize, usize, &str, &str) {
        let first = self.message.lines().next().unwrap_or("").trim_end();
        let code = self.code.as_deref().unwrap_or("");
        (
            self.severity,
            first.chars().count(),
            self.range.start.column,
            &self.source,
            code,
        )
    }

    /// The lens text: the first line of the message, and the muted
    /// "source code" part (`rustc E0308`), empty when there is neither.
    pub(crate) fn lens_text(&self) -> (&str, String) {
        let message = self.message.lines().next().unwrap_or("").trim_end();
        let meta = [Some(self.source.as_str()), self.code.as_deref()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        (message, meta)
    }
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
            code: None,
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

    #[test]
    fn lens_follows_the_setting_and_prefers_errors() {
        let mut w = diag((1, 0), (1, 1));
        w.severity = DiagnosticSeverity::Warning;
        w.message = "unused".into();
        let mut e = diag((1, 2), (1, 3));
        e.message = "mismatched types\nexpected i32".into();
        e.source = "rustc".into();
        e.code = Some("E0308".into());
        let ds = vec![w, e, diag((7, 0), (7, 0))];
        let errors = lens_diagnostics(&ds, 0..3, InlineDiagnostics::Errors);
        assert_eq!(errors.len(), 3);
        let lens = errors[1].expect("lens on line 1");
        assert_eq!(lens.lens_text(), ("mismatched types", "rustc E0308".into()));
        assert!(errors[0].is_none() && errors[2].is_none());
        let off = lens_diagnostics(&ds, 0..3, InlineDiagnostics::Off);
        assert!(off.iter().all(Option::is_none));
        // Warnings get a lens only with "all".
        let only_warning = [ds[0].clone()];
        assert!(lens_diagnostics(&only_warning, 0..3, InlineDiagnostics::Errors)[1].is_none());
        let all = lens_diagnostics(&only_warning, 0..3, InlineDiagnostics::All);
        assert_eq!(all[1].map(|d| d.lens_text()), Some(("unused", "s".into())));
    }

    #[test]
    fn lens_choice_does_not_depend_on_arrival_order() {
        // rust-analyzer reports E0308 twice: its own wording and rustc's.
        let mut ra = diag((1, 13), (1, 19));
        ra.message = "expected i32, found &'static str".into();
        ra.source = "rust-analyzer".into();
        let mut rustc = ra.clone();
        rustc.message = "mismatched types\nexpected `i32`, found `&str`".into();
        rustc.source = "rustc".into();
        for ds in [
            vec![ra.clone(), rustc.clone()],
            vec![rustc.clone(), ra.clone()],
        ] {
            let lens = lens_diagnostics(&ds, 0..3, InlineDiagnostics::Errors);
            assert_eq!(lens[1].map(|d| d.source.as_str()), Some("rustc"));
        }
        // Severity still comes first: a long error beats a short warning.
        let mut w = rustc.clone();
        w.severity = DiagnosticSeverity::Warning;
        w.message = "x".into();
        let ds = [w, ra.clone()];
        let lens = lens_diagnostics(&ds, 0..3, InlineDiagnostics::All);
        assert_eq!(lens[1].map(|d| d.source.as_str()), Some("rust-analyzer"));
    }
}
