//! Snippets: parsing, variable resolution, expansion, sessions, libraries.

mod ast;
mod builtin;
mod expand;
mod library;
mod parse;
mod session;
mod transform;
mod variables;

pub use ast::{CaseOp, FormatItem, Node, Snippet, Transform};
pub use expand::{ExpandedSnippet, Tabstop, expand};
pub use library::{SnippetDef, SnippetLibrary, builtin_snippets, load_snippets, snippets_for};
pub use session::SnippetSession;
pub use transform::change_case;
pub use variables::{DateTime, KNOWN_VARIABLES, VariableContext, uuid_v4};
