//! Plain text: no grammar, no highlighting. The fallback language.

use std::sync::OnceLock;

use crate::Language;

fn no_grammar() -> Option<tree_sitter::Language> {
    None
}

/// Plain text, used for unknown files.
pub(crate) static PLAIN_TEXT: Language = Language {
    name: "Plain Text",
    id: "plaintext",
    extensions: &["txt", "text"],
    file_names: &[],
    line_comment: None,
    block_comment: None,
    ts_language: no_grammar,
    highlights_query: "",
    query: OnceLock::new(),
};
