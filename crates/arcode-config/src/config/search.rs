//! The `[search]` and `[todo]` sections of `config.toml`.

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// `[search]`: project-wide search (the Search view) and the find bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    /// Stop a project search after this many matches ("show more" raises
    /// the limit for the current query).
    pub max_results: usize,
    /// Milliseconds after the last keystroke before a search starts.
    pub debounce_ms: u64,
    /// Skip files and folders listed in `.gitignore` / `.ignore`.
    pub respect_gitignore: bool,
    /// Also search hidden files and folders (names starting with a dot).
    pub include_hidden: bool,
    /// Globs always excluded from project search (e.g. `"*.min.js"`), on
    /// top of the exclude field of the Search view.
    pub exclude: Vec<String>,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            max_results: 2000,
            debounce_ms: 250,
            respect_gitignore: true,
            include_hidden: false,
            exclude: Vec::new(),
        }
    }
}

impl_section!(SearchSettings {
    max_results,
    debounce_ms,
    respect_gitignore,
    include_hidden,
    exclude,
});

impl SearchSettings {
    /// Smallest accepted result limit.
    pub const MIN_RESULTS: usize = 50;
    /// Largest accepted result limit.
    pub const MAX_RESULTS: usize = 100_000;
    /// Longest accepted debounce.
    pub const MAX_DEBOUNCE_MS: u64 = 2000;

    pub(crate) fn validate(&mut self, warnings: &mut Vec<String>) {
        let clamped = self.max_results.clamp(Self::MIN_RESULTS, Self::MAX_RESULTS);
        if clamped != self.max_results {
            warnings.push(format!(
                "`search.max_results` must be between {} and {}",
                Self::MIN_RESULTS,
                Self::MAX_RESULTS
            ));
            self.max_results = clamped;
        }
        if self.debounce_ms > Self::MAX_DEBOUNCE_MS {
            warnings.push(format!(
                "`search.debounce_ms` capped at {}",
                Self::MAX_DEBOUNCE_MS
            ));
            self.debounce_ms = Self::MAX_DEBOUNCE_MS;
        }
    }
}

/// `[todo]`: the TODO list (activity bar and bottom panel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoSettings {
    /// Comment tags to list, matched case-sensitively as whole words.
    pub tags: Vec<String>,
}

impl Default for TodoSettings {
    fn default() -> Self {
        Self {
            tags: ["TODO", "FIXME", "HACK", "XXX", "BUG", "NOTE"]
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }
}

impl_section!(TodoSettings { tags });

impl TodoSettings {
    pub(crate) fn validate(&mut self, warnings: &mut Vec<String>) {
        let before = self.tags.len();
        self.tags = self
            .tags
            .iter()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .collect();
        if self.tags.len() != before {
            warnings.push("empty entries in `todo.tags` ignored".to_owned());
        }
        if self.tags.is_empty() {
            warnings.push("`todo.tags` is empty; using the default tags".to_owned());
            self.tags = Self::default().tags;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn search_and_todo_sections_parse_and_validate() {
        let (config, warnings) = Config::from_toml_str(
            "[search]\nmax_results = 5\ndebounce_ms = 100\nexclude = [\"*.min.js\"]\n\
             [todo]\ntags = [\"TODO\", \" \", \"PERF\"]\n",
        );
        assert_eq!(config.search.max_results, 50);
        assert_eq!(config.search.debounce_ms, 100);
        assert_eq!(config.search.exclude, ["*.min.js"]);
        assert_eq!(config.todo.tags, ["TODO", "PERF"]);
        assert_eq!(warnings.len(), 2, "{warnings:?}");

        let (config, warnings) = Config::from_toml_str("[todo]\ntags = []\n");
        assert_eq!(config.todo.tags.len(), 6);
        assert_eq!(warnings.len(), 1);
        assert_eq!(Config::default().search.max_results, 2000);
    }
}
