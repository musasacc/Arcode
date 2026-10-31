//! Plain text: no grammar, no highlighting. The fallback language.

use crate::Language;

/// Plain text, used for unknown files.
pub(crate) static PLAIN_TEXT: Language =
    Language::new("Plain Text", "plaintext").with_extensions(&["txt", "text"]);
