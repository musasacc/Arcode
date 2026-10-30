//! Project-wide search, replace and TODO scanning for Arcode, built on the
//! ripgrep libraries. UI-agnostic: every entry point blocks, so run it on a
//! background thread and stop it with a [`CancelToken`].
//!
//! - [`search`] streams [`FileMatches`] per file; [`search_collect`] returns
//!   them sorted; [`search_text`] searches an in-memory buffer.
//! - [`replace_in_text`], [`replace_in_file`], [`replace_all`] implement
//!   "replace in files" (regex `$1` groups, line endings preserved);
//!   [`line_replacements`] gives each match of a line with its expansion
//!   (for undoable edits in an open buffer).
//! - [`scan_todos`] / [`scan_todos_with`] find TODO/FIXME/... comments in a
//!   project; [`scan_todos_in_paths`] rescans only some files of it (after
//!   they changed); [`scan_todos_in_text`] does the same for an open buffer.
//!
//! Lines are zero-based; columns and ranges count chars.

mod cancel;
mod error;
mod line;
mod query;
mod replace;
mod search;
mod todo;
mod todo_scan;
mod walk;

pub use cancel::CancelToken;
pub use error::{Result, SearchError};
pub use line::{ELLIPSIS, LineMatch, MAX_LINE_CHARS};
pub use query::{DEFAULT_MAX_FILE_SIZE, DEFAULT_MAX_RESULTS, SearchQuery, escape, parse_glob_list};
pub use replace::{
    ChangedFile, ReplaceSummary, SkipReason, SkippedFile, line_replacements, replace_all,
    replace_in_file, replace_in_text,
};
pub use search::{FileMatches, SearchStats, search, search_collect, search_text};
pub use todo::{DEFAULT_TODO_TAGS, TodoMatch, parse_todo_line, scan_todos_in_text};
pub use todo_scan::{TodoItem, TodoOptions, scan_todos, scan_todos_in_paths, scan_todos_with};
