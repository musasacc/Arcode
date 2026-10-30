//! tree-sitter parsing, incremental reparse and highlight spans for Arcode.
//!
//! - [`Language`] and the registry ([`language_for_path`], [`language_by_id`],
//!   [`all_languages`]) describe the languages Arcode knows.
//! - [`Syntax`] holds a tree-sitter tree for one document. Feed it edits with
//!   [`Syntax::edit`] and call [`Syntax::reparse`] with the new [`ropey::Rope`].
//! - [`Syntax::highlight_lines`] returns [`HighlightSpan`]s for the visible
//!   lines only; names come from [`HIGHLIGHT_NAMES`].
//! - [`Syntax::matching_bracket`] finds bracket partners using the tree.
//! - [`highlight_excerpt`] highlights a few lines of a huge document by
//!   parsing only an excerpt, so no full tree has to be kept.
//! - [`Syntax::runnables`] finds mains, tests and benchmarks ([`RunnableItem`]).
//! - [`Syntax::symbols`] lists the document outline ([`OutlineItem`]).
//!
//! This crate does not depend on GPUI or on `arcode-buffer`: callers convert
//! buffer edits into [`InputEditData`].

mod anchor;
mod brackets;
mod edit;
mod excerpt;
mod highlight;
mod language;
mod languages;
mod outline;
mod query_match;
mod registry;
mod resolve;
mod rope_text;
mod runnables;
mod syntax;

pub use brackets::{PLAIN_SCAN_LIMIT, matching_bracket_plain};
pub use edit::InputEditData;
pub use excerpt::{EXCERPT_CONTEXT_LINES, EXCERPT_MAX_BYTES, highlight_excerpt};
pub use highlight::{HIGHLIGHT_NAMES, HighlightSpan};
pub use language::Language;
pub use outline::{OutlineItem, OutlineKind};
pub use registry::{all_languages, language_by_id, language_for_path, plain_text};
pub use rope_text::point_for_byte;
pub use runnables::{RunnableItem, RunnableKind};
pub use syntax::Syntax;

/// Re-export so callers can use tree types without a direct dependency.
pub use tree_sitter;
