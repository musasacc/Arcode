use std::path::Path;

use arcode_config::Config;
use arcode_ui_kit::FileLogo;

use super::*;

fn logo(name: &str) -> Option<FileLogo> {
    logo_for(Path::new(name), &Config::default())
}

fn with_associations(pairs: &[(&str, &str)]) -> Config {
    let mut config = Config::default();
    for (pattern, language) in pairs {
        config
            .files
            .associations
            .insert((*pattern).into(), (*language).into());
    }
    config
}

/// docs/guide/languages.md: every built-in language has its logo, except
/// TOML (Devicon has none) and plain text.
#[test]
fn every_built_in_language_has_a_logo() {
    for language in arcode_syntax::all_languages() {
        let found = logo_for_language(language.id);
        match language.id {
            "toml" | "plaintext" => assert_eq!(found, None, "{}", language.id),
            _ => assert!(found.is_some(), "{} has no logo", language.id),
        }
    }
    assert_eq!(logo_for_language("javascriptreact"), Some(FileLogo::React));
    assert_eq!(logo_for_language("TypeScriptReact"), Some(FileLogo::React));
    assert_eq!(logo_for_language("cobol"), None);
}

#[test]
fn files_get_the_logo_of_their_language() {
    let cases: &[(&str, Option<FileLogo>)] = &[
        ("main.rs", Some(FileLogo::Rust)),
        ("app.py", Some(FileLogo::Python)),
        ("stubs.pyi", Some(FileLogo::Python)),
        ("SConstruct", Some(FileLogo::Python)),
        ("index.js", Some(FileLogo::Javascript)),
        ("tool.mjs", Some(FileLogo::Javascript)),
        ("App.jsx", Some(FileLogo::React)),
        ("App.TSX", Some(FileLogo::React)),
        ("lib.ts", Some(FileLogo::Typescript)),
        ("main.go", Some(FileLogo::Go)),
        ("lib.c", Some(FileLogo::C)),
        ("lib.h", Some(FileLogo::C)),
        ("vec.cpp", Some(FileLogo::Cplusplus)),
        ("vec.hpp", Some(FileLogo::Cplusplus)),
        ("Main.java", Some(FileLogo::Java)),
        ("app.rb", Some(FileLogo::Ruby)),
        ("Gemfile", Some(FileLogo::Ruby)),
        ("index.php", Some(FileLogo::Php)),
        ("init.lua", Some(FileLogo::Lua)),
        ("run.sh", Some(FileLogo::Bash)),
        (".bashrc", Some(FileLogo::Bash)),
        ("PKGBUILD", Some(FileLogo::Bash)),
        ("index.html", Some(FileLogo::Html)),
        ("site.css", Some(FileLogo::Css)),
        ("data.json", Some(FileLogo::Json)),
        ("ci.yml", Some(FileLogo::Yaml)),
        ("README.md", Some(FileLogo::Markdown)),
        ("Build.kt", Some(FileLogo::Kotlin)),
        ("App.swift", Some(FileLogo::Swift)),
        ("Program.cs", Some(FileLogo::Csharp)),
        ("build.zig", Some(FileLogo::Zig)),
        ("main.dart", Some(FileLogo::Dart)),
        ("Main.hs", Some(FileLogo::Haskell)),
        ("mix.exs", Some(FileLogo::Elixir)),
        ("server.erl", Some(FileLogo::Erlang)),
        ("App.scala", Some(FileLogo::Scala)),
        ("App.vue", Some(FileLogo::Vue)),
        ("App.svelte", Some(FileLogo::Svelte)),
        ("schema.graphql", Some(FileLogo::Graphql)),
        ("default.nix", Some(FileLogo::Nix)),
        ("main.tf", Some(FileLogo::Terraform)),
        ("build.ps1", Some(FileLogo::Powershell)),
        ("plot.R", Some(FileLogo::R)),
        ("site.scss", Some(FileLogo::Sass)),
        ("tool.pl", Some(FileLogo::Perl)),
        ("main.ml", Some(FileLogo::Ocaml)),
        ("core.clj", Some(FileLogo::Clojure)),
        ("fit.jl", Some(FileLogo::Julia)),
        ("Lib.fs", Some(FileLogo::Fsharp)),
        ("plugin.vim", Some(FileLogo::Vim)),
        ("module.wat", Some(FileLogo::Wasm)),
        // File-name rules.
        ("Cargo.toml", Some(FileLogo::Rust)),
        ("Cargo.lock", Some(FileLogo::Rust)),
        ("rust-toolchain.toml", Some(FileLogo::Rust)),
        ("package.json", Some(FileLogo::Npm)),
        ("package-lock.json", Some(FileLogo::Npm)),
        (".nvmrc", Some(FileLogo::Nodejs)),
        (".gitignore", Some(FileLogo::Git)),
        (".gitattributes", Some(FileLogo::Git)),
        ("requirements.txt", Some(FileLogo::Python)),
        ("requirements-dev.txt", Some(FileLogo::Python)),
        ("pyproject.toml", Some(FileLogo::Python)),
        ("go.mod", Some(FileLogo::Go)),
        ("Dockerfile", Some(FileLogo::Docker)),
        ("Dockerfile.dev", Some(FileLogo::Docker)),
        ("web.dockerfile", Some(FileLogo::Docker)),
        ("docker-compose.yml", Some(FileLogo::Docker)),
        ("CMakeLists.txt", Some(FileLogo::Cmake)),
        ("toolchain.cmake", Some(FileLogo::Cmake)),
        ("build.gradle", Some(FileLogo::Gradle)),
        ("build.gradle.kts", Some(FileLogo::Gradle)),
        ("tsconfig.json", Some(FileLogo::Typescript)),
        ("tsconfig.build.json", Some(FileLogo::Typescript)),
        ("flake.lock", Some(FileLogo::Nix)),
        ("composer.json", Some(FileLogo::Php)),
        // No logo: the plain icons.
        ("rustfmt.txt", None),
        ("config.toml", None),
        ("Makefile", None),
        ("notes.txt", None),
        ("query.sql", None),
        ("logo.png", None),
        ("LICENSE", None),
        ("", None),
    ];
    for (name, expected) in cases {
        assert_eq!(logo(name), *expected, "{name}");
    }
}

