//! Small path helpers shared by the discovery code.

use std::path::{Component, Path, PathBuf};

/// Maximum number of symlinks followed while resolving one path, like the
/// OS limit that yields `ELOOP`.
const MAX_SYMLINKS: u32 = 40;

/// Resolves `target` against `base` when it is relative, then removes `.`
/// and `..` components the way the OS would (see [`resolve_dots`]).
///
/// This mirrors git, which joins a relative `gitdir:` or `commondir` onto
/// its base and lets the OS resolve the result.
pub(crate) fn resolve(base: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        resolve_dots(target)
    } else {
        resolve_dots(&base.join(target))
    }
}

/// Removes `.` and `..` components with the OS's semantics while keeping
/// the path's visible form where possible.
///
/// On Unix, `x/..` names the parent of whatever `x` points to, so when the
/// component before a `..` is a symlink it is replaced by its target first.
/// Paths without symlinks before a `..` come out exactly as lexical
/// normalization would produce them (no canonicalization, no `/private`
/// prefixes on macOS). On Windows the OS folds `..` lexically, and so does
/// this function. Leading `..` of a relative path are kept; an empty result
/// becomes `.`.
pub(crate) fn resolve_dots(path: &Path) -> PathBuf {
    let mut budget = MAX_SYMLINKS;
    let out = resolve_dots_with(path, &mut budget);
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

fn resolve_dots_with(path: &Path, budget: &mut u32) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out = follow_symlinks(out, budget);
                pop_lexically(&mut out);
            }
            other => out.push(other),
        }
    }
    out
}

/// While the last component of `path` is a symlink, replaces it with the
/// link's target (resolved against the link's directory).
fn follow_symlinks(mut path: PathBuf, budget: &mut u32) -> PathBuf {
    while *budget > 0 {
        if !matches!(path.components().next_back(), Some(Component::Normal(_))) {
            break;
        }
        let Some(target) = symlink_target(&path) else {
            break;
        };
        *budget -= 1;
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        path = resolve_dots_with(&parent.join(target), budget);
    }
    path
}

/// The target of `path` if it is a symlink.
#[cfg(unix)]
fn symlink_target(path: &Path) -> Option<PathBuf> {
    std::fs::read_link(path).ok()
}

/// Windows resolves `..` lexically, before any reparse point is followed.
#[cfg(not(unix))]
fn symlink_target(_path: &Path) -> Option<PathBuf> {
    None
}

/// Applies one `..` lexically: drops a trailing normal component, keeps a
/// root or prefix, and otherwise appends `..` (leading `..` of a relative
/// path).
fn pop_lexically(path: &mut PathBuf) {
    match path.components().next_back() {
        Some(Component::Normal(_)) => {
            path.pop();
        }
        // `..` at the root stays at the root.
        Some(Component::RootDir | Component::Prefix(_)) => {}
        _ => path.push(Component::ParentDir),
    }
}

/// Makes `path` absolute (relative to the current directory) and resolves
/// `.`/`..` with [`resolve_dots`]. Falls back to the input when the current
/// directory is unavailable.
pub(crate) fn absolute(path: &Path) -> PathBuf {
    match std::path::absolute(path) {
        Ok(abs) => resolve_dots(&abs),
        Err(_) => resolve_dots(path),
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
    fn resolve_dots_folds_dots_lexically_without_symlinks() {
        // Paths that do not exist cannot contain symlinks.
        assert_eq!(
            resolve_dots(Path::new("/arcode-missing/b/./c/../d")),
            PathBuf::from("/arcode-missing/b/d")
        );
        assert_eq!(resolve_dots(Path::new("a/../..")), PathBuf::from(".."));
        assert_eq!(resolve_dots(Path::new("./")), PathBuf::from("."));
        assert_eq!(resolve_dots(Path::new("/../x")), PathBuf::from("/x"));
    }

    #[test]
    fn resolve_relative_and_absolute() {
        let base = Path::new("/repo/sub");
        assert_eq!(
            resolve(base, Path::new("../.git/worktrees/w")),
            PathBuf::from("/repo/.git/worktrees/w")
        );
        let abs = std::path::absolute("/elsewhere/x").unwrap();
        assert_eq!(resolve(base, &abs), resolve_dots(&abs));
    }

    #[test]
    fn git_path_uses_forward_slashes() {
        let rel: PathBuf = ["src", "main.rs"].iter().collect();
        assert_eq!(to_git_path(&rel).as_deref(), Some("src/main.rs"));
        assert_eq!(to_git_path(Path::new("./a/./b")).as_deref(), Some("a/b"));
        assert_eq!(to_git_path(Path::new("../x")), None);
        assert_eq!(to_git_path(Path::new("")), None);
    }

    #[test]
    fn resolve_dots_keeps_real_directories_as_written() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        let path = tmp.path().join("a/./b/../c");
        assert_eq!(resolve_dots(&path), tmp.path().join("a/c"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_dots_follows_symlink_before_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let deep = root.join("x").join("y").join("z");
        std::fs::create_dir_all(&deep).unwrap();
        // Absolute and relative link targets.
        std::os::unix::fs::symlink(&deep, root.join("abs")).unwrap();
        std::os::unix::fs::symlink("x/y/z", root.join("rel")).unwrap();
        // A chain: link -> link -> dir.
        std::os::unix::fs::symlink("rel", root.join("chain")).unwrap();
        for link in ["abs", "rel", "chain"] {
            assert_eq!(
                resolve_dots(&root.join(link).join("../q")),
                root.join("x/y/q"),
                "{link}"
            );
        }
        // Components after the last `..` keep their form.
        assert_eq!(resolve_dots(&root.join("abs/k")), root.join("abs/k"));
        assert_eq!(
            resolve(&root.join("abs"), Path::new("../../w")),
            root.join("x/w")
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_dots_survives_symlink_loops() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::os::unix::fs::symlink("b", root.join("a")).unwrap();
        std::os::unix::fs::symlink("a", root.join("b")).unwrap();
        // Must terminate; the exact result is unspecified for a loop.
        let _ = resolve_dots(&root.join("a/../c"));
    }
}
