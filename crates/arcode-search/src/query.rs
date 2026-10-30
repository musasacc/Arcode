//! [`SearchQuery`] and its compilation into a ripgrep matcher.

use grep_regex::{RegexMatcher, RegexMatcherBuilder};

use crate::error::{Result, SearchError};
use crate::walk::WalkConfig;

/// Default cap on the number of matches reported by one search.
pub const DEFAULT_MAX_RESULTS: usize = 20_000;
/// Default cap on the size of searched files (10 MiB).
pub const DEFAULT_MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// What to search for and where. `Default` gives a case-insensitive literal
/// search that respects `.gitignore` and skips hidden files.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SearchQuery {
    /// The text or regex to find. Multi-line patterns are not supported.
    pub pattern: String,
    /// Treat `pattern` as a regular expression (Rust `regex` syntax).
    /// Otherwise it is matched literally.
    pub is_regex: bool,
    /// Match case exactly. When `false`, see `smart_case`.
    pub case_sensitive: bool,
    /// With `case_sensitive == false`: become case sensitive if the pattern
    /// contains an uppercase letter.
    pub smart_case: bool,
    /// Only match whole words: the match must not be preceded or followed by
    /// a word character (`\b` semantics for word-character edges).
    pub whole_word: bool,
    /// Only search files matching one of these globs (gitignore syntax,
    /// e.g. `*.rs`, `src/**`). A directory includes its contents; like in
    /// `exclude_globs`, a name without `/` (`src`) matches at any depth and
    /// one with `/` (`crates/app`) is relative to the root. Empty means
    /// "all files".
    pub include_globs: Vec<String>,
    /// Skip files/directories matching any of these globs (gitignore
    /// syntax, same anchoring as `include_globs`).
    pub exclude_globs: Vec<String>,
    /// Also search hidden files and directories (dot files). `.git` is
    /// always skipped.
    pub include_hidden: bool,
    /// Honour `.gitignore`, `.ignore`, `.git/info/exclude` and the global
    /// git excludes (also outside git repositories).
    pub respect_gitignore: bool,
    /// Stop after this many matches (`0` = unlimited).
    pub max_results: usize,
    /// Skip files larger than this many bytes (`0` = unlimited).
    pub max_file_size: u64,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            is_regex: false,
            case_sensitive: false,
            smart_case: false,
            whole_word: false,
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            include_hidden: false,
            respect_gitignore: true,
            max_results: DEFAULT_MAX_RESULTS,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl SearchQuery {
    /// A default query for `pattern`.
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            ..Self::default()
        }
    }

    /// Check that the pattern and globs compile, without searching. Use it
    /// to show errors while the user types.
    pub fn validate(&self) -> Result<()> {
        self.matcher()?;
        crate::walk::build_overrides(std::path::Path::new("."), &self.walk_config())?;
        Ok(())
    }

    /// The effective regex source (escaped in literal mode, wrapped for
    /// whole-word).
    pub fn regex_source(&self) -> String {
        let core = if self.is_regex {
            self.pattern.clone()
        } else {
            escape(&self.pattern)
        };
        if self.whole_word {
            format!(r"\b{{start-half}}(?:{core})\b{{end-half}}")
        } else {
            core
        }
    }

    pub(crate) fn matcher(&self) -> Result<RegexMatcher> {
        if self.pattern.is_empty() {
            return Err(SearchError::EmptyPattern);
        }
        let mut builder = RegexMatcherBuilder::new();
        builder
            .case_insensitive(!self.case_sensitive && !self.smart_case)
            .case_smart(!self.case_sensitive && self.smart_case)
            .multi_line(true)
            .crlf(true)
            .ban_byte(Some(0));
        builder
            .build(&self.regex_source())
            .map_err(|e| SearchError::InvalidPattern(e.to_string()))
    }

    pub(crate) fn walk_config(&self) -> WalkConfig<'_> {
        WalkConfig {
            include_hidden: self.include_hidden,
            respect_gitignore: self.respect_gitignore,
            include_globs: &self.include_globs,
            exclude_globs: &self.exclude_globs,
            max_file_size: self.max_file_size,
        }
    }
}

/// Split a user-typed glob list (`"*.rs, src/**"`) into globs.
pub fn parse_glob_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Escape regex meta characters so `text` matches literally.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if r"\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use grep_matcher::Matcher;

    fn finds(q: &SearchQuery, hay: &str) -> bool {
        q.matcher().unwrap().is_match(hay.as_bytes()).unwrap()
    }

    #[test]
    fn literal_escapes_meta() {
        let q = SearchQuery::new("a.b(c)*");
        assert!(finds(&q, "xx a.b(c)* yy"));
        assert!(!finds(&q, "axb(c)"));
    }

    #[test]
    fn case_modes() {
        let mut q = SearchQuery::new("Foo");
        assert!(finds(&q, "foo"));
        q.case_sensitive = true;
        assert!(!finds(&q, "foo"));
        q.case_sensitive = false;
        q.smart_case = true;
        assert!(!finds(&q, "foo"));
        q.pattern = "foo".into();
        assert!(finds(&q, "FOO"));
    }

    #[test]
    fn whole_word_edges() {
        let mut q = SearchQuery::new("foo");
        q.whole_word = true;
        assert!(finds(&q, "a foo b"));
        assert!(!finds(&q, "foobar"));
        assert!(!finds(&q, "_foo"));
        q.pattern = "-2".into();
        assert!(finds(&q, "x = -2;"));
    }

    #[test]
    fn errors_are_descriptive() {
        assert!(matches!(
            SearchQuery::new("").validate(),
            Err(SearchError::EmptyPattern)
        ));
        let mut q = SearchQuery::new("a(b");
        q.is_regex = true;
        let err = q.validate().unwrap_err();
        assert!(matches!(err, SearchError::InvalidPattern(_)));
        assert!(err.to_string().contains("invalid regular expression"));
        let mut q = SearchQuery::new("x");
        q.include_globs = vec!["a[".into()];
        assert!(matches!(q.validate(), Err(SearchError::InvalidGlob { .. })));
    }

    #[test]
    fn glob_list_parsing() {
        assert_eq!(parse_glob_list(" *.rs, ,src/** "), vec!["*.rs", "src/**"]);
    }
}
