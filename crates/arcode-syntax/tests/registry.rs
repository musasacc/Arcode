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
    assert!(language_by_id("cobol").is_none());
}

#[test]
fn builtin_metadata() {
    let names: Vec<_> = all_languages().iter().map(|l| l.name).collect();
    assert_eq!(&names[..3], ["Rust", "TOML", "JSON"]);
    assert_eq!(names.last(), Some(&"Plain Text"));

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

#[test]
fn language_for_path_detects_new_languages() {
    let cases = [
        ("app.py", "python"),
        ("stubs.pyi", "python"),
        ("SConstruct", "python"),
        ("index.js", "javascript"),
        ("lib.mjs", "javascript"),
        ("conf.cjs", "javascript"),
        ("App.jsx", "javascriptreact"),
        ("main.ts", "typescript"),
        ("lib.mts", "typescript"),
        ("App.tsx", "typescriptreact"),
        ("main.go", "go"),
        ("main.c", "c"),
        ("main.h", "c"),
        ("main.cpp", "cpp"),
        ("main.cc", "cpp"),
        ("main.cxx", "cpp"),
        ("vec.hpp", "cpp"),
        ("vec.hh", "cpp"),
        ("build.sh", "shellscript"),
        ("run.bash", "shellscript"),
        (".zshrc", "shellscript"),
        ("home/.bashrc", "shellscript"),
        ("PKGBUILD", "shellscript"),
        ("index.html", "html"),
        ("old.htm", "html"),
        ("site.css", "css"),
        ("ci.yml", "yaml"),
        ("config.yaml", "yaml"),
        (".clang-format", "yaml"),
        ("README.md", "markdown"),
        ("notes.markdown", "markdown"),
        ("App.java", "java"),
        ("init.lua", "lua"),
        ("app.rb", "ruby"),
        ("Gemfile", "ruby"),
        ("Rakefile", "ruby"),
        ("x.gemspec", "ruby"),
        ("index.php", "php"),
        ("README.MD", "markdown"),
        ("Main.JAVA", "java"),
    ];
    for (path, id) in cases {
        assert_eq!(id_for(path), id, "{path}");
    }
}

#[test]
fn build_and_dot_files_stay_plain_text() {
    for path in [
        "Makefile",
        "Dockerfile",
        ".gitignore",
        ".env",
        "LICENSE",
        "x.scss",
    ] {
        assert_eq!(id_for(path), "plaintext", "{path}");
    }
}

#[test]
fn extensions_and_file_names_are_unambiguous() {
    let langs = all_languages();
    for (i, a) in langs.iter().enumerate() {
        for b in &langs[i + 1..] {
            for ext in a.extensions {
                assert!(
                    !b.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)),
                    ".{ext} in {} and {}",
                    a.name,
                    b.name
                );
            }
            for name in a.file_names {
                assert!(
                    !b.file_names.contains(name),
                    "{name} in {} and {}",
                    a.name,
                    b.name
                );
            }
        }
    }
}

#[test]
fn comment_tokens_per_language() {
    let line = |id: &str| language_by_id(id).and_then(|l| l.line_comment);
    let block = |id: &str| language_by_id(id).and_then(|l| l.block_comment);
    for id in [
        "javascript",
        "javascriptreact",
        "typescript",
        "typescriptreact",
        "go",
        "c",
        "cpp",
        "java",
        "php",
    ] {
        assert_eq!(line(id), Some("//"), "{id}");
        assert_eq!(block(id), Some(("/*", "*/")), "{id}");
    }
    for id in ["python", "shellscript", "yaml", "ruby"] {
        assert_eq!(line(id), Some("#"), "{id}");
    }
    assert_eq!(block("python"), None);
    assert_eq!(block("ruby"), Some(("=begin", "=end")));
    assert_eq!(line("lua"), Some("--"));
    assert_eq!(block("lua"), Some(("--[[", "]]")));
    for id in ["html", "markdown"] {
        assert_eq!(line(id), None, "{id}");
        assert_eq!(block(id), Some(("<!--", "-->")), "{id}");
    }
    assert_eq!(line("css"), None);
    assert_eq!(block("css"), Some(("/*", "*/")));
}

#[test]
fn lsp_ids_follow_the_spec() {
    for id in [
        "rust",
        "toml",
        "json",
        "python",
        "javascript",
        "javascriptreact",
        "typescript",
        "typescriptreact",
        "go",
        "c",
        "cpp",
        "shellscript",
        "html",
        "css",
        "yaml",
        "markdown",
        "java",
        "lua",
        "ruby",
        "php",
        "plaintext",
    ] {
        assert!(language_by_id(id).is_some(), "{id}");
    }
    assert_eq!(all_languages().len(), 21);
}

#[test]
fn every_grammar_language_has_a_highlight_query() {
    for lang in all_languages().iter().filter(|l| l.has_grammar()) {
        if let Err(err) = lang.compile_highlights_query() {
            panic!("{err}");
        }
        if !lang.runnables_query.is_empty() {
            assert!(lang.runnables_query().is_some(), "{}", lang.name);
        }
        if !lang.outline_query.is_empty() {
            assert!(lang.outline_query().is_some(), "{}", lang.name);
        }
    }
    assert!(plain_text().runnables_query().is_none());
    assert!(plain_text().outline_query().is_none());
}
