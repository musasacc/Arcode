//! `go.mod` requirements and whether they are in the module cache, checked
//! with a few `stat` calls (no `go` process, no network).

use std::path::{Path, PathBuf};

/// Most requirements checked; a larger `go.mod` is judged by these.
const MAX_CHECKED: usize = 200;

/// A `require` entry of `go.mod`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoRequire {
    /// Module path, e.g. `github.com/BurntSushi/toml`.
    pub path: String,
    /// Version, e.g. `v1.3.2`.
    pub version: String,
}

/// A `replace` directive: `old [version] => target`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Replace {
    /// The replaced module.
    old: String,
    /// Only this version is replaced (`None`: every version).
    old_version: Option<String>,
    /// The module and version used instead (`None`: a local directory,
    /// never downloaded).
    target: Option<GoRequire>,
}

/// The modules `go mod download` puts in the cache for `go.mod` text: the
/// `require` entries with `replace` directives applied (a module replaced
/// by another module is looked up under the replacement; one replaced by a
/// local directory is left out).
pub(crate) fn requirements(text: &str) -> Vec<GoRequire> {
    let mut requires = Vec::new();
    let mut replaces = Vec::new();
    let mut block: Option<&str> = None;
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(kind) = block {
            if line == ")" {
                block = None;
            } else {
                directive(kind, line, &mut requires, &mut replaces);
            }
            continue;
        }
        let (kind, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        if !matches!(kind, "require" | "replace") {
            continue;
        }
        let rest = rest.trim();
        if rest == "(" {
            block = Some(kind);
        } else {
            directive(kind, rest, &mut requires, &mut replaces);
        }
    }
    requires
        .into_iter()
        .filter_map(|r| match replacement(&replaces, &r) {
            None => Some(r),
            Some(replace) => replace.target.clone(),
        })
        .collect()
}

/// The `replace` that applies to `req`: one naming its exact version wins
/// over one for every version (as in `go`).
fn replacement<'a>(replaces: &'a [Replace], req: &GoRequire) -> Option<&'a Replace> {
    let for_module = |exact: bool| {
        replaces.iter().rev().find(|r| {
            r.old == req.path
                && match &r.old_version {
                    Some(v) => exact && *v == req.version,
                    None => !exact,
                }
        })
    };
    for_module(true).or_else(|| for_module(false))
}

/// `line` without a `//` comment. A `//` right after a drive letter or
/// inside a word (`C://x`, `file://`) is rare in `go.mod`; only `//` at the
/// start or after whitespace starts a comment.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while let Some(pos) = line[i..].find("//") {
        let at = i + pos;
        if at == 0 || bytes[at - 1].is_ascii_whitespace() {
            return &line[..at];
        }
        i = at + 2;
    }
    line
}

fn directive(kind: &str, line: &str, requires: &mut Vec<GoRequire>, replaces: &mut Vec<Replace>) {
    let words: Vec<&str> = line.split_whitespace().map(unquote).collect();
    match kind {
        "require" => {
            if let [path, version, ..] = words.as_slice() {
                requires.push(GoRequire {
                    path: (*path).to_string(),
                    version: (*version).to_string(),
                });
            }
        }
        _ => {
            // `old [version] => new version` replaces a module with another
            // module; `old [version] => path` (no version) with a local
            // directory (`./x`, `../x`, `/x`, `C:\x`, ...).
            let Some(arrow) = words.iter().position(|w| *w == "=>") else {
                return;
            };
            let (old, target) = (&words[..arrow], &words[arrow + 1..]);
            let Some(module) = old.first() else {
                return;
            };
            let target = match target {
                [path, version] => Some(GoRequire {
                    path: (*path).to_string(),
                    version: (*version).to_string(),
                }),
                [_] => None,
                _ => return,
            };
            replaces.push(Replace {
                old: (*module).to_string(),
                old_version: old.get(1).map(|v| (*v).to_string()),
                target,
            });
        }
    }
}

fn unquote(word: &str) -> &str {
    word.trim_matches('"').trim_matches('`')
}

