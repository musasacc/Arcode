//! Change-gutter hunks against the index, and reverting them in a buffer.

use std::ops::Range;
use std::path::Path;

use crate::diff::{line_span, line_starts};
use crate::exec::{exec, repo_path};
use crate::{ChangeKind, GitResult, LineChange, Repo, line_diff};

/// One changed block of a file, in zero-based editor lines (see
/// [`crate::split_lines`]).
///
/// `old_*` refer to the base text (the index for [`diff_against_index`]),
/// `new_*` to the compared text (the editor buffer).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hunk {
    /// First base line of the block (for [`ChangeKind::Added`], the base
    /// line the insertion precedes).
    pub old_start: usize,
    /// Number of base lines replaced or removed.
    pub old_lines: usize,
    /// First new line of the block (for [`ChangeKind::Deleted`], the line
    /// after the removal; may equal the line count).
    pub new_start: usize,
    /// Number of new lines.
    pub new_lines: usize,
    /// Added, modified or deleted.
    pub kind: ChangeKind,
    /// The base lines of the block, each with its original terminator
    /// (empty for [`ChangeKind::Added`]). Used for hover previews and revert.
    pub old_text: String,
}

impl Hunk {
    /// Base line range.
    pub fn old_range(&self) -> Range<usize> {
        self.old_start..self.old_start + self.old_lines
    }

    /// New line range.
    pub fn new_range(&self) -> Range<usize> {
        self.new_start..self.new_start + self.new_lines
    }

    pub(crate) fn from_change(change: &LineChange, old: &str, old_starts: &[usize]) -> Self {
        let span = line_span(old_starts, &change.old_lines);
        Self {
            old_start: change.old_lines.start,
            old_lines: change.old_lines.len(),
            new_start: change.new_lines.start,
            new_lines: change.new_lines.len(),
            kind: change.kind,
            old_text: old.get(span).unwrap_or_default().to_owned(),
        }
    }
}

/// Computes hunks turning `old` into `new`.
pub fn diff_hunks(old: &str, new: &str) -> Vec<Hunk> {
    let starts = line_starts(old);
    line_diff(old, new)
        .iter()
        .map(|change| Hunk::from_change(change, old, &starts))
        .collect()
}

/// What a path's base version looks like.
pub(crate) enum Base {
    /// UTF-8 content from the index or `HEAD`.
    Text(String),
    /// Present but not UTF-8.
    Binary,
    /// Not in the index nor in `HEAD`.
    Missing,
}

