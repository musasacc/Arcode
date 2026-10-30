//! Back / forward navigation history across files.
//!
//! Every jump (go to definition, a picked symbol or reference, switching
//! to another file) records where the cursor was. Going back returns there
//! and remembers the current place for going forward.

use std::path::PathBuf;

use arcode_buffer::Point;

/// Most entries kept in each direction.
const MAX_ENTRIES: usize = 100;
/// Places on the same file closer than this many lines count as one.
const SAME_PLACE_LINES: usize = 5;

/// A cursor position in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavLocation {
    /// The file.
    pub path: PathBuf,
    /// The cursor.
    pub point: Point,
}

impl NavLocation {
    /// Whether `other` is about the same place (same file, a few lines
    /// apart), so recording both would make Back a no-op.
    fn near(&self, other: &NavLocation) -> bool {
        self.path == other.path && self.point.line.abs_diff(other.point.line) < SAME_PLACE_LINES
    }
}

/// The back and forward stacks.
#[derive(Debug, Default)]
pub struct History {
    back: Vec<NavLocation>,
    forward: Vec<NavLocation>,
}

impl History {
    /// Records `from` before a jump: it becomes the Back target and the
    /// forward stack is cleared. A place next to the last one replaces it.
    pub fn record(&mut self, from: NavLocation) {
        self.forward.clear();
        if self.back.last().is_some_and(|last| last.near(&from)) {
            self.back.pop();
        }
        push_capped(&mut self.back, from);
    }

    /// The place to go back to from `current` (which becomes the Forward
    /// target). Entries next to `current` are skipped.
    pub fn back(&mut self, current: Option<NavLocation>) -> Option<NavLocation> {
        step(&mut self.back, &mut self.forward, current)
    }

    /// The place to go forward to from `current`.
    pub fn forward(&mut self, current: Option<NavLocation>) -> Option<NavLocation> {
        step(&mut self.forward, &mut self.back, current)
    }

    /// Whether Back has somewhere to go.
    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    /// Whether Forward has somewhere to go.
    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Forgets everything (e.g. another project was opened).
    pub fn clear(&mut self) {
        self.back.clear();
        self.forward.clear();
    }
}

fn step(
    from: &mut Vec<NavLocation>,
    to: &mut Vec<NavLocation>,
    current: Option<NavLocation>,
) -> Option<NavLocation> {
    let mut target = from.pop()?;
    if let Some(cur) = &current {
        while target.near(cur) {
            target = from.pop()?;
        }
    }
    if let Some(cur) = current {
        push_capped(to, cur);
    }
    Some(target)
}

fn push_capped(stack: &mut Vec<NavLocation>, location: NavLocation) {
    stack.push(location);
    if stack.len() > MAX_ENTRIES {
        stack.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(file: &str, line: usize) -> NavLocation {
        NavLocation {
            path: PathBuf::from(file),
            point: Point::new(line, 0),
        }
    }

    #[test]
    fn back_and_forward_walk_the_jumps() {
        let mut h = History::default();
        h.record(loc("a.rs", 10));
        h.record(loc("b.rs", 40));
        // Now at c.rs:3.
        assert_eq!(h.back(Some(loc("c.rs", 3))), Some(loc("b.rs", 40)));
        assert_eq!(h.back(Some(loc("b.rs", 40))), Some(loc("a.rs", 10)));
        assert_eq!(h.back(Some(loc("a.rs", 10))), None);
        assert_eq!(h.forward(Some(loc("a.rs", 10))), Some(loc("b.rs", 40)));
        assert_eq!(h.forward(Some(loc("b.rs", 40))), Some(loc("c.rs", 3)));
        assert!(!h.can_go_forward());
        assert!(h.can_go_back());
    }

    #[test]
    fn a_new_jump_clears_forward_and_nearby_places_merge() {
        let mut h = History::default();
        h.record(loc("a.rs", 10));
        h.record(loc("a.rs", 12));
        assert_eq!(h.back(Some(loc("z.rs", 0))), Some(loc("a.rs", 12)));
        assert!(!h.can_go_back(), "a.rs:10 merged into a.rs:12");
        h.record(loc("a.rs", 12));
        assert!(!h.can_go_forward());
    }

    #[test]
    fn back_skips_entries_at_the_current_place() {
        let mut h = History::default();
        h.record(loc("a.rs", 1));
        h.record(loc("b.rs", 7));
        assert_eq!(h.back(Some(loc("b.rs", 8))), Some(loc("a.rs", 1)));
    }

    #[test]
    fn stacks_are_capped() {
        let mut h = History::default();
        for i in 0..150 {
            h.record(loc("a.rs", i * 10));
        }
        let mut n = 0;
        while h.back(None).is_some() {
            n += 1;
        }
        assert_eq!(n, MAX_ENTRIES);
    }
}
