//! Tracking an inserted snippet's tabstops through buffer edits.

use std::ops::Range;

use arcode_buffer::{Edit, Selection, SelectionSet};

use super::expand::ExpandedSnippet;

#[derive(Debug, Clone)]
struct Tracked {
    range: Range<usize>,
    /// Current text of the range, `None` once an edit straddled a boundary.
    text: Option<String>,
    /// Edited inside since the last mirror sync.
    changed: bool,
    /// Stops (positions in `stops`) with a range nested in this one; this
    /// range grows with them while one of them is active.
    encloses: Vec<usize>,
}

#[derive(Debug, Clone)]
struct Stop {
    index: u32,
    ranges: Vec<Tracked>,
    choices: Option<Vec<String>>,
}

/// An active snippet: tabstop navigation plus range tracking.
///
/// Feed every buffer change to [`SnippetSession::apply_edits`]; apply the
/// mirror updates it returns with `Buffer::edit` and feed those edits back
/// too. Edits at the boundary of the active tabstop grow it, so typing into
/// an (empty) placeholder extends it.
#[derive(Debug, Clone)]
pub struct SnippetSession {
    stops: Vec<Stop>,
    extent: Tracked,
    active: usize,
    finished: bool,
    /// Mirror updates handed out and not yet seen back.
    pending: Vec<(Range<usize>, String)>,
}

fn char_slice(s: &str, range: Range<usize>) -> (usize, usize) {
    let mut it = s
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(s.len()));
    let start = it.nth(range.start).unwrap_or(s.len());
    let end = if range.end > range.start {
        it.nth(range.end - range.start - 1).unwrap_or(s.len())
    } else {
        start
    };
    (start, end)
}

impl SnippetSession {
    /// Starts a session for `snippet` whose text was inserted at char
    /// `offset`. The first tabstop is active.
    pub fn new(snippet: &ExpandedSnippet, offset: usize) -> Self {
        let mut stops: Vec<Stop> = snippet
            .tabstops
            .iter()
            .map(|t| Stop {
                index: t.index,
                ranges: t
                    .ranges
                    .iter()
                    .map(|r| Tracked {
                        range: r.start + offset..r.end + offset,
                        text: Some(snippet.text.chars().skip(r.start).take(r.len()).collect()),
                        changed: false,
                        encloses: Vec::new(),
                    })
                    .collect(),
                choices: t.choices.clone(),
            })
            .collect();
        let len = snippet.text.chars().count();
        if stops.is_empty() {
            stops.push(Stop {
                index: 0,
                ranges: vec![Tracked {
                    range: offset + len..offset + len,
                    text: Some(String::new()),
                    changed: false,
                    encloses: Vec::new(),
                }],
                choices: None,
            });
        }
        link_nesting(&mut stops);
        let finished = stops.len() == 1;
        SnippetSession {
            stops,
            extent: Tracked {
                range: offset..offset + len,
                text: None,
                changed: false,
                encloses: Vec::new(),
            },
            active: 0,
            finished,
            pending: Vec::new(),
        }
    }

    /// Whether the final tabstop was reached (or the session was ended).
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Ends the session (e.g. on Escape or when the cursor leaves it).
    pub fn finish(&mut self) {
        self.finished = true;
    }

    /// Number of the active tabstop.
    pub fn active_index(&self) -> u32 {
        self.stops.get(self.active).map_or(0, |s| s.index)
    }

    /// Choices of the active tabstop, if it is a choice.
    pub fn current_choices(&self) -> Option<&[String]> {
        self.stops.get(self.active)?.choices.as_deref()
    }

    /// Current char ranges of the active tabstop (mirrors included).
    pub fn active_ranges(&self) -> Vec<Range<usize>> {
        self.stops.get(self.active).map_or_else(Vec::new, |s| {
            s.ranges.iter().map(|t| t.range.clone()).collect()
        })
    }

    /// All tabstop ranges as `(index, range)`, e.g. to draw placeholders.
    pub fn all_ranges(&self) -> Vec<(u32, Range<usize>)> {
        self.stops
            .iter()
            .flat_map(|s| s.ranges.iter().map(move |t| (s.index, t.range.clone())))
            .collect()
    }

    /// Char range spanned by the whole inserted snippet.
    pub fn extent(&self) -> Range<usize> {
        self.extent.range.clone()
    }

    /// Whether `pos` lies within the snippet (boundaries included).
    pub fn contains(&self, pos: usize) -> bool {
        self.extent.range.start <= pos && pos <= self.extent.range.end
    }

    /// Selections for the active tabstop: every mirror selected, the first
    /// one primary.
    pub fn selections(&self) -> SelectionSet {
        let sels: Vec<Selection> = self
            .active_ranges()
            .into_iter()
            .map(|r| Selection::new(r.start, r.end))
            .collect();
        SelectionSet::from_vec(sels, 0)
            .unwrap_or_else(|| SelectionSet::single(self.extent.range.end))
    }

