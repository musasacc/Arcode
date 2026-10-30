//! The [`Language`] description and its lazily compiled highlight query.

use std::fmt;
use std::sync::OnceLock;

use tree_sitter::Query;

/// A language Arcode knows about: how to detect it, how to comment in it,
/// and (optionally) how to parse and highlight it with tree-sitter.
///
/// Built-in languages live in `src/languages/`, one small file each, and are
/// listed in the registry (`src/registry.rs`).
pub struct Language {
    /// Display name, e.g. `"Rust"`.
    pub name: &'static str,
    /// LSP `languageId`, e.g. `"rust"`.
    pub id: &'static str,
    /// File extensions without the dot, matched case-insensitively.
    pub extensions: &'static [&'static str],
    /// Exact file names that select this language, e.g. `"Cargo.lock"`.
    pub file_names: &'static [&'static str],
    /// Line comment token, e.g. `"//"`.
    pub line_comment: Option<&'static str>,
    /// Block comment delimiters, e.g. `("/*", "*/")`.
    pub block_comment: Option<(&'static str, &'static str)>,
    /// Returns the tree-sitter grammar, or `None` when there is none.
    pub ts_language: fn() -> Option<tree_sitter::Language>,
    /// Highlight query source (tree-sitter `.scm`). Empty when unused.
    ///
    /// Later patterns take precedence over earlier ones for the same node.
    pub highlights_query: &'static str,
    /// Lazily compiled highlight query (compiled at most once).
    pub(crate) query: OnceLock<Option<Query>>,
}

impl Language {
    /// Returns the tree-sitter grammar for this language, if it has one.
    pub fn grammar(&self) -> Option<tree_sitter::Language> {
        (self.ts_language)()
    }

    /// Whether this language can be parsed with tree-sitter.
    pub fn has_grammar(&self) -> bool {
        self.grammar().is_some()
    }

    /// Returns the compiled highlight query.
    ///
    /// The query is compiled on first use and cached for the life of the
    /// process. Returns `None` for languages without a grammar or query, or
    /// if the query fails to compile (see [`Language::compile_highlights_query`]).
    pub fn highlight_query(&self) -> Option<&Query> {
        self.query
            .get_or_init(|| self.compile_highlights_query().ok())
            .as_ref()
    }

    /// Compiles [`Language::highlights_query`] without caching.
    ///
    /// Mostly useful to report query errors; prefer [`Language::highlight_query`].
    pub fn compile_highlights_query(&self) -> Result<Query, String> {
        let grammar = self
            .grammar()
            .ok_or_else(|| format!("{} has no tree-sitter grammar", self.name))?;
        if self.highlights_query.trim().is_empty() {
            return Err(format!("{} has no highlight query", self.name));
        }
        Query::new(&grammar, self.highlights_query)
            .map_err(|err| format!("{} highlight query: {err}", self.name))
    }
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Language {}

impl fmt::Debug for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Language")
            .field("name", &self.name)
            .field("id", &self.id)
            .field("extensions", &self.extensions)
            .field("file_names", &self.file_names)
            .finish_non_exhaustive()
    }
}
