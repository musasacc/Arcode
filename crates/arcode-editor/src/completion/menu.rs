//! Ranked rows, the selection and the scroll window of the list.

use arcode_completion::{CompletionCandidate, RankedCandidate, Ranker};

/// Rows visible at once.
pub(crate) const VISIBLE_ROWS: usize = 10;
/// At most this many ranked rows are kept.
pub(crate) const MAX_ROWS: usize = 200;

/// The ranked list with its selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MenuState {
    pub ranked: Vec<RankedCandidate>,
    pub selected: usize,
    /// First visible row.
    pub scroll_top: usize,
    /// The user moved the selection (arrows, page keys).
    pub user_moved: bool,
}

impl MenuState {
    /// Ranks `candidates` for `query`; the best (or preselected) row is
    /// selected.
    pub fn rank(ranker: &mut Ranker, query: &str, candidates: &[CompletionCandidate]) -> Self {
        let ranked = ranker.rank(query, candidates, MAX_ROWS);
        let selected = if query.is_empty() {
            0
        } else {
            // A preselected item among the equally good prefix matches.
            let best = ranked.first().map(|r| r.score);
            ranked
                .iter()
                .take_while(|r| Some(r.score) == best)
                .position(|r| candidates[r.index].preselect)
                .unwrap_or(0)
        };
        let mut menu = MenuState {
            ranked,
            selected,
            scroll_top: 0,
            user_moved: false,
        };
        menu.reveal();
        menu
    }

    /// Moves the selection by `delta` rows, wrapping around at the ends.
    pub fn move_by(&mut self, delta: isize) {
        let len = self.ranked.len() as isize;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        self.user_moved = true;
        self.reveal();
    }

    /// Moves a page down (`forward`) or up, stopping at the ends.
    pub fn page(&mut self, forward: bool) {
        let len = self.ranked.len();
        if len == 0 {
            return;
        }
        let step = VISIBLE_ROWS.saturating_sub(1).max(1);
        self.selected = if forward {
            (self.selected + step).min(len - 1)
        } else {
            self.selected.saturating_sub(step)
        };
        self.user_moved = true;
        self.reveal();
    }

    /// Selects the first row for which `f` holds, as if the user had
    /// moved there; returns whether there was one.
    pub fn select_where(&mut self, f: impl Fn(&RankedCandidate) -> bool) -> bool {
        let Some(row) = self.ranked.iter().position(f) else {
            return false;
        };
        self.selected = row;
        self.user_moved = true;
        self.reveal();
        true
    }

    /// Scrolls so the selected row is visible.
    pub fn reveal(&mut self) {
        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        } else if self.selected >= self.scroll_top + VISIBLE_ROWS {
            self.scroll_top = self.selected + 1 - VISIBLE_ROWS;
        }
        self.scroll_top = clamp_top(self.scroll_top, self.ranked.len());
    }
}

/// The largest useful first row for `len` rows.
pub(crate) fn clamp_top(top: usize, len: usize) -> usize {
    top.min(len.saturating_sub(VISIBLE_ROWS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_completion::{CandidateKind, CandidateSource};

    fn cands(labels: &[&str]) -> Vec<CompletionCandidate> {
        labels
            .iter()
            .map(|l| CompletionCandidate::new(*l, CandidateKind::Word, CandidateSource::BufferWord))
            .collect()
    }

    #[test]
    fn navigation_wraps_and_pages_stop() {
        let labels: Vec<String> = (0..25).map(|i| format!("item{i:02}")).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let c = cands(&refs);
        let mut m = MenuState::rank(&mut Ranker::new(), "", &c);
        assert_eq!((m.selected, m.scroll_top), (0, 0));
        m.move_by(-1);
        assert_eq!(m.selected, 24);
        assert_eq!(m.scroll_top, 15);
        m.move_by(1);
        assert_eq!((m.selected, m.scroll_top), (0, 0));
        m.page(true);
        assert_eq!((m.selected, m.scroll_top), (9, 0));
        m.page(true);
        assert_eq!((m.selected, m.scroll_top), (18, 9));
        m.page(true);
        assert_eq!(m.selected, 24);
        m.page(false);
        m.page(false);
        m.page(false);
        assert_eq!((m.selected, m.scroll_top), (0, 0));
    }

    #[test]
    fn preselected_item_wins_among_equal_matches() {
        let mut c = cands(&["format", "forward"]);
        c[1].preselect = true;
        let m = MenuState::rank(&mut Ranker::new(), "fo", &c);
        assert_eq!(c[m.ranked[m.selected].index].label, "forward");
        let none = MenuState::rank(&mut Ranker::new(), "zz", &c);
        assert!(none.ranked.is_empty());
        let mut empty = none.clone();
        empty.move_by(1);
        empty.page(true);
        assert_eq!(empty.selected, 0);
    }

    #[test]
    fn select_where_marks_the_choice_as_the_users() {
        let labels: Vec<String> = (0..15).map(|i| format!("item{i:02}")).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let c = cands(&refs);
        let mut m = MenuState::rank(&mut Ranker::new(), "", &c);
        assert!(!m.user_moved);
        assert!(m.select_where(|r| c[r.index].label == "item12"));
        assert_eq!((m.selected, m.scroll_top), (12, 3));
        assert!(m.user_moved);
        assert!(!m.select_where(|r| c[r.index].label == "nope"));
        assert_eq!(m.selected, 12);
    }
}
