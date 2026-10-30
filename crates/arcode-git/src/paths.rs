//! Small path helpers shared by the discovery code.

use std::path::{Component, Path, PathBuf};

/// Resolves `target` against `base` when it is relative, then normalizes it
/// lexically (removing `.` and resolving `..` without touching the disk).
///
/// Lexical normalization is used instead of `canonicalize` so paths keep the
/// form the user sees (no `\\?\` prefixes on Windows, symlinks preserved).
pub(crate) fn resolve(base: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        normalize(target)
    } else {
        normalize(&base.join(target))
    }
}

/// Lexically normalizes a path: drops `.` components and folds `..` into the
/// preceding normal component. Leading `..` of a relative path are kept.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                // `..` at the root stays at the root.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(component),
            },
            other => out.push(other),
        }
    }
    if out.is_empty() {
        return PathBuf::from(".");
    }
    out.iter().map(|c| c.as_os_str()).collect()
}

/// Makes `path` absolute (relative to the current directory) and normalizes
/// it. Falls back to the normalized input when the current directory is
/// unavailable.
pub(crate) fn absolute(path: &Path) -> PathBuf {
    match std::path::absolute(path) {
        Ok(abs) => normalize(&abs),
        Err(_) => normalize(path),
    }
}

/// Converts a repository-relative path into git's `/`-separated form.
///
/// Returns `None` if the path is empty or escapes the repository (`..`, a
/// root or a drive prefix).
pub(crate) fn to_git_path(rel: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_folds_dots() {
        assert_eq!(
            normalize(Path::new("/a/b/./c/../d")),
            PathBuf::from("/a/b/d")
        );
        assert_eq!(normalize(Path::new("a/../..")), PathBuf::from(".."));
        assert_eq!(normalize(Path::new("./")), PathBuf::from("."));
        assert_eq!(normalize(Path::new("/../x")), PathBuf::from("/x"));
    }

    #[test]
    fn resolve_relative_and_absolute() {
        let base = Path::new("/repo/sub");
        assert_eq!(
            resolve(base, Path::new("../.git/worktrees/w")),
            PathBuf::from("/repo/.git/worktrees/w")
        );
        let abs = std::path::absolute("/elsewhere/x").unwrap();
        assert_eq!(resolve(base, &abs), normalize(&abs));
    }

    #[test]
    fn git_path_uses_forward_slashes() {
        let rel: PathBuf = ["src", "main.rs"].iter().collect();
        assert_eq!(to_git_path(&rel).as_deref(), Some("src/main.rs"));
        assert_eq!(to_git_path(Path::new("./a/./b")).as_deref(), Some("a/b"));
        assert_eq!(to_git_path(Path::new("../x")), None);
        assert_eq!(to_git_path(Path::new("")), None);
    }
}
