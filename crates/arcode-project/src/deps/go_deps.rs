//! `go.mod` requirements with their lines, `// indirect` markers and
//! local `replace` directives. `go.mod` pins exact versions, so the
//! requested version is also the selected one.

use super::{DepKind, DepSource, Dependency};

/// What a `go.mod` declares.
#[derive(Debug, Default)]
pub(crate) struct GoModule {
    pub module: Option<String>,
    pub deps: Vec<Dependency>,
}

/// Parses `go.mod` text (never fails; unknown lines are skipped).
pub(crate) fn parse(text: &str) -> GoModule {
    let mut out = GoModule::default();
    let mut replaces: Vec<(String, String)> = Vec::new();
    let mut block: Option<String> = None;
    for (ix, raw) in text.lines().enumerate() {
        let (code, comment) = split_comment(raw);
        let line = code.trim();
        if line.is_empty() {
            continue;
        }
        let indirect = comment.is_some_and(|c| c.trim().starts_with("indirect"));
        if let Some(kind) = &block {
            if line == ")" {
                block = None;
            } else {
                let kind = kind.clone();
                directive(&kind, line, ix, indirect, &mut out, &mut replaces);
            }
            continue;
        }
        let (kind, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        if rest == "(" {
            block = Some(kind.to_string());
        } else {
            directive(kind, rest, ix, indirect, &mut out, &mut replaces);
        }
    }
    for (old, target) in replaces {
        for dep in out.deps.iter_mut().filter(|d| d.name == old) {
            dep.source = DepSource::Path(target.clone());
        }
    }
    out
}

fn directive(
    kind: &str,
    rest: &str,
    ix: usize,
    indirect: bool,
    out: &mut GoModule,
    replaces: &mut Vec<(String, String)>,
) {
    let words: Vec<&str> = rest
        .split_whitespace()
        .map(|w| w.trim_matches('"'))
        .collect();
    match (kind, words.as_slice()) {
        ("module", [path, ..]) => out.module = Some((*path).to_string()),
        ("require", [path, version, ..]) => {
            let kind = if indirect {
                DepKind::Indirect
            } else {
                DepKind::Normal
            };
            let mut dep = Dependency::new(path, version, kind);
            dep.locked = Some((*version).to_string());
            dep.line = Some(ix);
            out.deps.push(dep);
        }
        ("replace", _) => {
            // `old [v] => ./local` (a directory has no version after it).
            if let Some(arrow) = words.iter().position(|w| *w == "=>")
                && let (Some(old), [target]) = (words.first(), &words[arrow + 1..])
            {
                replaces.push(((*old).to_string(), (*target).to_string()));
            }
        }
        _ => {}
    }
}

/// Code and `//` comment of a line (a `//` at the start or after a blank).
fn split_comment(line: &str) -> (&str, Option<&str>) {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(pos) = line[from..].find("//") {
        let at = from + pos;
        if at == 0 || bytes[at - 1].is_ascii_whitespace() {
            return (&line[..at], Some(&line[at + 2..]));
        }
        from = at + 2;
    }
    (line, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_requires_and_replacements() {
        let m = parse(
            "module example.com/app\n\ngo 1.22\n\nrequire github.com/BurntSushi/toml v1.3.2\nrequire (\n\tgolang.org/x/text v0.14.0 // indirect\n\texample.com/local v0.0.0\n)\nreplace example.com/local => ../local\n",
        );
        assert_eq!(m.module.as_deref(), Some("example.com/app"));
        let found: Vec<_> = m
            .deps
            .iter()
            .map(|d| (d.name.as_str(), d.requested.as_str(), d.kind, d.line))
            .collect();
        assert_eq!(
            found,
            [
                (
                    "github.com/BurntSushi/toml",
                    "v1.3.2",
                    DepKind::Normal,
                    Some(4)
                ),
                ("golang.org/x/text", "v0.14.0", DepKind::Indirect, Some(6)),
                ("example.com/local", "v0.0.0", DepKind::Normal, Some(7)),
            ]
        );
        assert_eq!(m.deps[2].source, DepSource::Path("../local".into()));
        assert_eq!(m.deps[0].locked.as_deref(), Some("v1.3.2"));
    }
}