    /// Moves to the next tabstop and returns its selections; reaching `$0`
    /// finishes the session. `None` when already finished.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<SelectionSet> {
        if self.finished {
            return None;
        }
        self.active = (self.active + 1).min(self.stops.len() - 1);
        if self.active + 1 == self.stops.len() {
            self.finished = true;
        }
        Some(self.selections())
    }

    /// Moves to the previous tabstop; `None` at the first one or when
    /// finished.
    pub fn prev(&mut self) -> Option<SelectionSet> {
        if self.finished || self.active == 0 {
            return None;
        }
        self.active -= 1;
        Some(self.selections())
    }

    /// Maps all ranges through `edits` (in the order the buffer returned
    /// them) and returns mirror updates `(char range, text)` to apply with
    /// `Buffer::edit`, so mirrors follow the tabstop that was typed in.
    pub fn apply_edits(&mut self, edits: &[Edit]) -> Vec<(Range<usize>, String)> {
        for edit in edits {
            self.apply_edit(edit);
        }
        self.sync_mirrors()
    }

    fn apply_edit(&mut self, edit: &Edit) {
        let new_len = edit.new_text.chars().count();
        let mirror = self
            .pending
            .iter()
            .position(|(r, t)| *r == edit.range && *t == edit.new_text);
        let mirror = mirror.map(|i| self.pending.remove(i).0);
        let active = self.active;
        for (si, stop) in self.stops.iter_mut().enumerate() {
            let is_sticky = |t: &Tracked| si == active || t.encloses.contains(&active);
            // A mirror update belongs to its target range alone.
            let exact = mirror
                .as_ref()
                .and_then(|m| stop.ranges.iter().position(|t| t.range == *m));
            let claim = exact.or_else(|| boundary_claim(&stop.ranges, &edit.range, is_sticky));
            for (ri, t) in stop.ranges.iter_mut().enumerate() {
                let sticky = is_sticky(t) && claim.is_none_or(|c| c == ri);
                let exact = mirror.as_ref() == Some(&t.range);
                map_range(t, edit, new_len, sticky || exact);
            }
        }
        map_range(&mut self.extent, edit, new_len, true);
    }

    fn sync_mirrors(&mut self) -> Vec<(Range<usize>, String)> {
        let mut updates = Vec::new();
        for stop in &mut self.stops {
            let source = stop
                .ranges
                .iter()
                .rev()
                .find(|t| t.changed)
                .and_then(|t| t.text.clone());
            if let Some(source) = source {
                for t in &stop.ranges {
                    if t.text.as_ref().is_some_and(|x| *x != source) {
                        updates.push((t.range.clone(), source.clone()));
                    }
                }
            }
            for t in &mut stop.ranges {
                t.changed = false;
            }
        }
        self.pending.clone_from(&updates);
        updates
    }
}

/// For an insertion touching several sticky ranges of one stop (adjacent
/// mirrors like `${1:a}$1`), the single range that grows: the first one
/// ending at the insertion point (where the cursor is after typing at its
/// end), else the first one touched. `None` when there is no conflict.
fn boundary_claim(
    ranges: &[Tracked],
    edit: &Range<usize>,
    is_sticky: impl Fn(&Tracked) -> bool,
) -> Option<usize> {
    if !edit.is_empty() {
        return None;
    }
    let at = edit.start;
    let touched: Vec<usize> = ranges
        .iter()
        .enumerate()
        .filter(|(_, t)| is_sticky(t) && t.range.start <= at && at <= t.range.end)
        .map(|(i, _)| i)
        .collect();
    if touched.len() < 2 {
        return None;
    }
    touched
        .iter()
        .copied()
        .find(|&i| ranges[i].range.end == at)
        .or(touched.first().copied())
}

/// Whether `inner` is nested in `outer`. An empty range at a boundary of
/// another is adjacent, not nested (`$1${2:x}`).
fn nests(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start
        && inner.end <= outer.end
        && !outer.is_empty()
        && !(inner.is_empty() && (inner.start == outer.start || inner.start == outer.end))
}

/// Fills [`Tracked::encloses`] from the initial ranges.
fn link_nesting(stops: &mut [Stop]) {
    let all: Vec<(usize, Range<usize>)> = stops
        .iter()
        .enumerate()
        .flat_map(|(si, s)| s.ranges.iter().map(move |t| (si, t.range.clone())))
        .collect();
    for (si, stop) in stops.iter_mut().enumerate() {
        for t in &mut stop.ranges {
            for (sj, inner) in &all {
                if *sj != si && nests(&t.range, inner) && !t.encloses.contains(sj) {
                    t.encloses.push(*sj);
                }
            }
        }
    }
}

/// Maps one tracked range through `edit`. `sticky` ranges grow on
/// insertions at either boundary.
fn map_range(t: &mut Tracked, edit: &Edit, new_len: usize, sticky: bool) {
    let (s, e) = (edit.range.start, edit.range.end);
    let (a, b) = (t.range.start, t.range.end);
    let insertion = s == e;
    // Non-sticky ranges do not swallow insertions at their boundaries.
    let inside = a <= s && e <= b && (sticky || !(insertion && (s == a || s == b)));
    if inside {
        t.range = a..b - (e - s) + new_len;
        if let Some(text) = &mut t.text {
            let (bs, be) = char_slice(text, s - a..e - a);
            text.replace_range(bs..be, &edit.new_text);
        }
        t.changed = true;
        return;
    }
    let start = if a >= e {
        a - (e - s) + new_len
    } else if a <= s {
        a
    } else {
        s + new_len
    };
    let end = if b <= s {
        b
    } else if b >= e {
        b - (e - s) + new_len
    } else {
        s
    };
    if e > a && s < b {
        // The edit straddles a boundary: the content is no longer known.
        t.text = None;
    }
    t.range = start..end.max(start);
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
