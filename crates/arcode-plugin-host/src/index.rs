//! A [`ProjectIndex`] that walks the project tree (honouring `.gitignore`,
//! `.ignore` and `.git/info/exclude`) on each `find-files` call.
//!
//! The app may plug its own file index in instead; this one serves headless
//! use, tests, and the app until it has one. It never follows symlinks,
//! never leaves the root, skips `.git`, returns paths sorted and relative
//! with `/` separators, and stops after [`MAX_VISITED`] entries so a huge
//! tree cannot stall a plugin call.

use std::path::{Path, PathBuf};

use crate::bridge::ProjectIndex;

/// Directory entries visited per call at most.
pub const MAX_VISITED: usize = 500_000;

/// Walks `root` on demand.
#[derive(Debug, Clone)]
pub struct WalkIndex {
    root: PathBuf,
}

impl WalkIndex {
    /// An index of the project at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

/// `path` relative to `root` with `/` separators; `None` if not UTF-8.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

impl ProjectIndex for WalkIndex {
    fn find_files(&self, matcher: &globset::GlobMatcher, limit: usize) -> Vec<String> {
        let walk = ignore::WalkBuilder::new(&self.root)
            .hidden(false)
            .parents(false)
            .follow_links(false)
            .require_git(false)
            .git_global(false)
            .sort_by_file_name(|a, b| a.cmp(b))
            .filter_entry(|e| e.file_name() != ".git")
            .build();
        let mut found = Vec::new();
        for entry in walk.take(MAX_VISITED) {
            if found.len() >= limit {
                break;
            }
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            if let Some(rel) = relative(&self.root, entry.path())
                && matcher.is_match(&rel)
            {
                found.push(rel);
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globset::GlobBuilder;

    fn glob(g: &str) -> globset::GlobMatcher {
        GlobBuilder::new(g)
            .literal_separator(true)
            .build()
            .unwrap()
            .compile_matcher()
    }

    #[test]
    fn honours_gitignore_and_skips_dot_git() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for f in [
            "a.db",
            "data/b.sqlite",
            "data/nested/c.sqlite3",
            "target/big.db",
            "data/skip.db",
            ".git/objects/x.db",
            ".hidden/d.db",
            "notes.txt",
        ] {
            let path = root.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(root.join("data/.gitignore"), "skip.db\n").unwrap();
        let index = WalkIndex::new(root);
        let found = index.find_files(&glob("**/*.{sqlite,sqlite3,db}"), 100);
        assert_eq!(
            found,
            [
                ".hidden/d.db",
                "a.db",
                "data/b.sqlite",
                "data/nested/c.sqlite3"
            ]
        );
        assert_eq!(index.find_files(&glob("*.db"), 100), ["a.db"]);
        assert_eq!(index.find_files(&glob("**/*"), 2).len(), 2);
        assert!(index.find_files(&glob("**/*.rs"), 10).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn never_follows_symlinks_out_of_the_project() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.db"), "x").unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.db"),
            dir.path().join("file-link.db"),
        )
        .unwrap();
        let found = WalkIndex::new(dir.path()).find_files(&glob("**/*.db"), 10);
        assert!(found.is_empty(), "{found:?}");
    }
}