#[test]
fn full_paths_work_like_names() {
    assert_eq!(logo("/home/me/demo/src/main.rs"), Some(FileLogo::Rust));
    assert_eq!(logo("C:/work/app/Cargo.toml"), Some(FileLogo::Rust));
    assert_eq!(logo("/home/me/demo/src"), None);
}

/// `[files] associations` come first, like for highlighting.
#[test]
fn associations_pick_the_logo() {
    let config = with_associations(&[
        ("*.mdx", "markdown"),
        ("Jenkinsfile", "sh"),
        ("*.rs", "plaintext"),
        ("*.kts", "kotlin"),
        ("*.weird", "cobol"),
        ("**/templates/*.txt", "html"),
    ]);
    let logo = |p: &str| logo_for(Path::new(p), &config);
    assert_eq!(logo("docs/page.mdx"), Some(FileLogo::Markdown));
    assert_eq!(logo("Jenkinsfile"), Some(FileLogo::Bash));
    assert_eq!(logo("main.rs"), None, "plain text has no logo");
    assert_eq!(logo("build.gradle.kts"), Some(FileLogo::Kotlin));
    assert_eq!(logo("data.weird"), None);
    assert_eq!(logo("/p/templates/page.txt"), Some(FileLogo::Html));
    assert_eq!(logo("/p/other/page.txt"), None);
    assert_eq!(logo("lib.py"), Some(FileLogo::Python), "others unchanged");
}

/// Extension rules are only for languages without a grammar: none of them
/// takes a file from the built-in detection.
#[test]
fn extension_rules_never_shadow_built_in_languages() {
    for rule in RULES {
        for ext in rule.extensions {
            let path = format!("x.{ext}");
            assert_eq!(
                arcode_syntax::language_for_path(Path::new(&path)),
                arcode_syntax::plain_text(),
                ".{ext} is a built-in language"
            );
        }
    }
}

#[test]
fn the_table_is_well_formed() {
    for logo in FileLogo::ALL {
        let rules = RULES.iter().filter(|r| r.logo == *logo).count();
        assert_eq!(rules, 1, "{} has {rules} rules", logo.name());
    }
    for rule in RULES {
        for name in rule.names {
            assert_eq!(*name, name.to_ascii_lowercase(), "{name}");
            assert!(name.matches('*').count() <= 1, "{name}");
        }
        for ext in rule.extensions {
            assert_eq!(*ext, ext.to_ascii_lowercase(), "{ext}");
            assert!(!ext.contains('.'), "{ext}");
        }
    }
}

#[test]
fn name_patterns() {
    assert!(name_matches("cargo.toml", "cargo.toml"));
    assert!(!name_matches("cargo.toml", "cargo.toml.bak"));
    assert!(name_matches("dockerfile.*", "dockerfile.dev"));
    assert!(!name_matches("dockerfile.*", "dockerfile"));
    assert!(name_matches("requirements*.txt", "requirements.txt"));
    assert!(!name_matches("requirements*.txt", "requirements.tx"));
    assert!(name_matches("*.gradle.kts", "build.gradle.kts"));
    assert!(!name_matches("*.gradle.kts", "gradle.kts.x"));
}
