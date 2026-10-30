//! Built-in language definitions, one file per language (or language family).
//!
//! To add a language:
//! 1. add the grammar crate to this crate's `Cargo.toml`,
//! 2. put its query in `queries/<id>/highlights.scm` (names from
//!    [`crate::HIGHLIGHT_NAMES`], later patterns win), and optionally
//!    `runnables.scm` / `outline.scm` (see `src/runnables.rs` and
//!    `src/outline.rs` for the capture conventions),
//! 3. create `src/languages/<id>.rs` with a `pub(crate) static` [`Language`](crate::Language)
//!    built with `Language::new(..).with_*(..)`,
//! 4. list it in [`BUILTIN`] below.

mod bash;
mod c;
mod cpp;
mod css;
mod go;
mod html;
mod java;
mod javascript;
mod json;
mod lua;
mod markdown;
mod php;
mod plain_text;
mod python;
mod ruby;
mod rust;
mod toml;
mod typescript;
mod yaml;

use crate::Language;

pub(crate) use plain_text::PLAIN_TEXT;

/// All built-in languages. Plain text must stay last: it is the fallback.
pub(crate) static BUILTIN: &[&Language] = &[
    &rust::RUST,
    &toml::TOML,
    &json::JSON,
    &python::PYTHON,
    &javascript::JAVASCRIPT,
    &javascript::JAVASCRIPT_REACT,
    &typescript::TYPESCRIPT,
    &typescript::TSX,
    &go::GO,
    &c::C,
    &cpp::CPP,
    &bash::BASH,
    &html::HTML,
    &css::CSS,
    &yaml::YAML,
    &markdown::MARKDOWN,
    &java::JAVA,
    &lua::LUA,
    &ruby::RUBY,
    &php::PHP,
    &PLAIN_TEXT,
];
