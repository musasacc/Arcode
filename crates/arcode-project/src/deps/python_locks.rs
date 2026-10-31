//! Locked Python versions (`uv.lock`, `poetry.lock`, `Pipfile.lock`) and
//! the versions installed in a virtual environment (`*.dist-info`).

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value as Json;
use toml::{Table, Value};

use super::cargo_deps::first_line;
use super::normalize_python;

/// Normalized name → version.
pub(crate) type Versions = HashMap<String, String>;

/// Reads `uv.lock` or `poetry.lock` (both list `[[package]]` tables).
pub(crate) fn toml_lock(text: &str) -> Result<Versions, String> {
    let table: Table = toml::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    let mut out = Versions::new();
    for pkg in table
        .get("package")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = pkg.get("name").and_then(Value::as_str);
        let version = pkg.get("version").and_then(Value::as_str);
        if let (Some(n), Some(v)) = (name, version) {
            out.entry(normalize_python(n))
                .or_insert_with(|| v.to_string());
        }
    }
    Ok(out)
}

/// Reads `Pipfile.lock` (`default` and `develop`, versions as `==1.0`).
pub(crate) fn pipfile_lock(text: &str) -> Result<Versions, String> {
    let json: Json = serde_json::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    let mut out = Versions::new();
    for section in ["default", "develop"] {
        for (name, entry) in json
            .get(section)
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            if let Some(v) = entry.get("version").and_then(Json::as_str) {
                out.entry(normalize_python(name))
                    .or_insert_with(|| v.trim_start_matches("==").to_string());
            }
        }
    }
    Ok(out)
}

/// The distributions installed in the virtual environment at `venv`.
pub(crate) fn venv_versions(venv: &Path) -> Versions {
    let mut out = Versions::new();
    for site in site_packages(venv) {
        let Ok(entries) = std::fs::read_dir(&site) else {
            continue;
        };
        for entry in entries.flatten().take(20_000) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some((n, v)) = dist_info(&name) {
                out.insert(normalize_python(n), v.to_string());
            }
        }
    }
    out
}

/// `typing_extensions-4.9.0.dist-info` → (`typing_extensions`, `4.9.0`).
pub(crate) fn dist_info(dir_name: &str) -> Option<(&str, &str)> {
    let stem = dir_name.strip_suffix(".dist-info")?;
    let (name, version) = stem.split_once('-')?;
    (!name.is_empty() && version.starts_with(|c: char| c.is_ascii_digit()))
        .then_some((name, version))
}

/// `lib/python3.*/site-packages` (Unix) and `Lib/site-packages` (Windows).
fn site_packages(venv: &Path) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![venv.join("Lib").join("site-packages")];
    if let Ok(entries) = std::fs::read_dir(venv.join("lib")) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("python") {
                dirs.push(entry.path().join("site-packages"));
            }
        }
    }
    dirs.retain(|d| d.is_dir());
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dist_info_names() {
        assert_eq!(
            dist_info("typing_extensions-4.9.0.dist-info"),
            Some(("typing_extensions", "4.9.0"))
        );
        assert_eq!(dist_info("pip-24.0.dist-info"), Some(("pip", "24.0")));
        assert_eq!(dist_info("foo.dist-info"), None);
        assert_eq!(dist_info("README"), None);
    }

    #[test]
    fn venv_scan() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("lib/python3.12/site-packages");
        std::fs::create_dir_all(site.join("Flask-3.0.0.dist-info")).unwrap();
        std::fs::create_dir_all(site.join("flask")).unwrap();
        let v = venv_versions(dir.path());
        assert_eq!(v.get("flask").map(String::as_str), Some("3.0.0"));
        assert_eq!(v.len(), 1);
    }
}
