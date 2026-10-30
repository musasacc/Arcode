//! Language registry lookups.

use std::path::Path;

use arcode_syntax::{Syntax, all_languages, language_by_id, language_for_path, plain_text};
use ropey::Rope;

fn id_for(path: &str) -> &'static str {
    language_for_path(Path::new(path)).id
}

#[test]
fn language_for_path_detects_builtins() {
    assert_eq!(id_for("src/main.rs"), "rust");
    assert_eq!(id_for("Cargo.toml"), "toml");
    assert_eq!(id_for("Cargo.lock"), "toml");
    assert_eq!(id_for("some/dir/Cargo.lock"), "toml");
    assert_eq!(id_for("package.json"), "json");
    assert_eq!(id_for("DATA.JSON"), "json");
    assert_eq!(id_for("notes.txt"), "plaintext");
}

#[test]
fn unknown_paths_fall_back_to_plain_text() {
    for path in [
        "foo.unknownext",
        "Makefile",
        "",
        "dir/",
        ".hidden",
        "other.lock",
    ] {
        assert_eq!(language_for_path(Path::new(path)), plain_text(), "{path}");
    }
}

#[test]
fn language_by_id_finds_each_language() {
    for lang in all_languages() {
        assert_eq!(language_by_id(lang.id), Some(*lang));
    }
    assert!(language_by_id("python").is_none());
}

#[test]
fn builtin_metadata() {
    let names: Vec<_> = all_languages().iter().map(|l| l.name).collect();
    assert_eq!(names, ["Rust", "TOML", "JSON", "Plain Text"]);

    let rust = language_by_id("rust").expect("rust");
    assert_eq!(rust.line_comment, Some("//"));
    assert_eq!(rust.block_comment, Some(("/*", "*/")));
    assert!(rust.extensions.contains(&"rs"));

    let toml = language_by_id("toml").expect("toml");
    assert_eq!(toml.line_comment, Some("#"));
    assert!(toml.file_names.contains(&"Cargo.lock"));

    let json = language_by_id("json").expect("json");
    assert_eq!(json.line_comment, None);
}

#[test]
fn grammars_load_and_queries_compile() {
    for lang in all_languages() {
        let syntax = Syntax::new(lang, &Rope::from_str("x"));
        assert_eq!(syntax.is_some(), lang.has_grammar(), "{}", lang.name);
        assert_eq!(
            lang.highlight_query().is_some(),
            lang.has_grammar(),
            "{}",
            lang.name
        );
    }
}

#[test]
fn plain_text_highlights_nothing() {
    assert!(Syntax::new(plain_text(), &Rope::from_str("fn main() {}")).is_none());
    assert!(!plain_text().has_grammar());
}
