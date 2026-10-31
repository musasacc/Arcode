//! `pyproject.toml` (PEP 621, PEP 735 groups, uv and Poetry tables) and
//! `Pipfile` dependencies.

use toml::{Table, Value};

use super::cargo_deps::first_line;
use super::lines::{quoted_item_line, toml_key_line, toml_section_line};
use super::requirements::{parse_requirement, url_source};
use super::{DepKind, DepSource, Dependency, normalize_python};

/// What a `pyproject.toml` declares.
#[derive(Debug, Default)]
pub(crate) struct Pyproject {
    pub name: Option<String>,
    /// `[tool.poetry]` is present.
    pub poetry: bool,
    /// `[tool.uv]` is present.
    pub uv: bool,
    /// The file declares a project at all (not only tool settings).
    pub is_project: bool,
    pub deps: Vec<Dependency>,
}

fn same(a: &str, b: &str) -> bool {
    normalize_python(a) == normalize_python(b)
}

/// Parses `pyproject.toml` text.
pub(crate) fn parse_pyproject(text: &str) -> Result<Pyproject, String> {
    let table: Table = toml::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    let project = table.get("project").and_then(Value::as_table);
    let tool = table.get("tool").and_then(Value::as_table);
    let poetry = tool.and_then(|t| t.get("poetry")).and_then(Value::as_table);
    let mut out = Pyproject {
        name: project
            .and_then(|p| p.get("name"))
            .or_else(|| poetry.and_then(|p| p.get("name")))
            .and_then(Value::as_str)
            .map(str::to_string),
        poetry: poetry.is_some(),
        uv: tool.is_some_and(|t| t.contains_key("uv")),
        is_project: project.is_some()
            || poetry.is_some()
            || table.contains_key("dependency-groups"),
        deps: Vec::new(),
    };
    let from = |section: &str, key: &str| toml_key_line(text, section, key).unwrap_or(0);
    if let Some(project) = project {
        let at = from("project", "dependencies");
        push_list(
            &mut out.deps,
            text,
            project.get("dependencies"),
            at,
            DepKind::Normal,
            None,
        );
        for (extra, list) in project
            .get("optional-dependencies")
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
        {
            let at = from("project.optional-dependencies", extra);
            push_list(
                &mut out.deps,
                text,
                Some(list),
                at,
                DepKind::Optional,
                Some(extra),
            );
        }
    }
    for (group, list) in table
        .get("dependency-groups")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
    {
        let at = from("dependency-groups", group);
        push_list(
            &mut out.deps,
            text,
            Some(list),
            at,
            DepKind::Dev,
            Some(group),
        );
    }
    if let Some(uv) = tool.and_then(|t| t.get("uv")) {
        let at = from("tool.uv", "dev-dependencies");
        push_list(
            &mut out.deps,
            text,
            uv.get("dev-dependencies"),
            at,
            DepKind::Dev,
            Some("dev"),
        );
    }
    if let Some(poetry) = poetry {
        let mut tables = vec![
            (
                "tool.poetry.dependencies".to_string(),
                poetry.get("dependencies"),
                DepKind::Normal,
                None,
            ),
            (
                "tool.poetry.dev-dependencies".to_string(),
                poetry.get("dev-dependencies"),
                DepKind::Dev,
                Some("dev".to_string()),
            ),
        ];
        for (group, g) in poetry
            .get("group")
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
        {
            tables.push((
                format!("tool.poetry.group.{group}.dependencies"),
                g.get("dependencies"),
                if group == "main" {
                    DepKind::Normal
                } else {
                    DepKind::Dev
                },
                Some(group.clone()),
            ));
        }
        for (section, deps, kind, group) in tables {
            let Some(deps) = deps.and_then(Value::as_table) else {
                continue;
            };
            for (name, value) in deps {
                if name.eq_ignore_ascii_case("python") {
                    continue;
                }
                let mut dep = table_dependency(name, value, kind);
                dep.group = group.clone();
                dep.line = toml_key_line(text, &section, name);
                out.deps.push(dep);
            }
        }
    }
    Ok(out)
}

/// Requirement strings of a list (tables such as `{include-group = ..}`
/// are skipped).
fn push_list(
    out: &mut Vec<Dependency>,
    text: &str,
    list: Option<&Value>,
    at: usize,
    kind: DepKind,
    group: Option<&str>,
) {
    for item in list.and_then(Value::as_array).into_iter().flatten() {
        let Some(req) = item.as_str().and_then(parse_requirement) else {
            continue;
        };
        let mut dep = Dependency::new(&req.name, &req.spec, kind);
        dep.source = req.source;
        dep.group = group.map(str::to_string);
        dep.line = quoted_item_line(text, at, &req.name, same);
        out.push(dep);
    }
}

/// A Poetry or Pipfile entry: `"^1.0"`, `"*"` or a table with `version`,
/// `git`, `path`, `url`, `optional`.
fn table_dependency(name: &str, value: &Value, kind: DepKind) -> Dependency {
    let mut dep = Dependency::new(name, "", kind);
    match value {
        Value::String(s) => dep.requested = s.clone(),
        Value::Table(t) => {
            let s = |k: &str| t.get(k).and_then(Value::as_str).map(str::to_string);
            dep.requested = s("version").unwrap_or_default();
            if let Some(git) = s("git") {
                dep.source = DepSource::Git(git);
            } else if let Some(path) = s("path") {
                dep.source = DepSource::Path(path);
            } else if let Some(url) = s("url").or_else(|| s("file")) {
                dep.source = url_source(&url);
            }
            if t.get("optional").and_then(Value::as_bool) == Some(true) {
                dep.kind = DepKind::Optional;
            }
        }
        // Several constraints (per Python version): show the first.
        Value::Array(items) => {
            if let Some(first) = items.first() {
                return table_dependency(name, first, kind);
            }
        }
        _ => {}
    }
    dep
}

/// Parses a `Pipfile`: `[packages]` and `[dev-packages]`.
pub(crate) fn parse_pipfile(text: &str) -> Result<Vec<Dependency>, String> {
    let table: Table = toml::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    let mut out = Vec::new();
    for (section, kind) in [
        ("packages", DepKind::Normal),
        ("dev-packages", DepKind::Dev),
    ] {
        for (name, value) in table
            .get(section)
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
        {
            let mut dep = table_dependency(name, value, kind);
            dep.line =
                toml_key_line(text, section, name).or_else(|| toml_section_line(text, section));
            out.push(dep);
        }
    }
    Ok(out)
}
