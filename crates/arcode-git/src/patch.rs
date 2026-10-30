//! Builds minimal unified diffs for a single hunk, for `git apply`.
//!
//! Hunks use the editor's line model ([`crate::split_lines`]); git only
//! breaks lines at `\n`. The patch therefore widens a hunk to whole `\n`
//! lines (e.g. to include an unterminated last line that gains a newline)
//! and emits those lines verbatim, with `\ No newline at end of file`
//! markers where needed.

use std::fmt::Write as _;
use std::ops::Range;

use crate::GitError;
use crate::diff::{line_span, line_starts};

/// Lines of unchanged context around the hunk; checked by `git apply`.
const CONTEXT: usize = 3;

/// A patch for one hunk turning lines `old` of `base` into lines `new` of
/// `target` (editor line ranges). The patch applies to `base`.
pub(crate) fn hunk_patch(
    git_path: &str,
    base: &str,
    target: &str,
    old: Range<usize>,
    new: Range<usize>,
) -> Result<String, GitError> {
    let (ob, nb) = align(base, target, old, new)?;
    let start = base[..ob.start].matches('\n').count();
    let removed: Vec<&str> = base[ob.clone()].split_inclusive('\n').collect();
    let added: Vec<&str> = target[nb].split_inclusive('\n').collect();
    let before: Vec<&str> = base[..ob.start].split_inclusive('\n').collect();
    let before = &before[before.len().saturating_sub(CONTEXT)..];
    let after: Vec<&str> = base[ob.end..].split_inclusive('\n').take(CONTEXT).collect();

    let ctx = before.len() + after.len();
    let first = start - before.len();
    let mut out = header(git_path);
    let _ = writeln!(
        out,
        "@@ -{} +{} @@",
        range_header(first, removed.len() + ctx),
        range_header(first, added.len() + ctx)
    );
    push_lines(&mut out, ' ', before);
    push_lines(&mut out, '-', &removed);
    push_lines(&mut out, '+', &added);
    push_lines(&mut out, ' ', &after);
    Ok(out)
}

/// A patch adding the untracked file `git_path` with content `text`.
pub(crate) fn new_file_patch(git_path: &str, text: &str, executable: bool) -> String {
    let quoted = quote(&format!("b/{git_path}"));
    let mode = if executable { "100755" } else { "100644" };
    let mut out = format!(
        "diff --git {} {quoted}\nnew file mode {mode}\n",
        quote(&format!("a/{git_path}"))
    );
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if !lines.is_empty() {
        let _ = writeln!(out, "--- /dev/null\n+++ {quoted}");
        let _ = writeln!(out, "@@ -0,0 +{} @@", range_header(0, lines.len()));
        push_lines(&mut out, '+', &lines);
    }
    out
}

/// Widens editor line ranges until both byte ranges start and end on `\n`
/// line boundaries; returns the byte ranges.
fn align(
    base: &str,
    target: &str,
    mut old: Range<usize>,
    mut new: Range<usize>,
) -> Result<(Range<usize>, Range<usize>), GitError> {
    let (bs, ts) = (line_starts(base), line_starts(target));
    // `starts` has one entry per line plus the end.
    let (base_lines, target_lines) = (bs.len() - 1, ts.len() - 1);
    if old.start > old.end || new.start > new.end || old.end > base_lines || new.end > target_lines
    {
        return Err(GitError::StaleHunk);
    }
    loop {
        let (ob, nb) = (line_span(&bs, &old), line_span(&ts, &new));
        if !starts_line(base, ob.start) || !starts_line(target, nb.start) {
            if old.start == 0 || new.start == 0 {
                return Err(GitError::StaleHunk);
            }
            old.start -= 1;
            new.start -= 1;
        } else if !ends_line(base, ob.end) || !ends_line(target, nb.end) {
            if old.end >= base_lines || new.end >= target_lines {
                return Err(GitError::StaleHunk);
            }
            old.end += 1;
            new.end += 1;
        } else {
            return Ok((ob, nb));
        }
    }
}

/// True when byte `pos` begins a `\n`-terminated line (or the empty end of
/// a text whose last line is terminated).
fn starts_line(text: &str, pos: usize) -> bool {
    pos == 0 || text.as_bytes().get(pos - 1) == Some(&b'\n')
}

/// True when byte `pos` ends a line: after a `\n` or at the end of text.
fn ends_line(text: &str, pos: usize) -> bool {
    pos >= text.len() || starts_line(text, pos)
}

/// `start,count` for a hunk header; `first` is the zero-based first line.
/// An empty range names the line before it, as unified diffs do.
fn range_header(first: usize, count: usize) -> String {
    if count == 0 {
        format!("{first},0")
    } else {
        format!("{},{count}", first + 1)
    }
}

fn header(git_path: &str) -> String {
    let (a, b) = (
        quote(&format!("a/{git_path}")),
        quote(&format!("b/{git_path}")),
    );
    format!("diff --git {a} {b}\n--- {a}\n+++ {b}\n")
}

fn push_lines(out: &mut String, prefix: char, lines: &[&str]) {
    for line in lines {
        out.push(prefix);
        out.push_str(line);
        if !line.ends_with('\n') {
            out.push_str("\n\\ No newline at end of file\n");
        }
    }
}

/// C-style quotes a patch path; git accepts quoted names everywhere, which
/// keeps spaces, tabs and quotes unambiguous.
fn quote(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 2);
    out.push('"');
    for c in path.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\{:03o}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;
