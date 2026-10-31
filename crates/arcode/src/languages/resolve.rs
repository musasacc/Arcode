//! Which language a file gets: a `[files] associations` pattern first, then
//! the built-in extensions and file names of `arcode-syntax`.

use std::path::Path;

use arcode_config::Config;
use arcode_syntax::Language;

/// The language named `value`: an id (`typescriptreact`) or a display name
/// (`TypeScript React`), ignoring case. A few common aliases are accepted
/// (`sh`, `bash`, `js`, `ts`, `md`, `yml`, ...).
pub fn language_named(value: &str) -> Option<&'static Language> {
    let value = value.trim();
    let alias = match value.to_ascii_lowercase().as_str() {
        "sh" | "bash" | "shell" | "zsh" => "shellscript",
        "js" => "javascript",
        "jsx" => "javascriptreact",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "md" => "markdown",
        "yml" => "yaml",
        "py" => "python",
        "rs" => "rust",
        "rb" => "ruby",
        "c++" => "cpp",
        "text" | "txt" | "plain" => "plaintext",
        _ => value,
    };
    arcode_syntax::language_by_id(alias).or_else(|| {
        arcode_syntax::all_languages()
            .iter()
            .copied()
            .find(|l| l.name.eq_ignore_ascii_case(value))
    })
}

/// The language for `path` under `config`'s file associations. An
/// association naming an unknown language is ignored.
pub fn language_for(path: &Path, config: &Config) -> &'static Language {
    config
        .files
        .association_for(path)
        .and_then(language_named)
        .unwrap_or_else(|| arcode_syntax::language_for_path(path))
}

/// Associations whose language is unknown, as `pattern = value` text, for
/// a warning.
pub fn unknown_associations(config: &Config) -> Vec<String> {
    config
        .files
        .associations
        .iter()
        .filter(|(_, language)| language_named(language).is_none())
        .map(|(pattern, language)| format!("{pattern} = {language}"))
        .collect()
}

/// The association pattern offered for "always use this language" on
/// `path`: `*.ext` when it has an extension, else the file name.
pub fn association_pattern(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if !ext.is_empty() && name != format!(".{ext}") => Some(format!("*.{ext}")),
        _ => Some(name.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(pairs: &[(&str, &str)]) -> Config {
        let mut config = Config::default();
        for (pattern, language) in pairs {
            config
                .files
                .associations
                .insert((*pattern).into(), (*language).into());
        }
        config
    }

    #[test]
    fn names_ids_and_aliases() {
        assert_eq!(language_named("rust").map(|l| l.id), Some("rust"));
        assert_eq!(
            language_named("TypeScript React").map(|l| l.id),
            Some("typescriptreact")
        );
        assert_eq!(language_named("bash").map(|l| l.id), Some("shellscript"));
        assert_eq!(language_named("yml").map(|l| l.id), Some("yaml"));
        assert!(language_named("cobol").is_none());
    }

    #[test]
    fn associations_win_over_extensions() {
        let config = config(&[("*.mdx", "markdown"), ("*.h", "cpp"), ("*.zz", "cobol")]);
        assert_eq!(language_for(Path::new("/p/a.mdx"), &config).id, "markdown");
        assert_eq!(language_for(Path::new("/p/a.h"), &config).id, "cpp");
        assert_eq!(language_for(Path::new("/p/a.c"), &config).id, "c");
        assert_eq!(language_for(Path::new("/p/a.zz"), &config).id, "plaintext");
        assert_eq!(unknown_associations(&config), ["*.zz = cobol"]);
    }

    #[test]
    fn association_patterns() {
        assert_eq!(
            association_pattern(Path::new("/p/a.tpl")).as_deref(),
            Some("*.tpl")
        );
        assert_eq!(
            association_pattern(Path::new("/p/Jenkinsfile")).as_deref(),
            Some("Jenkinsfile")
        );
        assert_eq!(
            association_pattern(Path::new("/p/.envrc")).as_deref(),
            Some(".envrc")
        );
    }
}
