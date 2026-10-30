//! Merge conflict markers: parsing and resolving.

use std::ops::Range;

use crate::diff::{line_span, line_starts};
use crate::split_lines;

/// One conflict region; all positions are zero-based editor lines (see
/// [`crate::split_lines`]). Content ranges exclude the marker lines.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Conflict {
    /// The `<<<<<<<` line.
    pub start_line: usize,
    /// Lines of our side.
    pub ours: Range<usize>,
    /// Lines of the common ancestor (diff3 / zdiff3 `|||||||` section).
    pub base: Option<Range<usize>>,
    /// Lines of their side.
    pub theirs: Range<usize>,
    /// The `>>>>>>>` line.
    pub end_line: usize,
    /// Text after `<<<<<<<` (e.g. `HEAD`), trimmed.
    pub ours_label: String,
    /// Text after `|||||||`, trimmed (diff3 only).
    pub base_label: Option<String>,
    /// Text after `>>>>>>>` (e.g. the merged branch), trimmed.
    pub theirs_label: String,
}

impl Conflict {
    /// All lines of the conflict including markers.
    pub fn lines(&self) -> Range<usize> {
        self.start_line..self.end_line + 1
    }
}

/// How to resolve a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resolution {
    /// Keep our side.
    Ours,
    /// Keep their side.
    Theirs,
    /// Keep ours followed by theirs.
    Both,
    /// Keep the common ancestor (empty when the conflict has no base).
    Base,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Marker {
    Start,
    Base,
    Separator,
    End,
}

/// Recognizes a marker line: exactly seven marker characters at the start
/// of the line, then end of line or a space and a label (`=======` takes no
/// label, only trailing whitespace).
fn marker(line: &str) -> Option<(Marker, &str)> {
    let kind = match line.as_bytes().first()? {
        b'<' => Marker::Start,
        b'|' => Marker::Base,
        b'=' => Marker::Separator,
        b'>' => Marker::End,
        _ => return None,
    };
    let lead = line.as_bytes()[0];
    if line.len() < 7 || !line.as_bytes()[..7].iter().all(|&b| b == lead) {
        return None;
    }
    let rest = &line[7..];
    if kind == Marker::Separator {
        return rest.trim().is_empty().then_some((kind, ""));
    }
    if rest.is_empty() {
        return Some((kind, ""));
    }
    let label = rest.strip_prefix([' ', '\t'])?;
    Some((kind, label.trim()))
}

/// Partial conflict while scanning.
struct Open {
    start: usize,
    ours_label: String,
    base: Option<(usize, String)>,
    separator: Option<usize>,
}

/// Finds all well-formed conflicts in `text`, in order.
///
/// A `<<<<<<<` inside an unfinished conflict restarts it there, so marker
/// lines that were committed as content earlier do not swallow the real
/// conflict; a `>>>>>>>` without `=======`, or a conflict still open at the
/// end, is ignored. Within their section, `=======` repeats and `|||||||`
/// lines are content.
pub fn parse_conflicts(text: &str) -> Vec<Conflict> {
    let mut out = Vec::new();
    let mut open: Option<Open> = None;
    for (i, line) in split_lines(text).enumerate() {
        let Some((kind, label)) = marker(line) else {
            continue;
        };
        match (kind, open.as_mut()) {
            (Marker::Start, _) => {
                open = Some(Open {
                    start: i,
                    ours_label: label.to_owned(),
                    base: None,
                    separator: None,
                });
            }
            (Marker::Base, Some(o)) if o.base.is_none() && o.separator.is_none() => {
                o.base = Some((i, label.to_owned()));
            }
            (Marker::Separator, Some(o)) if o.separator.is_none() => o.separator = Some(i),
            (Marker::End, Some(o)) => {
                if let Some(sep) = o.separator {
                    out.push(finish(o, sep, i, label));
                }
                open = None;
            }
            _ => {}
        }
    }
    out
}

fn finish(open: &mut Open, sep: usize, end: usize, label: &str) -> Conflict {
    let ours_end = open.base.as_ref().map_or(sep, |(line, _)| *line);
    let (base, base_label) = match open.base.take() {
        Some((line, label)) => (Some(line + 1..sep), Some(label)),
        None => (None, None),
    };
    Conflict {
        start_line: open.start,
        ours: open.start + 1..ours_end,
        base,
        theirs: sep + 1..end,
        end_line: end,
        ours_label: std::mem::take(&mut open.ours_label),
        base_label,
        theirs_label: label.to_owned(),
    }
}

/// Returns `text` with `conflict` (from [`parse_conflicts`] on this text)
/// replaced by the chosen side(s). Other conflicts are left untouched; to
/// resolve several, resolve the last one first or re-parse in between.
pub fn resolve(text: &str, conflict: &Conflict, resolution: Resolution) -> String {
    let starts = line_starts(text);
    let section = |lines: &Range<usize>| {
        let span = line_span(&starts, lines);
        text.get(span).unwrap_or_default()
    };
    let chosen: Vec<&str> = match resolution {
        Resolution::Ours => vec![section(&conflict.ours)],
        Resolution::Theirs => vec![section(&conflict.theirs)],
        Resolution::Both => vec![section(&conflict.ours), section(&conflict.theirs)],
        Resolution::Base => conflict.base.iter().map(section).collect(),
    };
    let whole = line_span(&starts, &conflict.lines());
    let mut out = String::with_capacity(text.len());
    out.push_str(text.get(..whole.start).unwrap_or_default());
    for part in chosen {
        out.push_str(part);
    }
    out.push_str(text.get(whole.end..).unwrap_or_default());
    out
}

#[cfg(test)]
#[path = "conflict_tests.rs"]
mod tests;
