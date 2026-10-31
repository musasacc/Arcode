//! [`IgnoreFilter`]: the `.gitignore` rules of a project, for leaving
//! ignored build output (`dist/`, `build/`, `.next/`) out of the changed
//! paths the watcher hands to per-file features such as the TODO list.
//! Where the whole root is watched recursively (macOS, Windows) a build
//! would otherwise flood them and trigger full rescans.

use std::path::{Path, PathBuf};

use arcode_git::Repo;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// The root `.gitignore` files of the project and of its repositories,
/// plus each repository's `info/exclude`. Nested `.gitignore` files are
/// not read (they would need a walk of the tree).
pub struct IgnoreFilter {
    rules: Vec<Gitignore>,
}

impl IgnoreFilter {
    /// Reads the ignore files of `root` and `repos` (missing ones are
    /// skipped).
    pub fn new(root: &Path, repos: &[(PathBuf, PathBuf)]) -> Self {
        let mut rules = Vec::new();
        let mut add = |base: &Path, files: &[PathBuf]| {
            let mut builder = GitignoreBuilder::new(base);
            let mut any = false;
            for file in files.iter().filter(|f| f.is_file()) {
                any |= builder.add(file).is_none();
            }
            if any && let Ok(gi) = builder.build() {
                rules.push(gi);
            }
        };
        add(root, &[root.join(".gitignore")]);
        for (workdir, git_dir) in repos {
            if workdir != root {
                add(workdir, &[workdir.join(".gitignore")]);
            }
            add(workdir, &[git_dir.join("info").join("exclude")]);
        }
        Self { rules }
    }

    /// The (work dir, git dir) pairs of `repos`, for [`IgnoreFilter::new`].
    pub fn repo_dirs(repos: &[Repo]) -> Vec<(PathBuf, PathBuf)> {
        repos
            .iter()
            .map(|r| (r.workdir.clone(), r.common_dir.clone()))
            .collect()
    }

    /// Whether `path` (or a folder above it) is ignored.
    pub fn is_ignored(&self, path: &Path) -> bool {
        let is_dir = path.is_dir();
        self.rules.iter().any(|gi| {
            path.starts_with(gi.path()) && gi.matched_path_or_any_parents(path, is_dir).is_ignore()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_folders_and_files_are_recognised() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join(".gitignore"), "dist/\n*.log\n!keep.log\n").unwrap();
        std::fs::create_dir_all(root.join("dist/js")).unwrap();
        let filter = IgnoreFilter::new(&root, &[]);
        assert!(filter.is_ignored(&root.join("dist/js/app.js")));
        assert!(filter.is_ignored(&root.join("dist")));
        assert!(filter.is_ignored(&root.join("src/debug.log")));
        assert!(!filter.is_ignored(&root.join("src/keep.log")));
        assert!(!filter.is_ignored(&root.join("src/main.rs")));
        assert!(!filter.is_ignored(Path::new("/elsewhere/dist/x")));
        let none = IgnoreFilter::new(&root.join("dist"), &[]);
        assert!(!none.is_ignored(&root.join("dist/js/app.js")));
    }
}
