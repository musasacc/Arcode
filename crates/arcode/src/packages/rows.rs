//! Rows of the Packages view: a header per manifest and its dependencies,
//! filtered by name.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use arcode_project::deps::{Dependency, ProjectManifest};

/// One list row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// The header of manifest `ix`.
    Section(usize),
    /// Dependency `dep` of manifest `section`.
    Dep {
        /// Manifest index.
        section: usize,
        /// Dependency index.
        dep: usize,
    },
    /// A note under a header: an error, "No dependencies", "No matches".
    Note(usize),
}

/// Whether `dep` matches the filter text (case-insensitive, on its name,
/// registry name and group).
pub fn matches(dep: &Dependency, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    filter.is_empty()
        || dep.name.to_lowercase().contains(&filter)
        || dep.package.to_lowercase().contains(&filter)
        || dep
            .group
            .as_deref()
            .is_some_and(|g| g.to_lowercase().contains(&filter))
}

/// The rows for `manifests`: each header, then (unless collapsed) the
/// dependencies matching `filter`, or a note. While filtering, manifests
/// without a match are left out.
pub fn build_rows(
    manifests: &[ProjectManifest],
    filter: &str,
    collapsed: &HashSet<PathBuf>,
) -> Vec<Row> {
    let filtering = !filter.trim().is_empty();
    let mut rows = Vec::new();
    for (section, m) in manifests.iter().enumerate() {
        let shown: Vec<usize> = (0..m.deps.len())
            .filter(|&i| matches(&m.deps[i], filter))
            .collect();
        if filtering && shown.is_empty() {
            continue;
        }
        rows.push(Row::Section(section));
        if collapsed.contains(&m.path) {
            continue;
        }
        if m.error.is_some() || shown.is_empty() {
            rows.push(Row::Note(section));
        }
        rows.extend(shown.into_iter().map(|dep| Row::Dep { section, dep }));
    }
    rows
}

/// The folder of `manifest` relative to `root` (`""` at the root), `/`
/// separated.
pub fn relative_dir(root: Option<&Path>, manifest: &Path) -> String {
    let dir = manifest.parent().unwrap_or(manifest);
    let rel = root.and_then(|r| dir.strip_prefix(r).ok()).unwrap_or(dir);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The manifest an action without a target applies to: the innermost one
/// whose folder contains `hint` (the active file), else the first.
pub fn target_manifest<'a>(
    manifests: &'a [ProjectManifest],
    hint: Option<&Path>,
) -> Option<&'a ProjectManifest> {
    hint.and_then(|h| {
        manifests
            .iter()
            .filter(|m| h.starts_with(&m.dir))
            .max_by_key(|m| m.dir.components().count())
    })
    .or_else(|| manifests.first())
}

/// The version text of a row: `requested → locked`, or one of them.
pub fn version_text(dep: &Dependency) -> String {
    match (dep.requested.as_str(), dep.locked.as_deref()) {
        ("", Some(locked)) => locked.to_string(),
        (req, Some(locked)) if req.trim_start_matches(['=', 'v']) == locked => locked.to_string(),
        (req, Some(locked)) => format!("{req} → {locked}"),
        (req, None) => req.to_string(),
    }
}

/// Whether `latest` (from a check) is newer than what the row shows.
pub fn is_newer(dep: &Dependency, latest: &str) -> bool {
    let current = dep.locked.as_deref().unwrap_or(&dep.requested);
    current.trim_start_matches('v') != latest.trim_start_matches('v')
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_project::deps::{DepKind, Manager};

    fn manifest(path: &str, names: &[&str]) -> ProjectManifest {
        let mut m = ProjectManifest::new(Manager::Cargo, PathBuf::from(path));
        m.deps = names
            .iter()
            .map(|n| Dependency::new(n, "1", DepKind::Normal))
            .collect();
        m
    }

    #[test]
    fn rows_filter_collapse_and_notes() {
        let ms = vec![
            manifest("/p/Cargo.toml", &["serde", "anyhow"]),
            manifest("/p/a/Cargo.toml", &[]),
            manifest("/p/b/Cargo.toml", &["serde_json"]),
        ];
        let none = HashSet::new();
        assert_eq!(
            build_rows(&ms, "", &none),
            vec![
                Row::Section(0),
                Row::Dep { section: 0, dep: 0 },
                Row::Dep { section: 0, dep: 1 },
                Row::Section(1),
                Row::Note(1),
                Row::Section(2),
                Row::Dep { section: 2, dep: 0 },
            ]
        );
        assert_eq!(
            build_rows(&ms, " SERDE", &none),
            vec![
                Row::Section(0),
                Row::Dep { section: 0, dep: 0 },
                Row::Section(2),
                Row::Dep { section: 2, dep: 0 },
            ]
        );
        let collapsed: HashSet<PathBuf> = [PathBuf::from("/p/Cargo.toml")].into();
        assert_eq!(
            build_rows(&ms, "", &collapsed)[..2],
            [Row::Section(0), Row::Section(1)]
        );
    }

    #[test]
    fn targets_and_labels() {
        let ms = vec![
            manifest("/p/Cargo.toml", &[]),
            manifest("/p/crates/a/Cargo.toml", &[]),
        ];
        let target =
            |hint: &str| target_manifest(&ms, Some(Path::new(hint))).map(|m| m.dir.clone());
        assert_eq!(target("/p/crates/a/src/lib.rs"), Some("/p/crates/a".into()));
        assert_eq!(target("/p/src/main.rs"), Some("/p".into()));
        assert_eq!(target("/elsewhere/x.rs"), Some("/p".into()));
        assert_eq!(
            relative_dir(Some(Path::new("/p")), Path::new("/p/crates/a/Cargo.toml")),
            "crates/a"
        );
        assert_eq!(
            relative_dir(Some(Path::new("/p")), Path::new("/p/Cargo.toml")),
            ""
        );

        let mut d = Dependency::new("x", "^1.2", DepKind::Normal);
        assert_eq!(version_text(&d), "^1.2");
        d.locked = Some("1.2.5".into());
        assert_eq!(version_text(&d), "^1.2 → 1.2.5");
        assert!(is_newer(&d, "1.3.0"));
        assert!(!is_newer(&d, "1.2.5"));
        d.requested = "=1.2.5".into();
        assert_eq!(version_text(&d), "1.2.5");
    }
}
