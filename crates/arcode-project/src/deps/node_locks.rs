//! Installed versions from Node lockfiles: `package-lock.json` (v1-v3),
//! `yarn.lock` (classic and berry), `pnpm-lock.yaml` and the text
//! `bun.lock`. The binary `bun.lockb` is not read.

use std::collections::HashMap;

use serde_json::Value;

use super::cargo_deps::first_line;
use super::mini_yaml::{self, Node};

/// Versions from one lockfile.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct NodeLock {
    /// Per workspace member (`""` for the root, else its folder relative
    /// to the lockfile's, `/`-separated): name → version.
    by_member: HashMap<String, HashMap<String, String>>,
    /// (name, requested range) → version (yarn).
    by_spec: HashMap<(String, String), String>,
    /// name → version (any member).
    by_name: HashMap<String, String>,
}

impl NodeLock {
    /// The version of `name` (requested as `requested`) for the member at
    /// `member` (relative folder, `""` for the root).
    pub(crate) fn version(&self, member: &str, name: &str, requested: &str) -> Option<String> {
        let from_member = |m: &str| self.by_member.get(m).and_then(|map| map.get(name)).cloned();
        from_member(member)
            .or_else(|| {
                self.by_spec
                    .get(&(name.to_string(), requested.to_string()))
                    .cloned()
            })
            .or_else(|| from_member(""))
            .or_else(|| self.by_name.get(name).cloned())
    }

    fn insert(&mut self, member: &str, name: &str, version: &str) {
        self.by_member
            .entry(member.to_string())
            .or_default()
            .insert(name.to_string(), version.to_string());
        self.by_name
            .entry(name.to_string())
            .or_insert_with(|| version.to_string());
    }
}

/// Reads the lockfile named `file_name` from its text.
pub(crate) fn parse(file_name: &str, text: &str) -> Result<NodeLock, String> {
    match file_name {
        "package-lock.json" | "npm-shrinkwrap.json" => package_lock(text),
        "yarn.lock" => Ok(yarn_lock(text)),
        "pnpm-lock.yaml" => Ok(pnpm_lock(text)),
        "bun.lock" => bun_lock(text),
        other => Err(format!("{other} cannot be read")),
    }
}

fn package_lock(text: &str) -> Result<NodeLock, String> {
    let json: Value = serde_json::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    let mut lock = NodeLock::default();
    if let Some(packages) = json.get("packages").and_then(Value::as_object) {
        for (key, entry) in packages {
            let Some(version) = entry.get("version").and_then(Value::as_str) else {
                continue;
            };
            let Some(pos) = key.rfind("node_modules/") else {
                continue;
            };
            let name = &key[pos + "node_modules/".len()..];
            let member = key[..pos].trim_end_matches('/');
            if member.contains("node_modules") {
                continue;
            }
            lock.insert(member, name, version);
        }
    } else if let Some(deps) = json.get("dependencies").and_then(Value::as_object) {
        for (name, entry) in deps {
            if let Some(version) = entry.get("version").and_then(Value::as_str) {
                lock.insert("", name, version);
            }
        }
    }
    Ok(lock)
}

/// `name@range` → (`name`, `range`); scoped names keep their leading `@`.
fn split_spec(spec: &str) -> Option<(&str, &str)> {
    let (at, _) = spec.char_indices().skip(1).find(|(_, c)| *c == '@')?;
    Some((&spec[..at], &spec[at + 1..]))
}

fn yarn_lock(text: &str) -> NodeLock {
    let mut lock = NodeLock::default();
    let mut current: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ') {
            current.clear();
            let header = line.trim_end().trim_end_matches(':');
            for spec in header.split(", ") {
                let spec = spec.trim().trim_matches('"');
                if let Some((name, range)) = split_spec(spec) {
                    // Berry writes `npm:^1.0`.
                    let range = range.strip_prefix("npm:").unwrap_or(range);
                    current.push((name.to_string(), range.to_string()));
                }
            }
            continue;
        }
        let t = line.trim();
        let version = t
            .strip_prefix("version ")
            .or_else(|| t.strip_prefix("version: "))
            .map(|v| v.trim().trim_matches('"'));
        if let (Some(version), false) = (version, line.starts_with("    ")) {
            for (name, range) in current.drain(..) {
                lock.by_name
                    .entry(name.clone())
                    .or_insert_with(|| version.to_string());
                lock.by_spec.insert((name, range), version.to_string());
            }
        }
    }
    lock
}

/// `18.2.0(react@18.2.0)` or `18.2.0_react@18.2.0` → `18.2.0`.
fn clean_pnpm_version(v: &str) -> &str {
    let v = v.split('(').next().unwrap_or(v);
    if v.starts_with("link:") {
        v
    } else {
        v.split('_').next().unwrap_or(v)
    }
}

fn pnpm_lock(text: &str) -> NodeLock {
    let doc = mini_yaml::parse(text);
    let mut lock = NodeLock::default();
    let mut read_importer = |member: &str, node: &Node| {
        for section in ["dependencies", "devDependencies", "optionalDependencies"] {
            for (name, entry) in node.get(section).map(Node::entries).unwrap_or_default() {
                let version = entry
                    .as_str()
                    .or_else(|| entry.get("version").and_then(Node::as_str));
                if let Some(v) = version {
                    lock.insert(member, name, clean_pnpm_version(v));
                }
            }
        }
    };
    match doc.get("importers") {
        Some(importers) => {
            for (path, node) in importers.entries() {
                let member = if path == "." { "" } else { path.as_str() };
                read_importer(member, node);
            }
        }
        None => read_importer("", &doc),
    }
    lock
}

/// The text `bun.lock` is JSON with trailing commas.
fn bun_lock(text: &str) -> Result<NodeLock, String> {
    let json: Value = serde_json::from_str(&strip_trailing_commas(text))
        .map_err(|e| first_line(&e.to_string()))?;
    let mut lock = NodeLock::default();
    for (key, entry) in json
        .get("packages")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let Some(spec) = entry.get(0).and_then(Value::as_str) else {
            continue;
        };
        if let Some((name, version)) = split_spec(spec)
            && name == key
        {
            lock.insert("", name, version);
        }
    }
    Ok(lock)
}

/// Removes commas directly before `}` or `]` (outside strings).
pub(crate) fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for (i, &c) in chars.iter().enumerate() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_commas() {
        assert_eq!(
            strip_trailing_commas("{\"a\": [1, 2,], \"b,\": 3,}"),
            "{\"a\": [1, 2], \"b,\": 3}"
        );
    }

    #[test]
    fn spec_split() {
        assert_eq!(split_spec("@a/b@^1"), Some(("@a/b", "^1")));
        assert_eq!(split_spec("lodash@4"), Some(("lodash", "4")));
        assert_eq!(split_spec("x"), None);
        assert_eq!(clean_pnpm_version("18.2.0(react@18.2.0)"), "18.2.0");
        assert_eq!(clean_pnpm_version("1.0.0_react@18"), "1.0.0");
    }
}