/// The module cache: `GOMODCACHE`, else the first `GOPATH` entry's
/// `pkg/mod`, else `~/go/pkg/mod`.
pub(crate) fn module_cache() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("GOMODCACHE").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    if let Some(first) = std::env::var_os("GOPATH")
        .as_deref()
        .and_then(|p| std::env::split_paths(p).next())
        .filter(|p| !p.as_os_str().is_empty())
    {
        return Some(first.join("pkg").join("mod"));
    }
    home_dir().map(|home| home.join("go").join("pkg").join("mod"))
}

fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// The cache folder of a module: upper-case letters are written `!` plus
/// the lower-case letter (`BurntSushi` → `!burnt!sushi`).
pub(crate) fn escaped(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 4);
    for c in path.chars() {
        if c.is_ascii_uppercase() {
            out.push('!');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Whether some requirement of the `go.mod` in `dir` is missing from
/// `cache`. `false` when the module vendors its dependencies or requires
/// nothing.
pub(crate) fn modules_missing(dir: &Path, text: &str, cache: &Path) -> bool {
    if dir.join("vendor").join("modules.txt").is_file() {
        return false;
    }
    requirements(text).iter().take(MAX_CHECKED).any(|r| {
        let folder = format!("{}@{}", escaped(&r.path), r.version);
        !cache.join(folder).is_dir()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GO_MOD: &str = "module example.com/app\n\ngo 1.22\n\n\
        require github.com/BurntSushi/toml v1.3.2\n\
        require (\n\tgolang.org/x/text v0.14.0 // indirect\n\texample.com/local v0.0.0\n)\n\
        replace example.com/local => ../local\n";

    #[test]
    fn reads_requirements_without_local_replacements() {
        let reqs = requirements(GO_MOD);
        let paths: Vec<_> = reqs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, ["github.com/BurntSushi/toml", "golang.org/x/text"]);
        assert_eq!(reqs[1].version, "v0.14.0");
    }

    #[test]
    fn applies_module_and_version_replacements() {
        let text = "module a\n\nrequire (\n\tx.com/a v1.0.0\n\tx.com/b v1.2.0\n\tx.com/c v0.1.0\n)\n\
            replace x.com/a => y.com/a v1.0.1 // fork\n\
            replace x.com/b v1.1.0 => ./b\n\
            replace x.com/c v0.1.0 => D:/work/c\n";
        let reqs = requirements(text);
        let found: Vec<_> = reqs
            .iter()
            .map(|r| format!("{}@{}", r.path, r.version))
            .collect();
        // b's replace names another version, so it does not apply.
        assert_eq!(found, ["y.com/a@v1.0.1", "x.com/b@v1.2.0"]);
    }

    #[test]
    fn strips_comments_only_at_word_starts() {
        assert_eq!(strip_comment("x v1 // indirect"), "x v1 ");
        assert_eq!(strip_comment("// all"), "");
        assert_eq!(strip_comment("a => C://dir"), "a => C://dir");
    }

    #[test]
    fn escapes_upper_case_letters() {
        assert_eq!(
            escaped("github.com/BurntSushi/toml"),
            "github.com/!burnt!sushi/toml"
        );
    }

    #[test]
    fn checks_the_module_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("mod");
        let project = dir.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        assert!(modules_missing(&project, GO_MOD, &cache));
        std::fs::create_dir_all(cache.join("github.com/!burnt!sushi/toml@v1.3.2")).unwrap();
        assert!(modules_missing(&project, GO_MOD, &cache));
        std::fs::create_dir_all(cache.join("golang.org/x/text@v0.14.0")).unwrap();
        assert!(!modules_missing(&project, GO_MOD, &cache));
        assert!(!modules_missing(&project, "module a\n", &cache));
    }

    #[test]
    fn vendored_modules_count_as_installed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("vendor")).unwrap();
        std::fs::write(dir.path().join("vendor/modules.txt"), "").unwrap();
        assert!(!modules_missing(
            dir.path(),
            GO_MOD,
            &dir.path().join("none")
        ));
    }
}
