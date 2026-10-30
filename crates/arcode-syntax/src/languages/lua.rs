//! Lua, via `tree-sitter-lua`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_lua::LANGUAGE.into())
}

/// The Lua language.
pub(crate) static LUA: Language = Language::new("Lua", "lua")
    .with_extensions(&["lua"])
    .with_file_names(&[".luacheckrc"])
    .with_line_comment("--")
    .with_block_comment("--[[", "]]")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/lua/highlights.scm"));
