//! Project-wide TODO scanning: [`scan_todos`] and [`scan_todos_with`].

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{Searcher, Sink, SinkMatch};

use crate::cancel::{CancelToken, Halt};
use crate::error::{Result, SearchError};
use crate::line::trim_terminator;
use crate::query::{DEFAULT_MAX_FILE_SIZE, escape};
use crate::search::new_searcher;
use crate::todo::{TodoMatch, effective_tags, parse_todo_line};
use crate::walk::{WalkConfig, absolute_root, walk};

/// A TODO found in a project file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TodoItem {
    /// Absolute path.
    pub path: PathBuf,
    /// Path relative to the scanned root.
    pub relative: PathBuf,
    /// Zero-based line.
    pub line: usize,
    /// Char column of the tag.
    pub column: usize,
    pub tag: String,
    pub text: String,
    pub author: Option<String>,
}

impl TodoItem {
    fn new(path: &Path, relative: &Path, m: TodoMatch) -> Self {
        Self {
            path: path.to_path_buf(),
            relative: relative.to_path_buf(),
            line: m.line,
            column: m.column,
            tag: m.tag,
            text: m.text,
            author: m.author,
        }
    }
}

/// Options for [`scan_todos_with`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoOptions {
    /// Tags to find; empty means [`crate::DEFAULT_TODO_TAGS`].
    pub tags: Vec<String>,
    /// Match tags case-sensitively (default `true`).
    pub case_sensitive: bool,
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    pub include_globs: Vec<String>,
    pub exclude_globs: Vec<String>,
    /// Skip larger files (`0` = unlimited).
    pub max_file_size: u64,
    /// Stop after this many items (`0` = unlimited).
    pub max_items: usize,
}

impl Default for TodoOptions {
    fn default() -> Self {
        Self {
            tags: Vec::new(),
            case_sensitive: true,
            include_hidden: false,
            respect_gitignore: true,
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            max_items: 10_000,
        }
    }
}

/// Scan `root` for TODOs with default options. Empty `tags` means
/// [`crate::DEFAULT_TODO_TAGS`]. Sorted by relative path, then line. A
/// missing root yields an empty list.
pub fn scan_todos(root: &Path, tags: &[&str], cancel: &CancelToken) -> Vec<TodoItem> {
    let opts = TodoOptions {
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        ..TodoOptions::default()
    };
    scan_todos_with(root, &opts, cancel).unwrap_or_else(|err| {
        log::debug!("scan_todos: {err}");
        Vec::new()
    })
}

/// Scan `root` for TODOs. Sorted by relative path, then line.
pub fn scan_todos_with(
    root: &Path,
    opts: &TodoOptions,
    cancel: &CancelToken,
) -> Result<Vec<TodoItem>> {
    let tag_refs: Vec<&str> = opts.tags.iter().map(String::as_str).collect();
    let tags = effective_tags(&tag_refs);
    let matcher = prefilter(&tags, opts.case_sensitive)?;
    let root = absolute_root(root)?;
    let cfg = WalkConfig {
        include_hidden: opts.include_hidden,
        respect_gitignore: opts.respect_gitignore,
        include_globs: &opts.include_globs,
        exclude_globs: &opts.exclude_globs,
        max_file_size: opts.max_file_size,
    };
    let halt = Halt::new(cancel);
    let limit = if opts.max_items == 0 {
        usize::MAX
    } else {
        opts.max_items
    };
    let mut items: Vec<TodoItem> = Vec::new();
    walk(
        &root,
        &cfg,
        &halt,
        || {
            let mut searcher = new_searcher();
            let (matcher, tags, halt) = (&matcher, &tags, &halt);
            move |path: &Path, relative: &Path| {
                let mut sink = TodoSink {
                    tags,
                    case_sensitive: opts.case_sensitive,
                    halt,
                    found: Vec::new(),
                    binary: false,
                    aborted: false,
                };
                if let Err(err) = searcher.search_path(matcher, path, &mut sink) {
                    log::debug!("scan_todos: {}: {err}", path.display());
                    return None;
                }
                // A partial scan may hide a later NUL byte (binary file).
                let complete = !sink.binary && !sink.aborted;
                (complete && !sink.found.is_empty()).then(|| {
                    let found = sink.found.into_iter();
                    found
                        .map(|m| TodoItem::new(path, relative, m))
                        .collect::<Vec<_>>()
                })
            }
        },
        |file| {
            items.extend(file);
            if items.len() >= limit {
                items.truncate(limit);
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    )?;
    items.sort_by(|a, b| (&a.relative, a.line).cmp(&(&b.relative, b.line)));
    Ok(items)
}

/// Cheap line filter: any tag anywhere on the line.
fn prefilter(tags: &[&str], case_sensitive: bool) -> Result<RegexMatcher> {
    if tags.is_empty() {
        return Err(SearchError::EmptyPattern);
    }
    let alternation: Vec<String> = tags.iter().map(|t| escape(t)).collect();
    RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .crlf(true)
        .ban_byte(Some(0))
        .build(&alternation.join("|"))
        .map_err(|e| SearchError::InvalidPattern(e.to_string()))
}

struct TodoSink<'a> {
    tags: &'a [&'a str],
    case_sensitive: bool,
    halt: &'a Halt<'a>,
    found: Vec<TodoMatch>,
    binary: bool,
    /// Stopped before the end of the file.
    aborted: bool,
}

impl Sink for TodoSink<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if self.halt.should_stop() {
            self.aborted = true;
            return Ok(false);
        }
        let first = mat.line_number().unwrap_or(1).saturating_sub(1) as usize;
        for (i, raw) in mat.lines().enumerate() {
            let line = String::from_utf8_lossy(trim_terminator(raw));
            let parsed = parse_todo_line(first + i, &line, self.tags, self.case_sensitive);
            self.found.extend(parsed);
        }
        Ok(true)
    }

    fn binary_data(&mut self, _: &Searcher, _offset: u64) -> Result<bool, Self::Error> {
        self.binary = true;
        Ok(false)
    }
}