impl Repo {
    /// Returns the staged (index, stage 0) content of `rel_path`.
    ///
    /// Like [`Repo::head_file_contents`]: `None` when git is missing, the
    /// path is not in the index (or only as conflict stages), or the blob
    /// is not UTF-8.
    pub fn index_file_contents(&self, rel_path: &Path) -> Option<String> {
        match blob_text(self, rel_path, ":0:") {
            Base::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// Reads `<prefix><path>` (`:0:` for the index, `HEAD:` for HEAD); any
/// failure counts as [`Base::Missing`].
pub(crate) fn blob_text(repo: &Repo, path: &Path, prefix: &str) -> Base {
    let Ok(path) = repo_path(repo, path) else {
        return Base::Missing;
    };
    let spec = format!("{prefix}{}", path.git);
    match exec(repo, &["cat-file", "blob", spec.as_str()], None) {
        Ok(out) => String::from_utf8(out).map_or(Base::Binary, Base::Text),
        Err(_) => Base::Missing,
    }
}

/// The index version of `path`, falling back to `HEAD` (e.g. while the path
/// is conflicted or removed from the index).
pub(crate) fn index_or_head(repo: &Repo, path: &Path) -> Base {
    match blob_text(repo, path, ":0:") {
        Base::Missing => blob_text(repo, path, "HEAD:"),
        found => found,
    }
}

/// Gutter hunks for the editor buffer `current_text` of `rel_path`.
///
/// Diffs against the index version (`git cat-file blob :0:<path>`), falling
/// back to `HEAD`. An untracked file is one [`ChangeKind::Added`] hunk;
/// ignored, binary or out-of-repository files, and any git failure, give
/// no hunks.
pub fn diff_against_index(repo: &Repo, rel_path: &Path, current_text: &str) -> Vec<Hunk> {
    match index_or_head(repo, rel_path) {
        Base::Text(base) => diff_hunks(&base, current_text),
        Base::Binary => Vec::new(),
        Base::Missing if is_untracked(repo, rel_path) => diff_hunks("", current_text),
        Base::Missing => Vec::new(),
    }
}

/// Hunks staged for `rel_path`: `HEAD` (old) against the index (new).
/// Pass them to [`crate::unstage_hunk`].
pub fn staged_hunks(repo: &Repo, rel_path: &Path) -> GitResult<Vec<Hunk>> {
    let path = repo_path(repo, rel_path)?;
    let text = |base| match base {
        Base::Text(text) => Ok(text),
        Base::Binary => Err(crate::GitError::Binary(path.rel.clone())),
        Base::Missing => Ok(String::new()),
    };
    let head = text(blob_text(repo, rel_path, "HEAD:"))?;
    let index = text(blob_text(repo, rel_path, ":0:"))?;
    Ok(diff_hunks(&head, &index))
}

/// True when the path is inside the working tree and not ignored. Ignored
/// paths and git failures count as not untracked.
fn is_untracked(repo: &Repo, rel_path: &Path) -> bool {
    let Ok(path) = repo_path(repo, rel_path) else {
        return false;
    };
    let args = ["check-ignore", "-q", "--no-index", "--", path.git.as_str()];
    // Exit 0: ignored; exit 1: not ignored; anything else: error.
    matches!(
        exec(repo, &args, None),
        Err(crate::GitError::Command { code: Some(1), .. })
    )
}

/// How to undo a hunk in the editor buffer: replace the whole lines
/// `lines` (bytes `bytes` of the text the hunk was computed against) with
/// `text`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HunkRevert {
    /// Buffer lines to replace (empty for an insertion before `lines.start`).
    pub lines: Range<usize>,
    /// The same region as a byte range of the current text.
    pub bytes: Range<usize>,
    /// The replacement text.
    pub text: String,
}

impl HunkRevert {
    /// Applies the replacement to `text` (the text the hunk was computed
    /// against), returning the new text.
    pub fn apply(&self, text: &str) -> String {
        let end = self.bytes.end.min(text.len());
        let start = self.bytes.start.min(end);
        let (Some(head), Some(tail)) = (text.get(..start), text.get(end..)) else {
            return text.to_owned();
        };
        format!("{head}{}{tail}", self.text)
    }
}

/// Computes the edit that restores a hunk's base lines in `current_text`
/// (the text the hunk was computed against). Writes nothing; the editor
/// applies the edit so it lands in undo history.
///
/// Restored lines take the buffer's line ending (`\r\n` when the buffer's
/// first line break is one), and a newline is inserted when restoring
/// after a final line that has none.
pub fn revert_hunk(hunk: &Hunk, current_text: &str) -> HunkRevert {
    let starts = line_starts(current_text);
    let bytes = line_span(&starts, &hunk.new_range());
    let mut text = hunk.old_text.clone();
    if uses_crlf(current_text) {
        text = to_crlf(&text);
    }
    let unterminated_end = bytes.start == current_text.len()
        && current_text
            .chars()
            .next_back()
            .is_some_and(|c| !crate::diff::is_line_break(c));
    if unterminated_end && !text.is_empty() {
        let eol = if uses_crlf(current_text) {
            "\r\n"
        } else {
            "\n"
        };
        text.insert_str(0, eol);
    }
    HunkRevert {
        lines: hunk.new_range(),
        bytes,
        text,
    }
}

/// True when the first `\n` in `text` is preceded by `\r`.
fn uses_crlf(text: &str) -> bool {
    text.find('\n').is_some_and(|i| text[..i].ends_with('\r'))
}

/// Converts bare `\n` to `\r\n`, leaving existing `\r\n` alone.
fn to_crlf(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 16);
    let mut prev = '\0';
    for c in text.chars() {
        if c == '\n' && prev != '\r' {
            out.push('\r');
        }
        out.push(c);
        prev = c;
    }
    out
}

#[cfg(test)]
#[path = "hunk_tests.rs"]
mod tests;
