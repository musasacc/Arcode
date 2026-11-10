//! Which language logo a file gets: one table, read in this order:
//!
//! 1. a `[files] associations` pattern that matches the file: the logo of
//!    the language it names (none for plain text; a name Arcode does not
//!    know is ignored, as it is for highlighting);
//! 2. a file-name rule (`Cargo.toml` is Rust, `package.json` npm,
//!    `Dockerfile.dev` Docker);
//! 3. the built-in language detection of `arcode-syntax` (`.rs`, `.py`,
//!    `Gemfile`, ...);
//! 4. an extension for languages Arcode has no grammar for (`.kt`, `.swift`).
//!
//! Files that match nothing keep the plain icons.

use std::path::Path;

use arcode_config::Config;
use arcode_ui_kit::FileLogo;

/// One logo and what gets it.
pub struct LogoRule {
    /// The logo.
    pub logo: FileLogo,
    /// Language ids (built-in or named by an association).
    pub languages: &'static [&'static str],
    /// File names, lower case; one `*` stands for any run of characters.
    pub names: &'static [&'static str],
    /// Extensions without the dot, lower case, for languages without a
    /// built-in grammar.
    pub extensions: &'static [&'static str],
}

const fn rule(
    logo: FileLogo,
    languages: &'static [&'static str],
    names: &'static [&'static str],
    extensions: &'static [&'static str],
) -> LogoRule {
    LogoRule {
        logo,
        languages,
        names,
        extensions,
    }
}

use FileLogo as L;

/// The table. Names are checked for every rule before languages, and
/// languages before extensions, so a name rule may claim a file that its
/// extension would give another logo (`build.gradle.kts`).
pub const RULES: &[LogoRule] = &[
    rule(
        L::Rust,
        &["rust"],
        &[
            "cargo.toml",
            "cargo.lock",
            "rust-toolchain",
            "rust-toolchain.toml",
            "rustfmt.toml",
            ".rustfmt.toml",
            "clippy.toml",
            ".clippy.toml",
        ],
        &[],
    ),
    rule(
        L::Python,
        &["python"],
        &[
            "pyproject.toml",
            "requirements*.txt",
            "pipfile",
            "pipfile.lock",
            "poetry.lock",
            "uv.lock",
            "setup.cfg",
            ".python-version",
        ],
        &[],
    ),
    rule(L::Javascript, &["javascript"], &[], &[]),
    rule(L::React, &["javascriptreact", "typescriptreact"], &[], &[]),
    rule(
        L::Typescript,
        &["typescript"],
        &["tsconfig.json", "tsconfig.*.json"],
        &[],
    ),
    rule(
        L::Go,
        &["go"],
        &["go.mod", "go.sum", "go.work", "go.work.sum"],
        &[],
    ),
    rule(L::C, &["c"], &[], &[]),
    rule(L::Cplusplus, &["cpp"], &[], &[]),
    rule(L::Java, &["java"], &[], &[]),
    rule(L::Ruby, &["ruby"], &["gemfile.lock", ".ruby-version"], &[]),
    rule(L::Php, &["php"], &["composer.json", "composer.lock"], &[]),
    rule(L::Lua, &["lua"], &[], &["rockspec"]),
    rule(L::Bash, &["shellscript"], &[], &[]),
    rule(L::Html, &["html"], &[], &[]),
    rule(L::Css, &["css"], &[], &[]),
    rule(L::Json, &["json"], &[], &["jsonc", "json5"]),
    rule(L::Yaml, &["yaml"], &[], &[]),
    rule(L::Markdown, &["markdown"], &[], &[]),
    rule(
        L::Npm,
        &[],
        &[
            "package.json",
            "package-lock.json",
            "npm-shrinkwrap.json",
            ".npmrc",
        ],
        &[],
    ),
    rule(L::Nodejs, &[], &[".nvmrc", ".node-version"], &[]),
    rule(
        L::Git,
        &[],
        &[
            ".gitignore",
            ".gitattributes",
            ".gitmodules",
            ".gitkeep",
            ".git-blame-ignore-revs",
        ],
        &[],
    ),
    rule(
        L::Docker,
        &["dockerfile"],
        &[
            "dockerfile",
            "dockerfile.*",
            "containerfile",
            "containerfile.*",
            ".dockerignore",
            "docker-compose*.yml",
            "docker-compose*.yaml",
            "compose.yml",
            "compose.yaml",
        ],
        &["dockerfile"],
    ),
    rule(L::Cmake, &["cmake"], &["cmakelists.txt"], &["cmake"]),
    rule(
        L::Gradle,
        &["gradle"],
        &["*.gradle.kts", "gradle.properties", "gradlew"],
        &["gradle"],
    ),
    rule(L::Kotlin, &["kotlin"], &[], &["kt", "kts"]),
    rule(L::Swift, &["swift"], &[], &["swift"]),
    rule(L::Csharp, &["csharp"], &[], &["cs", "csx"]),
    rule(L::Zig, &["zig"], &[], &["zig", "zon"]),
    rule(
        L::Dart,
        &["dart"],
        &["pubspec.yaml", "pubspec.lock"],
        &["dart"],
    ),
    rule(
        L::Haskell,
        &["haskell"],
        &["stack.yaml"],
        &["hs", "lhs", "cabal"],
    ),
    rule(
        L::Elixir,
        &["elixir"],
        &["mix.lock"],
        &["ex", "exs", "heex", "leex"],
    ),
    rule(L::Erlang, &["erlang"], &["rebar.config"], &["erl", "hrl"]),
    rule(L::Scala, &["scala"], &[], &["scala", "sc", "sbt"]),
    rule(L::Vue, &["vue"], &[], &["vue"]),
    rule(L::Svelte, &["svelte"], &[], &["svelte"]),
    rule(
        L::Graphql,
        &["graphql"],
        &[],
        &["graphql", "gql", "graphqls"],
    ),
    rule(L::Nix, &["nix"], &["flake.lock"], &["nix"]),
    rule(
        L::Terraform,
        &["terraform"],
        &[".terraform.lock.hcl"],
        &["tf", "tfvars"],
    ),
    rule(
        L::Powershell,
        &["powershell"],
        &[],
        &["ps1", "psm1", "psd1"],
    ),
    rule(L::R, &["r"], &[".rprofile"], &["r", "rmd"]),
    rule(L::Sass, &["scss", "sass"], &[], &["scss", "sass"]),
    rule(L::Perl, &["perl"], &[], &["pl", "pm"]),
    rule(L::Ocaml, &["ocaml"], &[], &["ml", "mli"]),
    rule(
        L::Clojure,
        &["clojure"],
        &[],
        &["clj", "cljs", "cljc", "edn"],
    ),
    rule(L::Julia, &["julia"], &[], &["jl"]),
    rule(L::Fsharp, &["fsharp"], &[], &["fs", "fsi", "fsx"]),
    rule(L::Vim, &["vim"], &[".vimrc", "_vimrc", ".gvimrc"], &["vim"]),
    rule(L::Wasm, &[], &[], &["wasm", "wat"]),
];

