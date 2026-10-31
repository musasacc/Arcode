//! PEP 508 requirement strings and `requirements*.txt` files.

use super::{DepKind, DepSource, Dependency};

/// A parsed requirement: `name[extras] spec ; markers` or `name @ url`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Requirement {
    pub name: String,
    pub spec: String,
    pub source: DepSource,
}

/// Parses one PEP 508 requirement; `None` when it does not start with a
/// project name.
pub(crate) fn parse_requirement(text: &str) -> Option<Requirement> {
    let text = text.trim();
    let name_len = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
        .unwrap_or(text.len());
    if name_len == 0 {
        return None;
    }
    let name = text[..name_len].to_string();
    let mut rest = text[name_len..].trim_start();
    if rest.starts_with('[') {
        rest = rest.find(']').map_or("", |i| &rest[i + 1..]).trim_start();
    }
    if let Some(url) = rest.strip_prefix('@') {
        let url = url.split(" ;").next().unwrap_or(url).trim();
        return Some(Requirement {
            name,
            spec: String::new(),
            source: url_source(url),
        });
    }
    let spec = rest.split(';').next().unwrap_or("").trim();
    let spec = spec.trim_start_matches('(').trim_end_matches(')').trim();
    Some(Requirement {
        name,
        spec: spec.to_string(),
        source: DepSource::Registry,
    })
}

/// The source a requirement URL or path denotes.
pub(crate) fn url_source(url: &str) -> DepSource {
    if url.starts_with("git+") || url.ends_with(".git") {
        DepSource::Git(url.to_string())
    } else if let Some(path) = url.strip_prefix("file://") {
        DepSource::Path(path.to_string())
    } else if url.contains("://") {
        DepSource::Url(url.to_string())
    } else {
        DepSource::Path(url.to_string())
    }
}

/// The dependencies a requirements file lists. Options (`-r`, `-c`,
/// `--index-url`, ...) are skipped; `-e` lines and bare URLs or paths are
/// listed under their `#egg=` name or file name.
pub(crate) fn parse_requirements_file(text: &str) -> Vec<Dependency> {
    let mut out = Vec::new();
    let mut pending = String::new();
    let mut start = 0;
    for (ix, raw) in text.lines().enumerate() {
        if pending.is_empty() {
            start = ix;
        }
        let line = strip_comment(raw);
        if let Some(cont) = line.strip_suffix('\\') {
            pending.push_str(cont);
            pending.push(' ');
            continue;
        }
        pending.push_str(line);
        let full = std::mem::take(&mut pending);
        if let Some(mut dep) = requirement_line(full.trim()) {
            dep.line = Some(start);
            out.push(dep);
        }
    }
    out
}

/// `line` without a `#` comment (one at the start or after a blank).
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return line[..i].trim_end();
        }
    }
    line.trim_end()
}

fn requirement_line(line: &str) -> Option<Dependency> {
    if line.is_empty() {
        return None;
    }
    let editable = line
        .strip_prefix("-e ")
        .or_else(|| line.strip_prefix("--editable "))
        .or_else(|| line.strip_prefix("--editable="));
    let target = match editable {
        Some(t) => t.trim(),
        None if line.starts_with('-') => return None,
        None => line,
    };
    let is_location = editable.is_some()
        || target.contains("://")
        || target.starts_with('.')
        || target.starts_with('/')
        || target.starts_with("git+");
    if is_location {
        let location = target.split_whitespace().next().unwrap_or(target);
        let name = egg_name(location).or_else(|| file_stem(location))?;
        let mut dep = Dependency::new(&name, "", DepKind::Normal);
        dep.source = url_source(location.split('#').next().unwrap_or(location));
        return Some(dep);
    }
    let req = parse_requirement(target)?;
    let mut dep = Dependency::new(&req.name, &req.spec, DepKind::Normal);
    dep.source = req.source;
    Some(dep)
}

/// `...#egg=name` → `name`.
fn egg_name(location: &str) -> Option<String> {
    let fragment = location.split_once('#')?.1;
    fragment
        .split('&')
        .find_map(|p| p.strip_prefix("egg="))
        .map(|n| n.split(['[', '=']).next().unwrap_or(n).to_string())
        .filter(|n| !n.is_empty())
}

/// The project name from a wheel or sdist file name or a folder name.
fn file_stem(location: &str) -> Option<String> {
    let last = location
        .trim_end_matches('/')
        .rsplit(['/', '\\'])
        .next()?
        .split('#')
        .next()?;
    let name = last.split('-').next().unwrap_or(last);
    let name = name.trim_end_matches(".git");
    (!name.is_empty() && name != ".").then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pep508() {
        let r = parse_requirement("requests[socks] >=2.31,<3 ; python_version >= '3.8'").unwrap();
        assert_eq!(
            (r.name.as_str(), r.spec.as_str()),
            ("requests", ">=2.31,<3")
        );
        let r = parse_requirement("pkg @ git+https://example.org/pkg.git@v1").unwrap();
        assert!(matches!(r.source, DepSource::Git(_)));
        let r = parse_requirement("Django (>=4.2)").unwrap();
        assert_eq!(r.spec, ">=4.2");
        assert!(parse_requirement(">=1").is_none());
        assert!(parse_requirement("").is_none());
    }

    #[test]
    fn requirement_files() {
        let text = "# comment\n--index-url https://pypi.org/simple\n-r base.txt\nflask==3.0.0  # web\nnumpy>=1.26 \\\n    ; python_version > '3.9'\n-e git+https://github.com/u/mylib.git@main#egg=mylib\n./local/pkg\nhttps://x.org/wheels/foo-1.0-py3-none-any.whl\n";
        let deps = parse_requirements_file(text);
        let found: Vec<_> = deps
            .iter()
            .map(|d| {
                (
                    d.name.as_str(),
                    d.requested.as_str(),
                    d.line,
                    d.source.label(),
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("flask", "==3.0.0", Some(3), ""),
                ("numpy", ">=1.26", Some(4), ""),
                ("mylib", "", Some(6), "git"),
                ("pkg", "", Some(7), "path"),
                ("foo", "", Some(8), "url"),
            ]
        );
    }
}
