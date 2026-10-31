//! A read-only inline (unified, whole-file) diff for a diff viewer.

use crate::{ChangeKind, diff_hunks, split_lines};

/// What one line of an [`InlineDiff`] shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffLineKind {
    /// Present in both versions.
    Context,
    /// Only in the new version.
    Added,
    /// Only in the old version.
    Removed,
}

/// Both versions of a file interleaved line by line: unchanged lines once,
/// removed old lines right before the new lines that replace them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlineDiff {
    /// The interleaved text, one `\n`-terminated line per entry of `kinds`.
    pub text: String,
    /// The kind of every line of `text`.
    pub kinds: Vec<DiffLineKind>,
    /// Zero-based line number in the old version, per line.
    pub old_lines: Vec<Option<usize>>,
    /// Zero-based line number in the new version, per line.
    pub new_lines: Vec<Option<usize>>,
}

impl InlineDiff {
    /// Number of lines.
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    /// True when both versions are empty.
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// Index of the first line of every changed block, for next / previous
    /// change navigation.
    pub fn change_starts(&self) -> Vec<usize> {
        let mut starts = Vec::new();
        let mut prev = DiffLineKind::Context;
        for (ix, kind) in self.kinds.iter().enumerate() {
            if *kind != DiffLineKind::Context && prev == DiffLineKind::Context {
                starts.push(ix);
            }
            prev = *kind;
        }
        starts
    }

    fn push(&mut self, line: &str, kind: DiffLineKind, old: Option<usize>, new: Option<usize>) {
        self.text.push_str(line);
        self.text.push('\n');
        self.kinds.push(kind);
        self.old_lines.push(old);
        self.new_lines.push(new);
    }
}

/// Builds the inline diff turning `old` into `new` (line breaks as in
/// [`split_lines`]).
pub fn inline_diff(old: &str, new: &str) -> InlineDiff {
    let old_lines: Vec<&str> = split_lines(old).collect();
    let new_lines: Vec<&str> = split_lines(new).collect();
    let mut out = InlineDiff::default();
    let (mut o, mut n) = (0, 0);
    for hunk in diff_hunks(old, new) {
        while n < hunk.new_start {
            out.push(new_lines[n], DiffLineKind::Context, Some(o), Some(n));
            o += 1;
            n += 1;
        }
        if hunk.kind != ChangeKind::Added {
            for i in hunk.old_range() {
                out.push(old_lines[i], DiffLineKind::Removed, Some(i), None);
            }
        }
        for i in hunk.new_range() {
            out.push(new_lines[i], DiffLineKind::Added, None, Some(i));
        }
        o = hunk.old_start + hunk.old_lines;
        n = hunk.new_start + hunk.new_lines;
    }
    while n < new_lines.len() {
        out.push(new_lines[n], DiffLineKind::Context, Some(o), Some(n));
        o += 1;
        n += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use DiffLineKind::*;

    #[test]
    fn interleaves_removed_before_added() {
        let diff = inline_diff("a\nb\nc\nd\n", "a\nB\nc\nd\ne\n");
        assert_eq!(diff.text, "a\nb\nB\nc\nd\ne\n");
        assert_eq!(
            diff.kinds,
            [Context, Removed, Added, Context, Context, Added]
        );
        assert_eq!(diff.old_lines[3], Some(2));
        assert_eq!(diff.new_lines[3], Some(2));
        assert_eq!(diff.change_starts(), [1, 5]);
    }

    #[test]
    fn handles_empty_sides() {
        let diff = inline_diff("", "x\ny");
        assert_eq!(diff.kinds, [Added, Added]);
        let diff = inline_diff("x\n", "");
        assert_eq!(diff.kinds, [Removed]);
        assert!(inline_diff("", "").is_empty());
    }
}
