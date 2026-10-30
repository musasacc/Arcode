//! Compatibility: the grammar crates' own query files compile against our
//! tree-sitter version (proves the grammar ABI matches). Arcode ships its
//! own queries (`queries/`), adapted to its capture names and precedence.

use tree_sitter::{Language, Query};

fn compiles(name: &str, language: Language, sources: &[&str]) {
    let source = sources.join("\n");
    if let Err(err) = Query::new(&language, &source) {
        panic!("{name}: {err}");
    }
}

#[test]
fn upstream_highlight_queries_compile() {
    use tree_sitter_javascript as js;
    compiles(
        "python",
        tree_sitter_python::LANGUAGE.into(),
        &[tree_sitter_python::HIGHLIGHTS_QUERY],
    );
    compiles(
        "javascript",
        js::LANGUAGE.into(),
        &[js::HIGHLIGHT_QUERY, js::JSX_HIGHLIGHT_QUERY],
    );
    compiles(
        "typescript",
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        &[
            js::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
        ],
    );
    compiles(
        "tsx",
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        &[
            js::HIGHLIGHT_QUERY,
            js::JSX_HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
        ],
    );
    compiles(
        "go",
        tree_sitter_go::LANGUAGE.into(),
        &[tree_sitter_go::HIGHLIGHTS_QUERY],
    );
    compiles(
        "c",
        tree_sitter_c::LANGUAGE.into(),
        &[tree_sitter_c::HIGHLIGHT_QUERY],
    );
    compiles(
        "cpp",
        tree_sitter_cpp::LANGUAGE.into(),
        &[
            tree_sitter_c::HIGHLIGHT_QUERY,
            tree_sitter_cpp::HIGHLIGHT_QUERY,
        ],
    );
    compiles(
        "bash",
        tree_sitter_bash::LANGUAGE.into(),
        &[tree_sitter_bash::HIGHLIGHT_QUERY],
    );
    compiles(
        "html",
        tree_sitter_html::LANGUAGE.into(),
        &[tree_sitter_html::HIGHLIGHTS_QUERY],
    );
    compiles(
        "css",
        tree_sitter_css::LANGUAGE.into(),
        &[tree_sitter_css::HIGHLIGHTS_QUERY],
    );
    compiles(
        "yaml",
        tree_sitter_yaml::LANGUAGE.into(),
        &[tree_sitter_yaml::HIGHLIGHTS_QUERY],
    );
    compiles(
        "markdown",
        tree_sitter_md::LANGUAGE.into(),
        &[tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
    );
    compiles(
        "markdown-inline",
        tree_sitter_md::INLINE_LANGUAGE.into(),
        &[tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
    );
    compiles(
        "java",
        tree_sitter_java::LANGUAGE.into(),
        &[tree_sitter_java::HIGHLIGHTS_QUERY],
    );
    compiles(
        "lua",
        tree_sitter_lua::LANGUAGE.into(),
        &[tree_sitter_lua::HIGHLIGHTS_QUERY],
    );
    compiles(
        "ruby",
        tree_sitter_ruby::LANGUAGE.into(),
        &[tree_sitter_ruby::HIGHLIGHTS_QUERY],
    );
    compiles(
        "php",
        tree_sitter_php::LANGUAGE_PHP.into(),
        &[tree_sitter_php::HIGHLIGHTS_QUERY],
    );
}

#[test]
fn upstream_tags_queries_compile() {
    compiles(
        "python",
        tree_sitter_python::LANGUAGE.into(),
        &[tree_sitter_python::TAGS_QUERY],
    );
    compiles(
        "javascript",
        tree_sitter_javascript::LANGUAGE.into(),
        &[tree_sitter_javascript::TAGS_QUERY],
    );
    compiles(
        "typescript",
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        &[tree_sitter_typescript::TAGS_QUERY],
    );
    compiles(
        "go",
        tree_sitter_go::LANGUAGE.into(),
        &[tree_sitter_go::TAGS_QUERY],
    );
    compiles(
        "c",
        tree_sitter_c::LANGUAGE.into(),
        &[tree_sitter_c::TAGS_QUERY],
    );
    compiles(
        "cpp",
        tree_sitter_cpp::LANGUAGE.into(),
        &[tree_sitter_cpp::TAGS_QUERY],
    );
    compiles(
        "java",
        tree_sitter_java::LANGUAGE.into(),
        &[tree_sitter_java::TAGS_QUERY],
    );
}
