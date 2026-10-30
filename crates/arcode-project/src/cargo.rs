//! Reading `Cargo.toml`: packages, binary targets, workspace members.

use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::fsutil;

/// A Cargo package and its binary targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CargoPackage {
    pub name: String,
    pub dir: PathBuf,
    /// Binary target names with the source file of each (relative path).
    pub bins: Vec<(String, String)>,
    pub default_run: Option<String>,
}

impl CargoPackage {
    /// The binary whose source is `rel` (a `/`-separated path), if any.
    pub fn bin_for_path(&self, rel: &str) -> Option<&str> {
        self.bins
            .iter()
            .find(|(_, p)| p.trim_start_matches("./") == rel)
            .map(|(n, _)| n.as_str())
    }
}

/// Reads the package in `dir`; `None` for virtual manifests or bad files.
pub(crate) fn read_package(dir: &Path) -> Option<CargoPackage> {
    let table = fsutil::read_toml(&dir.join("Cargo.toml"))?;
    package_from_table(dir, &table)
}

fn package_from_table(dir: &Path, table: &Table) -> Option<CargoPackage> {
    let package = table.get("package")?.as_table()?;
    let name = package.get("name")?.as_str()?.to_string();
    let default_run = package
        .get("default-run")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut bins: Vec<(String, String)> = Vec::new();
    for bin in table
        .get("bin")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(bin_name) = bin.get("name").and_then(Value::as_str) else {
            continue;
        };
        let path = bin
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("src/bin/{bin_name}.rs"));
        bins.push((bin_name.to_string(), path));
    }
    let autobins = package
        .get("autobins")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if autobins {
        for (bin_name, path) in auto_bins(dir, &name) {
            let taken = bins.iter().any(|(n, p)| *n == bin_name || *p == path);
            if !taken {
                bins.push((bin_name, path));
            }
        }
    }
    Some(CargoPackage {
        name,
        dir: dir.to_path_buf(),
        bins,
        default_run,
    })
}

/// `src/main.rs`, `src/bin/*.rs` and `src/bin/*/main.rs`.
fn auto_bins(dir: &Path, package: &str) -> Vec<(String, String)> {
    let mut bins = Vec::new();
    if dir.join("src/main.rs").is_file() {
        bins.push((package.to_string(), "src/main.rs".to_string()));
    }
    let bin_dir = dir.join("src").join("bin");
    for entry in fsutil::list_dir(&bin_dir, |_| true) {
        let path = bin_dir.join(&entry);
        if let Some(stem) = entry.strip_suffix(".rs")
            && path.is_file()
        {
            bins.push((stem.to_string(), format!("src/bin/{entry}")));
        } else if path.join("main.rs").is_file() {
            bins.push((entry.clone(), format!("src/bin/{entry}/main.rs")));
        }
    }
    bins
}

/// A Cargo workspace (or single package) rooted at a directory.
#[derive(Debug, Clone, Default)]
pub(crate) struct CargoProject {
    /// Whether the root manifest has a `[workspace]` table.
    pub is_workspace: bool,
    /// The root package (if not virtual), then members in manifest order.
    pub packages: Vec<CargoPackage>,
}

/// Reads the Cargo project rooted at `root`; `None` without a `Cargo.toml`.
pub(crate) fn read_project(root: &Path) -> Option<CargoProject> {
    let table = fsutil::read_toml(&root.join("Cargo.toml"))?;
    let mut project = CargoProject::default();
    if let Some(pkg) = package_from_table(root, &table) {
        project.packages.push(pkg);
    }
    if let Some(ws) = table.get("workspace").and_then(Value::as_table) {
        project.is_workspace = true;
        let strings = |key: &str| -> Vec<String> {
            ws.get(key)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        let exclude: Vec<PathBuf> = strings("exclude").iter().map(|e| root.join(e)).collect();
        for member in strings("members") {
            for dir in expand_member(root, &member) {
                let known = project.packages.iter().any(|p| p.dir == dir);
                if known || exclude.contains(&dir) {
                    continue;
                }
                if let Some(pkg) = read_package(&dir) {
                    project.packages.push(pkg);
                }
            }
        }
    }
    Some(project)
}

/// Expands a member pattern; supports `*` and `?` in the last component.
fn expand_member(root: &Path, member: &str) -> Vec<PathBuf> {
    let member = member.trim_end_matches('/');
    let (parent, last) = match member.rsplit_once('/') {
        Some((p, l)) => (root.join(p), l),
        None => (root.to_path_buf(), member),
    };
    if !last.contains(['*', '?']) {
        return vec![parent.join(last)];
    }
    fsutil::list_dir(&parent, |p| p.join("Cargo.toml").is_file())
        .into_iter()
        .filter(|name| wildcard_match(last, name))
        .map(|name| parent.join(name))
        .collect()
}

/// Glob match with `*` (any run) and `?` (one char).
pub(crate) fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// The nearest package containing `start` (walking up to `boundary`).
pub(crate) fn package_for(start: &Path, boundary: Option<&Path>) -> Option<CargoPackage> {
    fsutil::ancestors(start, boundary)
        .iter()
        .filter(|d| d.join("Cargo.toml").is_file())
        .find_map(|d| read_package(d))
}

#[cfg(test)]
mod tests {
    use super::wildcard_match;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("arcode-*", "arcode-buffer"));
        assert!(!wildcard_match("arcode-*", "other"));
        assert!(wildcard_match("a?c", "abc"));
        assert!(wildcard_match("*-x*", "a-b-xy"));
    }
}
