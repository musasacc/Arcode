//! `Cargo.toml` dependencies and `Cargo.lock` versions.

use std::collections::HashMap;

use toml::{Table, Value};

use super::lines::toml_key_line;
use super::{DepKind, DepSource, Dependency};

/// What a `Cargo.toml` declares.
#[derive(Debug, Default)]
pub(crate) struct CargoManifest {
    pub name: Option<String>,
    pub is_workspace: bool,
    /// `[workspace]` without `[package]`.
    pub is_virtual: bool,
    pub deps: Vec<Dependency>,
}

const KINDS: [(&str, DepKind); 4] = [
    ("dependencies", DepKind::Normal),
    ("dev-dependencies", DepKind::Dev),
    ("dev_dependencies", DepKind::Dev),
    ("build-dependencies", DepKind::Build),
];

/// Parses `Cargo.toml` text. `inherited` holds the root's
/// `[workspace.dependencies]` requirements for `workspace = true` entries.
pub(crate) fn parse(
    text: &str,
    inherited: &HashMap<String, String>,
) -> Result<CargoManifest, String> {
    let table: Table = toml::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    let mut out = CargoManifest {
        name: table
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        is_workspace: table.get("workspace").is_some_and(Value::is_table),
        is_virtual: table.get("workspace").is_some_and(Value::is_table)
            && table.get("package").is_none(),
        deps: Vec::new(),
    };
    for (section, kind) in KINDS {
        if let Some(deps) = table.get(section).and_then(Value::as_table) {
            push_section(&mut out.deps, text, section, deps, kind, None, inherited);
        }
    }
    if let Some(targets) = table.get("target").and_then(Value::as_table) {
        for (cfg, t) in targets {
            for (section, kind) in KINDS {
                if let Some(deps) = t.get(section).and_then(Value::as_table) {
                    let full = format!("target.{cfg}.{section}");
                    push_section(&mut out.deps, text, &full, deps, kind, Some(cfg), inherited);
                }
            }
        }
    }
    if let Some(deps) = table
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(Value::as_table)
    {
        let before = out.deps.len();
        push_section(
            &mut out.deps,
            text,
            "workspace.dependencies",
            deps,
            DepKind::Normal,
            None,
            inherited,
        );
        for dep in &mut out.deps[before..] {
            dep.group = Some("workspace".to_string());
        }
    }
    Ok(out)
}

/// The `[workspace.dependencies]` version requirements of a root manifest.
pub(crate) fn workspace_requirements(text: &str) -> HashMap<String, String> {
    let Ok(table) = toml::from_str::<Table>(text) else {
        return HashMap::new();
    };
    let Some(deps) = table
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(Value::as_table)
    else {
        return HashMap::new();
    };
    deps.iter()
        .map(|(k, v)| {
            let version = match v {
                Value::String(s) => s.clone(),
                Value::Table(t) => t
                    .get("version")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                _ => String::new(),
            };
            (k.clone(), version)
        })
        .collect()
}

fn push_section(
    out: &mut Vec<Dependency>,
    text: &str,
    section: &str,
    deps: &Table,
    kind: DepKind,
    target: Option<&str>,
    inherited: &HashMap<String, String>,
) {
    for (key, value) in deps {
        let mut dep = Dependency::new(key, "", kind);
        dep.group = target.map(str::to_string);
        dep.line = toml_key_line(text, section, key);
        match value {
            Value::String(s) => dep.requested = s.clone(),
            Value::Table(t) => {
                let s = |k: &str| t.get(k).and_then(Value::as_str).map(str::to_string);
                dep.requested = s("version").unwrap_or_default();
                if let Some(real) = s("package") {
                    dep.package = real;
                }
                if t.get("optional").and_then(Value::as_bool) == Some(true)
                    && kind == DepKind::Normal
                {
                    dep.kind = DepKind::Optional;
                }
                if t.get("workspace").and_then(Value::as_bool) == Some(true) {
                    dep.source = DepSource::Workspace;
                    if dep.requested.is_empty() {
                        dep.requested = inherited.get(key).cloned().unwrap_or_default();
                    }
                } else if let Some(git) = s("git") {
                    dep.source = DepSource::Git(git);
                } else if let Some(path) = s("path") {
                    dep.source = DepSource::Path(path);
                }
            }
            _ => {}
        }
        out.push(dep);
    }
}

/// `Cargo.lock` versions by package name.
pub(crate) fn lock_versions(text: &str) -> Result<HashMap<String, Vec<String>>, String> {
    let table: Table = toml::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for pkg in table
        .get("package")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = pkg.get("name").and_then(Value::as_str);
        let version = pkg.get("version").and_then(Value::as_str);
        if let (Some(n), Some(v)) = (name, version) {
            map.entry(n.to_string()).or_default().push(v.to_string());
        }
    }
    Ok(map)
}

/// The locked version that satisfies `requested` best: the highest one
/// compatible with it (Cargo's caret rules), else the highest.
pub(crate) fn pick_version(requested: &str, versions: &[String]) -> Option<String> {
    let req = parse_version(requested.trim_start_matches(['^', '~', '=', '>', '<', ' ']));
    let compatible = |v: &[u64; 3]| match req {
        None => true,
        Some(r) if r[0] > 0 => v[0] == r[0],
        Some(r) if r[1] > 0 => v[0] == 0 && v[1] == r[1],
        Some(r) => v[0] == 0 && v[1] == 0 && v[2] == r[2],
    };
    let parsed: Vec<(&String, [u64; 3])> = versions
        .iter()
        .map(|v| (v, parse_version(v).unwrap_or_default()))
        .collect();
    let best = |only_compatible: bool| {
        parsed
            .iter()
            .filter(|(_, p)| !only_compatible || compatible(p))
            .max_by_key(|(_, p)| *p)
            .map(|(v, _)| (*v).clone())
    };
    best(true).or_else(|| best(false))
}

/// `1.2.3-beta` → `[1, 2, 3]`; missing parts are 0.
pub(crate) fn parse_version(text: &str) -> Option<[u64; 3]> {
    let core = text
        .trim()
        .trim_start_matches('v')
        .split([',', ' '])
        .next()?;
    let mut out = [0u64; 3];
    let mut any = false;
    for (i, part) in core.split('.').take(3).enumerate() {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            break;
        }
        out[i] = digits.parse().ok()?;
        any = true;
    }
    any.then_some(out)
}

pub(crate) fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}
