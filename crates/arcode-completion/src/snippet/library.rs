//! Snippet definitions: TOML user snippets and the built-in sets.

use serde::Deserialize;

use super::builtin::BUILTIN;
use crate::candidate::{CandidateKind, CandidateSource, CompletionCandidate, InsertAction};

/// One snippet definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetDef {
    /// Optional display name.
    pub name: Option<String>,
    /// Trigger text matched against the typed word.
    pub prefix: String,
    /// Body in snippet syntax.
    pub body: String,
    /// Shown as detail in the completion list.
    pub description: Option<String>,
    /// Language ids (LSP style, e.g. `rust`); empty = every language.
    pub languages: Vec<String>,
}

impl SnippetDef {
    /// Whether the snippet applies to `language_id` (case-insensitive).
    pub fn matches_language(&self, language_id: &str) -> bool {
        self.languages.is_empty()
            || self
                .languages
                .iter()
                .any(|l| l.eq_ignore_ascii_case(language_id))
    }

    /// The snippet as a completion candidate with `data` as its id.
    pub fn to_candidate(&self, data: u64) -> CompletionCandidate {
        let mut c = CompletionCandidate::new(
            self.prefix.clone(),
            CandidateKind::Snippet,
            CandidateSource::Snippet,
        );
        c.detail = self.description.clone().or_else(|| self.name.clone());
        c.insert = InsertAction::Snippet(self.body.clone());
        c.data = data;
        c
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Body {
    One(String),
    Lines(Vec<String>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSnippet {
    name: Option<String>,
    prefix: String,
    body: Body,
    description: Option<String>,
    #[serde(default)]
    languages: Vec<String>,
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    snippet: Vec<RawSnippet>,
}

/// Parses user snippets:
///
/// ```toml
/// [[snippet]]
/// prefix = "fn"
/// body = "fn ${1:name}(${2}) {\n\t$0\n}"   # or an array of lines
/// description = "function"
/// languages = ["rust"]                    # optional; empty = all
/// ```
pub fn load_snippets(toml_text: &str) -> anyhow::Result<Vec<SnippetDef>> {
    let raw: RawFile = toml::from_str(toml_text)?;
    raw.snippet
        .into_iter()
        .map(|s| {
            if s.prefix.trim().is_empty() {
                anyhow::bail!("snippet with an empty prefix");
            }
            let body = match s.body {
                Body::One(b) => b,
                Body::Lines(lines) => lines.join("\n"),
            };
            Ok(SnippetDef {
                name: s.name,
                prefix: s.prefix,
                body,
                description: s.description,
                languages: s.languages,
            })
        })
        .collect()
}

/// The built-in snippets (rust, python, javascript/typescript, go).
pub fn builtin_snippets() -> Vec<SnippetDef> {
    BUILTIN
        .iter()
        .flat_map(|(languages, defs)| {
            defs.iter().map(move |(prefix, body, description)| SnippetDef {
                name: None,
                prefix: (*prefix).to_string(),
                body: (*body).to_string(),
                description: Some((*description).to_string()),
                languages: languages.iter().map(|l| (*l).to_string()).collect(),
            })
        })
        .collect()
}

/// Built-in snippet candidates for `language_id`; `data` indexes
/// [`builtin_snippets`].
pub fn snippets_for(language_id: &str) -> Vec<CompletionCandidate> {
    SnippetLibrary::builtin().snippets_for(language_id)
}

/// Built-in plus user snippets.
#[derive(Debug, Clone, Default)]
pub struct SnippetLibrary {
    snippets: Vec<SnippetDef>,
}

impl SnippetLibrary {
    /// An empty library.
    pub fn new() -> Self {
        SnippetLibrary::default()
    }

    /// A library holding the built-in snippets.
    pub fn builtin() -> Self {
        SnippetLibrary {
            snippets: builtin_snippets(),
        }
    }

    /// Adds user snippets. They shadow earlier snippets with the same
    /// prefix for the languages they cover.
    pub fn add(&mut self, defs: impl IntoIterator<Item = SnippetDef>) {
        self.snippets.extend(defs);
    }

    /// All snippets in insertion order.
    pub fn all(&self) -> &[SnippetDef] {
        &self.snippets
    }

    /// The snippet a candidate's `data` refers to.
    pub fn get(&self, data: u64) -> Option<&SnippetDef> {
        self.snippets.get(usize::try_from(data).ok()?)
    }

    /// Candidates for `language_id`; later definitions win on equal
    /// prefixes. `data` is the index for [`SnippetLibrary::get`].
    pub fn snippets_for(&self, language_id: &str) -> Vec<CompletionCandidate> {
        let mut out: Vec<CompletionCandidate> = Vec::new();
        for (i, def) in self.snippets.iter().enumerate().rev() {
            if def.matches_language(language_id) && !out.iter().any(|c| c.label == def.prefix) {
                out.push(def.to_candidate(i as u64));
            }
        }
        out.reverse();
        out
    }
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
