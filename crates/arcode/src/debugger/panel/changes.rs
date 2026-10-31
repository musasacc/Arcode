//! Which variable values changed since the previous stop: the "changed"
//! ring of docs/DESIGN.md 6.8. Values are remembered by frame and tree
//! path (references change at every stop), so a value only counts as
//! changed when the same variable of the same function showed another
//! value at an earlier stop.

use std::collections::{HashMap, HashSet};

/// The values seen so far and the ones that changed at the current stop.
#[derive(Debug, Default)]
pub struct ValueChanges {
    /// The stop being shown (`Session::generation`).
    generation: u64,
    /// Last value of each key and the stop it was seen at.
    known: HashMap<String, (String, u64)>,
    /// Keys whose value differs from an earlier stop.
    changed: HashSet<String>,
}

/// The key of a variable: its frame's function and its tree path.
pub fn change_key(frame: &str, path: &str) -> String {
    format!("{frame}\u{0}{path}")
}

impl ValueChanges {
    /// Records `value` for `key` at stop `generation` and returns whether
    /// it changed since an earlier stop. A lower generation (a new session)
    /// forgets everything.
    pub fn observe(&mut self, generation: u64, key: &str, value: &str) -> bool {
        if generation < self.generation {
            *self = Self::default();
        }
        if generation != self.generation {
            self.generation = generation;
            self.changed.clear();
        }
        match self.known.get_mut(key) {
            Some((old, seen)) => {
                if *seen < generation && old != value {
                    self.changed.insert(key.to_string());
                }
                *old = value.to_string();
                *seen = generation;
            }
            None => {
                self.known
                    .insert(key.to_string(), (value.to_string(), generation));
            }
        }
        self.changed.contains(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_changed_when_it_differs_from_an_earlier_stop() {
        let mut v = ValueChanges::default();
        let total = change_key("main", "Locals/total");
        assert!(!v.observe(1, &total, "0"));
        // Drawn again at the same stop: still unchanged.
        assert!(!v.observe(1, &total, "0"));
        assert!(v.observe(2, &total, "6"));
        // Stays marked while the stop is shown.
        assert!(v.observe(2, &total, "6"));
        // The next stop with the same value: no longer changed.
        assert!(!v.observe(3, &total, "6"));
    }

    #[test]
    fn values_are_kept_apart_by_frame() {
        let mut v = ValueChanges::default();
        assert!(!v.observe(1, &change_key("main", "Locals/x"), "1"));
        assert!(!v.observe(1, &change_key("area", "Locals/x"), "9"));
        assert!(!v.observe(2, &change_key("main", "Locals/x"), "1"));
    }

    #[test]
    fn a_new_session_starts_fresh() {
        let mut v = ValueChanges::default();
        let key = change_key("main", "Locals/x");
        v.observe(5, &key, "1");
        assert!(!v.observe(1, &key, "2"));
        assert!(v.observe(2, &key, "3"));
    }

    #[test]
    fn unseen_values_are_not_changed() {
        let mut v = ValueChanges::default();
        assert!(!v.observe(4, &change_key("main", "Locals/new"), "1"));
    }
}
