//! `\n`-only line indexing on top of ropey.
//!
//! Depending on its cargo features (which are unified across the whole
//! workspace), ropey may also treat VT, FF, NEL, U+2028 and U+2029 as line
//! breaks. tree-sitter, LSP and the editor count only `\n`. This module
//! records the positions of those "extra" breaks (rare in practice) so the
//! buffer can translate ropey's line numbers into `\n`-only line numbers
//! regardless of how ropey was compiled.

use std::sync::OnceLock;

use ropey::Rope;

/// Chars that some ropey builds treat as line breaks besides `\n` (`\r` never
/// occurs in a buffer: text is normalized).
const CANDIDATES: [char; 5] = ['\u{000B}', '\u{000C}', '\u{0085}', '\u{2028}', '\u{2029}'];

/// Which of [`CANDIDATES`] the linked ropey build counts as line breaks.
fn ropey_breaks() -> &'static [bool; 5] {
    static TABLE: OnceLock<[bool; 5]> = OnceLock::new();
    TABLE.get_or_init(|| {
        CANDIDATES.map(|c| {
            let mut buf = [0u8; 4];
            Rope::from_str(c.encode_utf8(&mut buf)).len_lines() == 2
        })
    })
}

/// True if ropey treats `c` as a line break but the buffer does not.
pub(crate) fn is_extra_break(c: char) -> bool {
    // Fast path: every candidate is a control or U+2028/9 char.
    if !(c <= '\u{0085}' || c == '\u{2028}' || c == '\u{2029}') {
        return false;
    }
    CANDIDATES
        .iter()
        .zip(ropey_breaks())
        .any(|(&cand, &counts)| counts && cand == c)
}

/// Sorted char positions of extra line breaks in the rope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExtraBreaks {
    positions: Vec<usize>,
}

impl ExtraBreaks {
    /// Scans `text` (already normalized).
    pub(crate) fn scan(text: &str) -> Self {
        ExtraBreaks {
            positions: extra_offsets(text, 0),
        }
    }

    /// Number of extra breaks.
    pub(crate) fn len(&self) -> usize {
        self.positions.len()
    }

    /// Number of extra breaks at positions `< char_idx`.
    pub(crate) fn count_before(&self, char_idx: usize) -> usize {
        self.positions.partition_point(|&p| p < char_idx)
    }

    /// The extra break with index `i` (sorted order).
    pub(crate) fn get(&self, i: usize) -> usize {
        self.positions[i]
    }

    /// Updates the positions for replacing `start..start + old_len` with
    /// `new_text`.
    pub(crate) fn on_edit(&mut self, start: usize, old_len: usize, new_text: &str) {
        let inserted = extra_offsets(new_text, start);
        if self.positions.is_empty() && inserted.is_empty() {
            return;
        }
        let end = start + old_len;
        let new_len = new_text.chars().count();
        let lo = self.count_before(start);
        let hi = self.count_before(end);
        for p in &mut self.positions[hi..] {
            *p = *p - old_len + new_len;
        }
        self.positions.splice(lo..hi, inserted);
    }
}

/// Char offsets (plus `base`) of extra breaks in `text`.
fn extra_offsets(text: &str, base: usize) -> Vec<usize> {
    // Every candidate's UTF-8 form contains a byte 0x0B, 0x0C, 0xC2 or 0xE2.
    if !text.bytes().any(|b| matches!(b, 0x0B | 0x0C | 0xC2 | 0xE2)) {
        return Vec::new();
    }
    text.chars()
        .enumerate()
        .filter(|&(_, c)| is_extra_break(c))
        .map(|(i, _)| base + i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn any_counted() -> bool {
        ropey_breaks().iter().any(|&b| b)
    }

    #[test]
    fn newline_is_never_extra() {
        assert!(!is_extra_break('\n'));
        assert!(!is_extra_break('a'));
        assert!(!is_extra_break('中'));
    }

    #[test]
    fn scan_and_edit() {
        let mut e = ExtraBreaks::scan("a\u{c}b\u{2028}c");
        if !any_counted() {
            assert_eq!(e.len(), 0);
            return;
        }
        let ff = ropey_breaks()[1];
        let ls = ropey_breaks()[3];
        let expect = |v: &[(usize, bool)]| -> Vec<usize> {
            v.iter().filter(|(_, k)| *k).map(|(p, _)| *p).collect()
        };
        assert_eq!(e.positions, expect(&[(1, ff), (3, ls)]));
        // Replace "b" (2..3) with "\u{c}\u{c}xy".
        e.on_edit(2, 1, "\u{c}\u{c}xy");
        assert_eq!(e.positions, expect(&[(1, ff), (2, ff), (3, ff), (6, ls)]));
        // Delete everything from 1 to 6 (exclusive).
        e.on_edit(1, 5, "");
        assert_eq!(e.positions, expect(&[(1, ls)]));
        assert_eq!(e.count_before(1), 0);
        assert_eq!(e.count_before(2), usize::from(ls));
    }
}
