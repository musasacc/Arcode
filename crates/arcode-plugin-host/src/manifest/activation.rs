//! `when` clauses and activation events derived from contributions.

use serde::Deserialize;

use super::Manifest;
use super::ids::{QualifiedId, is_language_id};

/// A command's enablement clause. 0.1 supports exactly `language == <id>`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum When {
    /// Enabled when the focused document has this language.
    Language(String),
}

impl When {
    /// Parses `language == <id>` (whitespace around `==` optional).
    pub fn parse(text: &str) -> Result<Self, String> {
        let unsupported =
            || format!("unsupported `when` clause `{text}`: 0.1 supports only `language == <id>`");
        let (lhs, rhs) = text.split_once("==").ok_or_else(unsupported)?;
        let rhs = rhs.trim();
        if lhs.trim() != "language" || !is_language_id(rhs) {
            return Err(unsupported());
        }
        Ok(Self::Language(rhs.to_owned()))
    }

    /// Evaluates the clause for the focused document's language.
    pub fn matches(&self, language: Option<&str>) -> bool {
        match self {
            Self::Language(id) => language == Some(id.as_str()),
        }
    }
}

impl TryFrom<String> for When {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

/// Something that creates the plugin instance (§3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ActivationEvent {
    /// A contributed command runs.
    Command(QualifiedId),
    /// A document of a lens language becomes visible.
    LensLanguage(String),
    /// An opener pattern matches a file being opened.
    Opener(QualifiedId),
    /// A contributed panel is shown.
    Panel(QualifiedId),
}

pub(super) fn events(manifest: &Manifest) -> Vec<ActivationEvent> {
    let id = manifest.id();
    let c = &manifest.contributes;
    let mut events: Vec<ActivationEvent> = c
        .commands
        .iter()
        .map(|cmd| ActivationEvent::Command(id.qualify(&cmd.id)))
        .chain(c.code_lenses.iter().flat_map(|l| {
            l.languages
                .iter()
                .cloned()
                .map(ActivationEvent::LensLanguage)
        }))
        .chain(
            c.openers
                .iter()
                .map(|o| ActivationEvent::Opener(id.qualify(&o.id))),
        )
        .chain(
            c.panels
                .iter()
                .map(|p| ActivationEvent::Panel(id.qualify(&p.id))),
        )
        .collect();
    events.sort();
    events.dedup();
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn when_accepts_only_language_equality() {
        assert_eq!(
            When::parse("language == http"),
            Ok(When::Language("http".into()))
        );
        assert_eq!(
            When::parse("language==http"),
            Ok(When::Language("http".into()))
        );
        for bad in [
            "language != http",
            "editorFocus",
            "language == http && x",
            "lang == http",
            "language == ",
        ] {
            assert!(When::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn when_matches_focused_language() {
        let w = When::Language("http".into());
        assert!(w.matches(Some("http")));
        assert!(!w.matches(Some("rust")) && !w.matches(None));
    }
}
