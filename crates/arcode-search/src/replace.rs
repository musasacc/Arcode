//! "Replace in files": [`replace_in_text`], [`replace_in_file`] and
//! [`replace_all`]. Line endings (LF/CRLF, mixed) and a UTF-8 BOM are kept
//! byte for byte; only UTF-8 files are rewritten.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use grep_matcher::{Captures, Matcher};
use grep_regex::RegexMatcher;

use crate::cancel::CancelToken;
use crate::error::{Result, SearchError};
use crate::query::SearchQuery;
use crate::search::search;

const BOM: char = '\u{feff}';

/// A file rewritten by [`replace_all`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub relative: PathBuf,
    pub replacements: usize,
}

/// Why [`replace_all`] left a matching file alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// Not valid UTF-8 (e.g. Latin-1 or UTF-16).
    NotUtf8,
    /// Contains NUL bytes.
    Binary,
    /// Reading or writing failed.
    Io(String),
}

/// A matching file that was not rewritten.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub relative: PathBuf,
    pub reason: SkipReason,
}

/// Outcome of [`replace_all`]. Files are sorted by relative path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplaceSummary {
    pub changed: Vec<ChangedFile>,
    pub skipped: Vec<SkippedFile>,
    /// Total replacements over all changed files.
    pub replacements: usize,
    /// Cancelled part-way: some files may already have been rewritten.
    pub cancelled: bool,
}

/// Replace matches of `query` in `text`. In regex mode `replacement` may
/// use `$1`, `${name}` and `$$`; in literal mode it is inserted verbatim.
/// `only_lines` (zero-based) restricts replacing to those lines. Empty
/// matches are never replaced. Returns the new text and the count.
pub fn replace_in_text(
    text: &str,
    query: &SearchQuery,
    replacement: &str,
    only_lines: Option<&[usize]>,
) -> Result<(String, usize)> {
    let matcher = query.matcher()?;
    Ok(replace_with(
        &matcher,
        query.is_regex,
        text,
        replacement,
        only_lines,
    ))
}

/// Replace matches of `query` in the file at `path` and write it back if
/// anything changed. Fails with [`SearchError::NotUtf8`] or
/// [`SearchError::Binary`] instead of touching such files.
pub fn replace_in_file(
    path: &Path,
    query: &SearchQuery,
    replacement: &str,
    only_lines: Option<&[usize]>,
) -> Result<usize> {
    let matcher = query.matcher()?;
    replace_file_with(&matcher, query.is_regex, path, replacement, only_lines)
}

/// Replace every match of `query` under `root` (ignoring `max_results`).
/// Non-UTF-8 and binary files are reported in `skipped`.
pub fn replace_all(
    root: &Path,
    query: &SearchQuery,
    replacement: &str,
    cancel: &CancelToken,
) -> Result<ReplaceSummary> {
    let matcher = query.matcher()?;
    let unlimited = SearchQuery {
        max_results: 0,
        ..query.clone()
    };
    let mut files = Vec::new();
    let stats = search(root, &unlimited, cancel, |f| {
        files.push((f.path, f.relative));
        ControlFlow::Continue(())
    })?;
    let mut summary = ReplaceSummary {
        cancelled: stats.cancelled,
        ..ReplaceSummary::default()
    };
    files.sort_by(|a, b| a.1.cmp(&b.1));
    for (path, relative) in files {
        if cancel.is_cancelled() {
            summary.cancelled = true;
            break;
        }
        match replace_file_with(&matcher, query.is_regex, &path, replacement, None) {
            Ok(0) => {}
            Ok(n) => {
                summary.replacements += n;
                summary.changed.push(ChangedFile {
                    path,
                    relative,
                    replacements: n,
                });
            }
            Err(err) => {
                let reason = match err {
                    SearchError::NotUtf8(_) => SkipReason::NotUtf8,
                    SearchError::Binary(_) => SkipReason::Binary,
                    other => SkipReason::Io(other.to_string()),
                };
                summary.skipped.push(SkippedFile {
                    path,
                    relative,
                    reason,
                });
            }
        }
    }
    Ok(summary)
}

fn replace_file_with(
    matcher: &RegexMatcher,
    is_regex: bool,
    path: &Path,
    replacement: &str,
    only_lines: Option<&[usize]>,
) -> Result<usize> {
    let bytes = std::fs::read(path).map_err(|e| SearchError::io(path, e))?;
    if bytes.contains(&0) {
        return Err(SearchError::Binary(path.to_path_buf()));
    }
    let text = String::from_utf8(bytes).map_err(|_| SearchError::NotUtf8(path.to_path_buf()))?;
    let (new_text, count) = replace_with(matcher, is_regex, &text, replacement, only_lines);
    if count > 0 {
        std::fs::write(path, new_text).map_err(|e| SearchError::io(path, e))?;
    }
    Ok(count)
}

fn replace_with(
    matcher: &RegexMatcher,
    is_regex: bool,
    text: &str,
    replacement: &str,
    only_lines: Option<&[usize]>,
) -> (String, usize) {
    let mut allowed = only_lines.map(<[usize]>::to_vec);
    if let Some(lines) = allowed.as_mut() {
        lines.sort_unstable();
    }
    let (bom, body) = match text.strip_prefix(BOM) {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let mut out = String::with_capacity(text.len());
    if bom {
        out.push(BOM);
    }
    let mut count = 0;
    for (number, line) in body.split_inclusive('\n').enumerate() {
        let content_len = line
            .strip_suffix('\n')
            .map_or(line.len(), |l| l.strip_suffix('\r').unwrap_or(l).len());
        let (content, terminator) = line.split_at(content_len);
        let wanted = allowed
            .as_ref()
            .is_none_or(|l| l.binary_search(&number).is_ok());
        if wanted {
            let (new, n) = replace_line(matcher, is_regex, content, replacement);
            out.push_str(&new);
            count += n;
        } else {
            out.push_str(content);
        }
        out.push_str(terminator);
    }
    (out, count)
}

/// Replace all non-empty matches within one line (no terminator).
fn replace_line(
    matcher: &RegexMatcher,
    is_regex: bool,
    line: &str,
    replacement: &str,
) -> (String, usize) {
    let hay = line.as_bytes();
    let mut caps = match matcher.new_captures() {
        Ok(caps) => caps,
        Err(_) => return (line.to_owned(), 0),
    };
    let mut dst = Vec::with_capacity(hay.len());
    let mut count = 0;
    let result = matcher.replace_with_captures(hay, &mut caps, &mut dst, |caps, dst| {
        let Some(m) = caps.get(0) else { return true };
        if m.is_empty() {
            return true;
        }
        count += 1;
        if is_regex {
            caps.interpolate(
                |name| matcher.capture_index(name),
                hay,
                replacement.as_bytes(),
                dst,
            );
        } else {
            dst.extend_from_slice(replacement.as_bytes());
        }
        true
    });
    if result.is_err() || count == 0 {
        return (line.to_owned(), 0);
    }
    match String::from_utf8(dst) {
        Ok(s) => (s, count),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), count),
    }
}
