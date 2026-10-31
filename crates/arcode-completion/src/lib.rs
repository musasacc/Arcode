//! Completion and snippets for Arcode, independent of UI and LSP transport.
//!
//! - [`CompletionCandidate`]: one item from any source (LSP, buffer words,
//!   paths, snippets).
//! - [`rank`] / [`Ranker`]: fuzzy ranking with nucleo-matcher, smart case,
//!   prefix and `sort_text` tie-breakers, duplicate merging.
//! - [`WordIndex`]: incremental buffer-word index (fallback without LSP).
//! - [`path_completions`] / [`detect_path_prefix`]: file paths in strings.
//! - [`snippet`]: LSP/TextMate snippet parser, [`expand`],
//!   [`SnippetSession`] tabstop tracking, built-in and TOML user snippets.
//!
//! All buffer offsets are **char indices**, like `arcode-buffer`.
//!
//! ```
//! use arcode_completion::{CandidateKind, CandidateSource, CompletionCandidate, rank};
//!
//! let items: Vec<_> = ["format", "from_str", "foo"]
//!     .into_iter()
//!     .map(|l| CompletionCandidate::new(l, CandidateKind::Function, CandidateSource::BufferWord))
//!     .collect();
//! let ranked = rank("fs", &items, 10);
//! assert_eq!(items[ranked[0].index].label, "from_str");
//! ```

mod candidate;
mod paths;
mod rank;
pub mod snippet;
mod words;

pub use candidate::{CandidateKind, CandidateSource, CompletionCandidate, InsertAction};
pub use paths::{
    MAX_PATH_ENTRIES, detect_path_prefix, looks_like_path, path_completions, path_query,
    split_typed,
};
pub use rank::{RankedCandidate, Ranker, merge_duplicates, rank};
pub use snippet::{
    ExpandedSnippet, Snippet, SnippetDef, SnippetLibrary, SnippetSession, Tabstop, VariableContext,
    expand, load_snippets, snippets_for,
};
pub use words::{MAX_WORD_LEN, MIN_WORD_LEN, WordIndex, word_at, words_in};
