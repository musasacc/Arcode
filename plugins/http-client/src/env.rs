//! Environment files (docs/PLUGINS.md §9).
//!
//! `http-client.env.json` and `http-client.private.env.json` (the private
//! one is meant to stay out of version control) map environment names to
//! variables, JetBrains style, plus an optional `$shared` section that
//! every environment inherits (VS Code REST Client style):
//!
//! ```json
//! { "$shared": { "version": "v1" },
//!   "dev":  { "host": "localhost:8080", "token": "dev" },
//!   "prod": { "host": "api.example.com" } }
//! ```
//!
//! Each file is looked up in the `.http` file's directory, then its parents
//! up to the project root; the nearest one wins. Files are read with
//! `std::fs` under the WASI preopen `/project`, so without the workspace
//! grant there are simply no environments. The selected environment is
//! kept in project-scope `kv`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use arcode_plugin::kv;
use serde_json::Value;

/// The shared environment file.
pub const PUBLIC_FILE: &str = "http-client.env.json";
/// The private environment file (secrets; usually git-ignored).
pub const PRIVATE_FILE: &str = "http-client.private.env.json";
/// The section every environment inherits.
pub const SHARED: &str = "$shared";
/// Largest environment file read.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// kv key (project scope) of the selected environment's name.
const SELECTED_KEY: &str = "environment";

/// Variables per environment, already merged (public < private).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environments {
    envs: BTreeMap<String, BTreeMap<String, String>>,
}

impl Environments {
    /// Merges the public and private file contents.
    pub fn parse(public: Option<&str>, private: Option<&str>) -> Result<Self, String> {
        let mut envs: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for (file, text) in [(PUBLIC_FILE, public), (PRIVATE_FILE, private)] {
            let Some(text) = text else { continue };
            let root: Value =
                serde_json::from_str(text).map_err(|e| format!("{file}: invalid JSON: {e}"))?;
            let Value::Object(root) = root else {
                return Err(format!("{file}: expected an object of environments"));
            };
            for (name, vars) in root {
                let Value::Object(vars) = vars else {
                    return Err(format!("{file}: environment `{name}` is not an object"));
                };
                let env = envs.entry(name).or_default();
                for (key, value) in vars {
                    if let Some(v) = scalar(&value) {
                        env.insert(key, v);
                    }
                }
            }
        }
        Ok(Self { envs })
    }

    /// Reads the nearest environment files for the `.http` file at
    /// `doc_path` (project-relative) under `project`. Missing or unreadable
    /// files count as absent; invalid JSON is an error.
    pub fn load(project: &Path, doc_path: Option<&str>) -> Result<Self, String> {
        let dirs = search_dirs(project, doc_path);
        let public = nearest(&dirs, PUBLIC_FILE);
        let private = nearest(&dirs, PRIVATE_FILE);
        Self::parse(public.as_deref(), private.as_deref())
    }

    /// Environment names, sorted (`$shared` excluded).
    pub fn names(&self) -> Vec<String> {
        self.envs
            .keys()
            .filter(|n| !n.starts_with('$'))
            .cloned()
            .collect()
    }

    /// True if `name` is an environment.
    pub fn contains(&self, name: &str) -> bool {
        !name.starts_with('$') && self.envs.contains_key(name)
    }

    /// `$shared` merged with `name`'s variables (`None`: `$shared` only).
    pub fn variables(&self, name: Option<&str>) -> BTreeMap<String, String> {
        let mut vars = self.envs.get(SHARED).cloned().unwrap_or_default();
        if let Some(env) = name
            .filter(|n| !n.starts_with('$'))
            .and_then(|n| self.envs.get(n))
        {
            vars.extend(env.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        vars
    }
}

/// Strings as is, numbers and booleans as text, `null` as empty; objects
/// and arrays (JetBrains auth configs) are skipped.
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => Some(String::new()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// The `.http` file's directory and its parents, nearest first, ending at
/// `project`. Paths with `..` or absolute paths only search `project`.
pub fn search_dirs(project: &Path, doc_path: Option<&str>) -> Vec<PathBuf> {
    let mut parts: Vec<&str> = doc_path
        .unwrap_or_default()
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    parts.pop(); // the file name
    if parts.iter().any(|p| *p == ".." || p.contains('\\'))
        || doc_path.is_some_and(|p| p.starts_with('/'))
    {
        parts.clear();
    }
    let mut dirs = Vec::with_capacity(parts.len() + 1);
    while !parts.is_empty() {
        dirs.push(parts.iter().fold(project.to_path_buf(), |d, p| d.join(p)));
        parts.pop();
    }
    dirs.push(project.to_path_buf());
    dirs
}

/// The contents of the first `name` found in `dirs`.
fn nearest(dirs: &[PathBuf], name: &str) -> Option<String> {
    dirs.iter().find_map(|d| read_capped(&d.join(name)))
}

/// A file's text, if it exists and is at most [`MAX_FILE_BYTES`].
fn read_capped(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The selected environment's name, from project-scope kv.
pub fn selected() -> Option<String> {
    kv::get(kv::Scope::Project, SELECTED_KEY)
        .and_then(|v| String::from_utf8(v).ok())
        .filter(|s| !s.is_empty())
}

/// Remembers (or, with `None`, forgets) the selected environment.
pub fn select(name: Option<&str>) -> Result<(), String> {
    match name {
        Some(n) => kv::set(kv::Scope::Project, SELECTED_KEY, n.as_bytes())
            .map_err(|e| format!("cannot save the environment: {e:?}")),
        None => {
            kv::delete(kv::Scope::Project, SELECTED_KEY);
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;
