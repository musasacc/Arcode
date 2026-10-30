//! Project-wide search: [`search`], [`search_collect`] and [`search_text`].

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use grep_matcher::LineTerminator;
use grep_regex::RegexMatcher;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};

use crate::cancel::{CancelToken, Halt};
use crate::error::Result;
use crate::line::LineMatch;
use crate::query::SearchQuery;
use crate::walk::{absolute_root, walk};

/// All matching lines of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMatches {
    /// Absolute path of the file.
    pub path: PathBuf,
    /// Path relative to the search root.
    pub relative: PathBuf,
    /// Matching lines in ascending line order.
    pub matches: Vec<LineMatch>,
}

impl FileMatches {
    /// Total number of matches (not lines) in this file.
    pub fn match_count(&self) -> usize {
        self.matches.iter().map(LineMatch::match_count).sum()
    }
}

/// Summary of one search run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Files opened and searched.
    pub files_searched: usize,
    /// Files with at least one delivered match.
    pub files_matched: usize,
    /// Matches delivered (a line with three hits counts three).
    pub matches: usize,
    /// The search stopped at `max_results`, so results are incomplete.
    pub truncated: bool,
    /// The [`CancelToken`] fired before the search finished.
    pub cancelled: bool,
    /// Wall-clock duration.
    pub elapsed: Duration,
}

/// Search every non-ignored, non-binary file under `root` (a directory or a
/// single file). Results stream to `on_match` per file, on the calling
/// thread, in no particular order; return `ControlFlow::Break(())` to stop.
/// Blocks until finished, so call it from a background thread.
///
/// When `max_results` is hit, `truncated` is set; which files made the cut
/// depends on thread scheduling.
pub fn search(
    root: &Path,
    query: &SearchQuery,
    cancel: &CancelToken,
    mut on_match: impl FnMut(FileMatches) -> ControlFlow<()>,
) -> Result<SearchStats> {
    let started = Instant::now();
    let matcher = query.matcher()?;
    let root = absolute_root(root)?;
    let halt = Halt::new(cancel);
    let budget = Budget::new(query.max_results);
    let mut stats = SearchStats::default();

    let walked = walk(
        &root,
        &query.walk_config(),
        &halt,
        || {
            let matcher = matcher.clone();
            let mut searcher = new_searcher();
            let (halt, budget) = (&halt, &budget);
            move |path: &Path, relative: &Path| {
                let mut sink = LineSink::new(&matcher, halt, budget);
                if let Err(err) = searcher.search_path(&matcher, path, &mut sink) {
                    log::debug!("search: {}: {err}", path.display());
                }
                (!sink.binary && !sink.lines.is_empty()).then(|| FileMatches {
                    path: path.to_path_buf(),
                    relative: relative.to_path_buf(),
                    matches: sink.lines,
                })
            }
        },
        |file| {
            stats.files_matched += 1;
            stats.matches += file.match_count();
            on_match(file)
        },
    )?;

    stats.files_searched = walked.files_visited;
    stats.truncated = budget.exhausted.load(Ordering::Relaxed);
    stats.cancelled = cancel.is_cancelled();
    stats.elapsed = started.elapsed();
    Ok(stats)
}

/// Like [`search`], but collects everything and sorts by relative path, so
/// the result is deterministic (unless truncated).
pub fn search_collect(
    root: &Path,
    query: &SearchQuery,
    cancel: &CancelToken,
) -> Result<(Vec<FileMatches>, SearchStats)> {
    let mut files = Vec::new();
    let stats = search(root, query, cancel, |f| {
        files.push(f);
        ControlFlow::Continue(())
    })?;
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok((files, stats))
}

/// Search an in-memory text (e.g. an unsaved buffer) with the same rules as
/// [`search`] (binary detection aside). `max_results` applies.
pub fn search_text(text: &str, query: &SearchQuery) -> Result<Vec<LineMatch>> {
    let matcher = query.matcher()?;
    let token = CancelToken::new();
    let halt = Halt::new(&token);
    let budget = Budget::new(query.max_results);
    let mut sink = LineSink::new(&matcher, &halt, &budget);
    let mut searcher = new_searcher();
    searcher.set_binary_detection(BinaryDetection::none());
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if let Err(err) = searcher.search_slice(&matcher, text.as_bytes(), &mut sink) {
        log::debug!("search_text: {err}");
    }
    Ok(sink.lines)
}

pub(crate) fn new_searcher() -> Searcher {
    // Must agree with the matcher's `crlf(true)` line terminator.
    SearcherBuilder::new()
        .line_terminator(LineTerminator::crlf())
        .line_number(true)
        .binary_detection(BinaryDetection::quit(0))
        .bom_sniffing(true)
        .build()
}

/// Shared cap on the number of matches.
struct Budget {
    remaining: AtomicUsize,
    exhausted: AtomicBool,
}

impl Budget {
    fn new(max: usize) -> Self {
        Self {
            remaining: AtomicUsize::new(if max == 0 { usize::MAX } else { max }),
            exhausted: AtomicBool::new(false),
        }
    }

    /// Reserve up to `n` matches; returns how many were granted.
    fn take(&self, n: usize) -> usize {
        let prev = self
            .remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |r| {
                Some(r.saturating_sub(n))
            })
            .unwrap_or(0);
        let granted = prev.min(n);
        if granted < n {
            self.exhausted.store(true, Ordering::Relaxed);
        }
        granted
    }
}

/// Collects [`LineMatch`]es of one file.
struct LineSink<'a> {
    matcher: &'a RegexMatcher,
    halt: &'a Halt<'a>,
    budget: &'a Budget,
    lines: Vec<LineMatch>,
    binary: bool,
}

impl<'a> LineSink<'a> {
    fn new(matcher: &'a RegexMatcher, halt: &'a Halt<'a>, budget: &'a Budget) -> Self {
        Self {
            matcher,
            halt,
            budget,
            lines: Vec::new(),
            binary: false,
        }
    }
}

impl Sink for LineSink<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if self.halt.should_stop() {
            return Ok(false);
        }
        let first = mat.line_number().unwrap_or(1).saturating_sub(1) as usize;
        for (i, raw) in mat.lines().enumerate() {
            let number = first + i;
            let Some(line) = LineMatch::build(self.matcher, number, raw, usize::MAX) else {
                continue;
            };
            let wanted = line.match_count();
            let granted = self.budget.take(wanted);
            if granted == wanted {
                self.lines.push(line);
                continue;
            }
            if granted > 0 {
                self.lines
                    .extend(LineMatch::build(self.matcher, number, raw, granted));
            }
            self.halt.stop();
            return Ok(false);
        }
        Ok(true)
    }

    fn binary_data(&mut self, _: &Searcher, _offset: u64) -> Result<bool, Self::Error> {
        self.binary = true;
        Ok(false)
    }
}
