//! Built-in language definitions, one file per language.
//!
//! To add a language:
//! 1. add the grammar crate to this crate's `Cargo.toml`,
//! 2. put its query in `queries/<id>/highlights.scm` (names from
//!    [`crate::HIGHLIGHT_NAMES`], later patterns win),
//! 3. create `src/languages/<id>.rs` with a `pub(crate) static` [`Language`](crate::Language),
//! 4. list it in [`BUILTIN`] below.

mod json;
mod plain_text;
mod rust;
mod toml;

use crate::Language;

pub(crate) use plain_text::PLAIN_TEXT;

/// All built-in languages. Plain text must stay last: it is the fallback.
pub(crate) static BUILTIN: &[&Language] = &[&rust::RUST, &toml::TOML, &json::JSON, &PLAIN_TEXT];
