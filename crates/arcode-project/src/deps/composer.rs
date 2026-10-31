//! `composer.json` requirements and `composer.lock` versions.

use std::collections::HashMap;

use serde_json::Value;

use super::cargo_deps::first_line;
use super::lines::json_key_line;
use super::{DepKind, Dependency};

/// What a `composer.json` declares.
#[derive(Debug, Default)]
pub(crate) struct ComposerManifest {
    pub name: Option<String>,
    pub deps: Vec<Dependency>,
}

/// Platform requirements (`php`, extensions, libraries) are not packages.
fn is_platform(name: &str) -> bool {
    name == "php"
        || name.starts_with("php-")
        || name.starts_with("ext-")
        || name.starts_with("lib-")
        || name == "composer-plugin-api"
        || name == "composer-runtime-api"
}

/// Parses `composer.json` text.
pub(crate) fn parse(text: &str) -> Result<ComposerManifest, String> {
    let json: Value = serde_json::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    let mut deps = Vec::new();
    for (section, kind) in [("require", DepKind::Normal), ("require-dev", DepKind::Dev)] {
        for (name, spec) in json
            .get(section)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if is_platform(name) {
                continue;
            }
            let mut dep = Dependency::new(name, spec.as_str().unwrap_or(""), kind);
            dep.line = json_key_line(text, section, name);
            deps.push(dep);
        }
    }
    Ok(ComposerManifest {
        name: json.get("name").and_then(Value::as_str).map(str::to_string),
        deps,
    })
}

/// `composer.lock` versions by package name (lower case).
pub(crate) fn lock_versions(text: &str) -> Result<HashMap<String, String>, String> {
    let json: Value = serde_json::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    let mut out = HashMap::new();
    for section in ["packages", "packages-dev"] {
        for pkg in json
            .get(section)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = pkg.get("name").and_then(Value::as_str);
            let version = pkg.get("version").and_then(Value::as_str);
            if let (Some(n), Some(v)) = (name, version) {
                out.insert(n.to_ascii_lowercase(), v.to_string());
            }
        }
    }
    Ok(out)
}
