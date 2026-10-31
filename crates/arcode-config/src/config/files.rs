//! The `[files]` section of `config.toml`: file associations.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// `[files]`: how files are mapped to languages.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesSettings {
    /// File pattern → language id (or language name), e.g.
    /// `{ "*.mdx" = "markdown", "Jenkinsfile" = "groovy" }`.
    ///
    /// A pattern without `/` is matched against the file name; one with a
    /// `/` against the whole path (with `/` separators, so `"**/.config/*.conf"`
    /// works). `*` matches any run of characters (also `/` in path
    /// patterns), `?` one character; matching ignores ASCII case. When
    /// several patterns match, the most specific (the one with the most
    /// literal characters) wins.
    pub associations: BTreeMap<String, String>,
}

impl_section!(FilesSettings { associations });

impl FilesSettings {
    /// The language the user associated with `path`, if any pattern
    /// matches (the value as written: an id or a name).
    pub fn association_for(&self, path: &Path) -> Option<&str> {
        let name = path.file_name()?.to_string_lossy();
        let full = path.to_string_lossy().replace('\\', "/");
        self.associations
            .iter()
            .filter(|(pattern, _)| {
                if pattern.contains('/') {
                    path_pattern_matches(pattern, &full)
                } else {
                    wildcard_match(pattern, &name)
                }
            })
            .max_by_key(|(pattern, _)| specificity(pattern))
            .map(|(_, language)| language.as_str())
    }

    pub(crate) fn validate(&mut self, warnings: &mut Vec<String>) {
        self.associations.retain(|pattern, language| {
            let ok = !pattern.trim().is_empty() && !language.trim().is_empty();
            if !ok {
                warnings.push(format!(
                    "`files.associations` entry `{pattern}` = `{language}` is empty (ignored)"
                ));
            }
            ok
        });
    }
}

/// Literal characters in a pattern: more means more specific.
fn specificity(pattern: &str) -> usize {
    pattern.chars().filter(|c| !matches!(c, '*' | '?')).count()
}

/// A path pattern matches the whole path or any tail of it that starts
/// after a `/` (so `src/*.inc` matches `/home/me/p/src/a.inc`).
fn path_pattern_matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.strip_prefix("**/").unwrap_or(pattern);
    wildcard_match(pattern, path)
        || path
            .match_indices('/')
            .any(|(ix, _)| wildcard_match(pattern, &path[ix + 1..]))
}

/// `*` (any run, `**` is the same) and `?` (one char), ASCII case-insensitive.
pub(crate) fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi].eq_ignore_ascii_case(&t[ti])) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    fn settings(pairs: &[(&str, &str)]) -> FilesSettings {
        FilesSettings {
            associations: pairs
                .iter()
                .map(|(p, l)| ((*p).to_owned(), (*l).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*.mdx", "Readme.MDX"));
        assert!(wildcard_match("Dockerfile.*", "Dockerfile.dev"));
        assert!(!wildcard_match("Dockerfile.*", "Dockerfile"));
        assert!(wildcard_match("?akefile", "Makefile"));
        assert!(wildcard_match("*", ""));
        assert!(!wildcard_match("a*b", "acd"));
        assert!(wildcard_match("a*b*c", "axxbyyc"));
    }

    #[test]
    fn most_specific_pattern_wins() {
        let files = settings(&[("*.h", "c"), ("*.hpp.h", "cpp"), ("Jenkinsfile", "groovy")]);
        assert_eq!(files.association_for(Path::new("/p/a.h")), Some("c"));
        assert_eq!(files.association_for(Path::new("/p/a.hpp.h")), Some("cpp"));
        assert_eq!(
            files.association_for(Path::new("/p/Jenkinsfile")),
            Some("groovy")
        );
        assert_eq!(files.association_for(Path::new("/p/a.rs")), None);
    }

    #[test]
    fn path_patterns_match_a_tail_of_the_path() {
        let files = settings(&[("**/.config/*.conf", "toml"), ("src/*.inc", "php")]);
        assert_eq!(
            files.association_for(Path::new("/home/me/.config/x.conf")),
            Some("toml")
        );
        assert_eq!(
            files.association_for(Path::new("/w/proj/src/a.inc")),
            Some("php")
        );
        assert_eq!(files.association_for(Path::new("/w/proj/lib/a.inc")), None);
    }

    #[test]
    fn section_parses_and_drops_empty_entries() {
        let (config, warnings) = Config::from_toml_str(
            "[files]\nassociations = { \"*.mdx\" = \"markdown\", \"*.x\" = \" \" }\n",
        );
        assert_eq!(
            config.files.association_for(Path::new("a.mdx")),
            Some("markdown")
        );
        assert_eq!(config.files.associations.len(), 1);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }
}
