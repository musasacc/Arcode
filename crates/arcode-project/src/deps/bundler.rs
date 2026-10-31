//! `Gemfile` gems (with `group ... do` blocks and inline options) and
//! `Gemfile.lock` versions. The Gemfile is Ruby; only the usual
//! declarative forms are understood, anything else is skipped.

use std::collections::HashMap;

use super::{DepKind, DepSource, Dependency};

/// Parses `Gemfile` text.
pub(crate) fn parse(text: &str) -> Vec<Dependency> {
    let mut out = Vec::new();
    // Groups of each open `do` block (empty for other blocks).
    let mut blocks: Vec<Vec<String>> = Vec::new();
    for (ix, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line == "end" {
            blocks.pop();
            continue;
        }
        let word = line
            .split(|c: char| c.is_whitespace() || c == '(')
            .next()
            .unwrap_or("");
        let opens = line.ends_with(" do") || (line.contains(" do |") && line.ends_with('|'));
        if word == "gem" {
            if let Some(mut dep) = gem_line(line) {
                let mut groups: Vec<String> = blocks.iter().flatten().cloned().collect();
                if let Some(g) = dep.group.take() {
                    groups.extend(g.split(", ").map(str::to_string));
                }
                if !groups.is_empty() {
                    if groups.iter().all(|g| g == "development" || g == "test") {
                        dep.kind = DepKind::Dev;
                    }
                    dep.group = Some(groups.join(", "));
                }
                dep.line = Some(ix);
                out.push(dep);
            }
        } else if opens {
            let groups = if word == "group" {
                symbols(line.trim_start_matches("group").trim_end_matches(" do"))
            } else {
                Vec::new()
            };
            blocks.push(groups);
        } else if matches!(word, "if" | "unless" | "case" | "begin" | "while" | "def") {
            blocks.push(Vec::new());
        }
    }
    out
}

fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' => return &line[..i],
            None => {}
        }
    }
    line
}

/// `:development, :test` or `[:development, :test]` → names.
fn symbols(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace() || c == '[' || c == ']')
        .filter_map(|w| w.trim().strip_prefix(':'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Arguments of a call, split at top-level commas.
fn split_args(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0i32;
    for c in text.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    cur.push(c);
                }
                '[' | '(' | '{' => {
                    depth += 1;
                    cur.push(c);
                }
                ']' | ')' | '}' => {
                    depth -= 1;
                    cur.push(c);
                }
                ',' if depth == 0 => args.push(std::mem::take(&mut cur).trim().to_string()),
                _ => cur.push(c),
            },
        }
    }
    args.push(cur.trim().to_string());
    args.retain(|a| !a.is_empty());
    args
}

fn unquote(text: &str) -> Option<&str> {
    let t = text.trim();
    let q = t.chars().next()?;
    ((q == '"' || q == '\'') && t.len() >= 2 && t.ends_with(q)).then(|| &t[1..t.len() - 1])
}

/// `gem "name", "~> 1.0", group: :test, git: "..."`.
fn gem_line(line: &str) -> Option<Dependency> {
    let rest = line.strip_prefix("gem")?.trim();
    let rest = rest
        .strip_prefix('(')
        .map_or(rest, |r| r.trim_end_matches(')'));
    let args = split_args(rest);
    let name = unquote(args.first()?)?;
    let mut dep = Dependency::new(name, "", DepKind::Normal);
    let mut reqs = Vec::new();
    for arg in &args[1..] {
        if let Some(req) = unquote(arg) {
            reqs.push(req.to_string());
            continue;
        }
        let (key, value) = match arg.split_once("=>") {
            Some((k, v)) => (k.trim().trim_start_matches(':'), v.trim()),
            None => match arg.split_once(':') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => continue,
            },
        };
        let text_value = unquote(value).map(str::to_string);
        match key {
            "group" | "groups" => dep.group = Some(symbols(value).join(", ")),
            "git" => dep.source = DepSource::Git(text_value.unwrap_or_default()),
            "github" => {
                dep.source = DepSource::Git(format!(
                    "https://github.com/{}",
                    text_value.unwrap_or_default()
                ))
            }
            "path" => dep.source = DepSource::Path(text_value.unwrap_or_default()),
            _ => {}
        }
    }
    dep.requested = reqs.join(", ");
    Some(dep)
}

/// `Gemfile.lock` versions: the `    name (version)` lines of the `specs:`
/// sections (platform suffixes removed).
pub(crate) fn lock_versions(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let Some(spec) = line.strip_prefix("    ") else {
            continue;
        };
        if spec.starts_with(' ') {
            continue;
        }
        let Some((name, rest)) = spec.split_once(" (") else {
            continue;
        };
        let version = rest.trim_end_matches(')');
        let version = match version.split_once('-') {
            Some((v, platform)) if !platform.starts_with(|c: char| c.is_ascii_digit()) => v,
            _ => version,
        };
        out.entry(name.to_string())
            .or_insert_with(|| version.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_and_symbols() {
        assert_eq!(
            split_args("\"a\", \"~> 1, < 2\", platforms: %i[ mri windows ]").len(),
            3
        );
        assert_eq!(symbols("[:development, :test]"), ["development", "test"]);
        assert_eq!(strip_comment("gem 'a#b' # c"), "gem 'a#b' ");
    }
}
