//! [`SearchQuery::includes_path`]: whether a search would visit a file, for
//! callers that search text that is not on disk yet (an editor buffer with
//! unsaved changes).

use std::path::Path;

use ignore::Match;

use crate::query::SearchQuery;
use crate::walk::build_overrides;

impl SearchQuery {
    /// Whether a search of `root` would look at the file `path` as far as
    /// the include and exclude globs and the hidden-file setting go (a
    /// folder excluded on the way to it excludes it). `.gitignore` rules
    /// and the size limit are not consulted. Paths outside `root` and
    /// invalid globs give `false`.
    pub fn includes_path(&self, root: &Path, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(root) else {
            return false;
        };
        let names: Vec<_> = relative.components().map(|c| c.as_os_str()).collect();
        if names.is_empty() {
            return false;
        }
        if !self.include_hidden && names.iter().any(|n| n.to_string_lossy().starts_with('.')) {
            return false;
        }
        let Ok(overrides) = build_overrides(root, &self.walk_config()) else {
            return false;
        };
        let mut at = root.to_path_buf();
        for (ix, name) in names.iter().enumerate() {
            at.push(name);
            let is_dir = ix + 1 < names.len();
            if let Match::Ignore(_) = overrides.matched(&at, is_dir) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_and_hidden_files_decide() {
        let root = Path::new("/r");
        let q = |include: &[&str], exclude: &[&str]| SearchQuery {
            include_globs: include.iter().map(|g| (*g).to_owned()).collect(),
            exclude_globs: exclude.iter().map(|g| (*g).to_owned()).collect(),
            ..SearchQuery::new("x")
        };
        let all = q(&[], &[]);
        assert!(all.includes_path(root, Path::new("/r/src/a.rs")));
        assert!(!all.includes_path(root, Path::new("/r/.cache/a.rs")));
        assert!(!all.includes_path(root, Path::new("/elsewhere/a.rs")));
        let hidden = SearchQuery {
            include_hidden: true,
            ..all.clone()
        };
        assert!(hidden.includes_path(root, Path::new("/r/.cache/a.rs")));
        let rs = q(&["*.rs"], &["target"]);
        assert!(rs.includes_path(root, Path::new("/r/src/a.rs")));
        assert!(!rs.includes_path(root, Path::new("/r/src/a.txt")));
        assert!(!rs.includes_path(root, Path::new("/r/target/debug/a.rs")));
        let src = q(&["src"], &[]);
        assert!(src.includes_path(root, Path::new("/r/src/deep/a.txt")));
        assert!(!src.includes_path(root, Path::new("/r/lib/a.txt")));
    }
}