/// The logo for the file at `path` (a bare file name works too) under
/// `config`'s associations, if any (see the module docs for the order).
pub fn logo_for(path: &Path, config: &Config) -> Option<FileLogo> {
    if let Some(value) = config.files.association_for(path) {
        match crate::languages::resolve::language_named(value) {
            Some(language) => return logo_for_language(language.id),
            // A language Arcode has no grammar for, but a logo (`kotlin`);
            // any other unknown name is ignored, as for highlighting.
            None => {
                if let Some(logo) = logo_for_language(value.trim()) {
                    return Some(logo);
                }
            }
        }
    }
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if let Some(rule) = RULES
        .iter()
        .find(|r| r.names.iter().any(|p| name_matches(p, &name)))
    {
        return Some(rule.logo);
    }
    let language = arcode_syntax::language_for_path(path);
    if language != arcode_syntax::plain_text() {
        return logo_for_language(language.id);
    }
    let (_, ext) = name.rsplit_once('.')?;
    RULES
        .iter()
        .find(|r| r.extensions.contains(&ext))
        .map(|r| r.logo)
}

/// The logo of the language with id `id` (ASCII case ignored).
pub fn logo_for_language(id: &str) -> Option<FileLogo> {
    RULES
        .iter()
        .find(|r| r.languages.iter().any(|l| l.eq_ignore_ascii_case(id)))
        .map(|r| r.logo)
}

/// Whether the lower-case `name` matches `pattern` (one `*` at most).
fn name_matches(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((prefix, suffix)) => {
            name.len() >= prefix.len() + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix)
        }
    }
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
