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
    /// Runnables query source (see [`Syntax::runnables`](crate::Syntax::runnables)).
    /// Empty when the language has no runnables.
    pub runnables_query: &'static str,
    /// Outline query source (see [`Syntax::symbols`](crate::Syntax::symbols)).
    /// Empty when the language has no outline.
    pub outline_query: &'static str,
    /// Lazily compiled runnables query.
    pub(crate) runnables: OnceLock<Option<Query>>,
    /// Lazily compiled outline query.
    pub(crate) outline: OnceLock<Option<Query>>,
}

/// `const` builders used by the built-in language definitions.
impl Language {
    /// A language with no grammar, queries, extensions or comments.
    pub(crate) const fn new(name: &'static str, id: &'static str) -> Language {
        Language {
            name,
            id,
            extensions: &[],
            file_names: &[],
            line_comment: None,
            block_comment: None,
            ts_language: no_grammar,
            highlights_query: "",
            query: OnceLock::new(),
            runnables_query: "",
            outline_query: "",
            runnables: OnceLock::new(),
            outline: OnceLock::new(),
        }
    }

    pub(crate) const fn with_extensions(mut self, extensions: &'static [&'static str]) -> Self {
        self.extensions = extensions;
        self
    }

    pub(crate) const fn with_file_names(mut self, file_names: &'static [&'static str]) -> Self {
        self.file_names = file_names;
        self
    }

    pub(crate) const fn with_line_comment(mut self, token: &'static str) -> Self {
        self.line_comment = Some(token);
        self
    }

    pub(crate) const fn with_block_comment(
        mut self,
        open: &'static str,
        close: &'static str,
    ) -> Self {
        self.block_comment = Some((open, close));
        self
    }

    pub(crate) const fn with_grammar(
        mut self,
        grammar: fn() -> Option<tree_sitter::Language>,
    ) -> Self {
        self.ts_language = grammar;
        self
    }

    pub(crate) const fn with_highlights(mut self, query: &'static str) -> Self {
        self.highlights_query = query;
        self
    }

    pub(crate) const fn with_runnables(mut self, query: &'static str) -> Self {
        self.runnables_query = query;
        self
    }

    pub(crate) const fn with_outline(mut self, query: &'static str) -> Self {
        self.outline_query = query;
        self
    }
}

fn no_grammar() -> Option<tree_sitter::Language> {
    None
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
        self.compile(self.highlights_query, "highlight")
    }

    /// Returns the compiled runnables query (cached like
    /// [`Language::highlight_query`]), or `None` when there is none.
    pub fn runnables_query(&self) -> Option<&Query> {
        self.runnables
            .get_or_init(|| self.compile_runnables_query().ok())
            .as_ref()
    }

    /// Compiles [`Language::runnables_query`] without caching.
    pub fn compile_runnables_query(&self) -> Result<Query, String> {
        self.compile(self.runnables_query, "runnables")
    }

    /// Returns the compiled outline query (cached), or `None` when there is none.
    pub fn outline_query(&self) -> Option<&Query> {
        self.outline
            .get_or_init(|| self.compile_outline_query().ok())
            .as_ref()
    }

    /// Compiles [`Language::outline_query`] without caching.
    pub fn compile_outline_query(&self) -> Result<Query, String> {
        self.compile(self.outline_query, "outline")
    }

    fn compile(&self, source: &str, what: &str) -> Result<Query, String> {
        let grammar = self
            .grammar()
            .ok_or_else(|| format!("{} has no tree-sitter grammar", self.name))?;
        if source.trim().is_empty() {
            return Err(format!("{} has no {what} query", self.name));
        }
        Query::new(&grammar, source).map_err(|err| format!("{} {what} query: {err}", self.name))
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
