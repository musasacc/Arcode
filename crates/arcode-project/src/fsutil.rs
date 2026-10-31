//! Small filesystem helpers shared by the detectors.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// Whether `dir` is the root of a version-controlled tree.
pub(crate) fn is_vcs_root(dir: &Path) -> bool {
    [".git", ".hg", ".jj", ".svn"]
        .iter()
        .any(|m| dir.join(m).exists())
}

/// `start` and its ancestors, innermost first.
///
/// The walk ends at `boundary` (inclusive) when `start` lies inside it;
/// otherwise at the first version-control root, or at the filesystem root.
pub(crate) fn ancestors(start: &Path, boundary: Option<&Path>) -> Vec<PathBuf> {
    let boundary = boundary.filter(|b| start.starts_with(b));
    let mut dirs = Vec::new();
    for dir in start.ancestors() {
        if dir.as_os_str().is_empty() {
            break;
        }
        dirs.push(dir.to_path_buf());
        match boundary {
            Some(b) if dir == b => break,
            None if is_vcs_root(dir) => break,
            _ => {}
        }
    }
    dirs
}

/// The outermost directory of [`ancestors`]: the project root to assume.
pub(crate) fn walk_root(start: &Path, boundary: Option<&Path>) -> PathBuf {
    ancestors(start, boundary)
        .pop()
        .unwrap_or_else(|| start.to_path_buf())
}

/// The directory to start a walk from: `path` itself if it is a directory,
/// else its parent.
pub(crate) fn start_dir(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    }
}

/// Largest config file [`read_toml`] and [`read_json`] read.
const MAX_CONFIG: u64 = 8 * 1024 * 1024;

/// The text of a regular file of at most [`MAX_CONFIG`] bytes. A FIFO or
/// a device (a symlink to `/dev/zero` in a cloned repository) would block
/// or never end, so special files are skipped.
fn read_config_text(path: &Path) -> Option<String> {
    use std::io::Read;
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_CONFIG {
        return None;
    }
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_CONFIG)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// Parses a TOML file; `None` if missing, invalid or not a regular file.
pub(crate) fn read_toml(path: &Path) -> Option<toml::Table> {
    let text = read_config_text(path)?;
    toml::from_str(&text).ok()
}

/// Parses a JSON file; `None` if missing, invalid or not a regular file.
pub(crate) fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = read_config_text(path)?;
    serde_json::from_str(&text).ok()
}

/// `path` relative to `base` with `/` separators (`.` when equal), or the
/// full path when it is not inside `base`.
pub(crate) fn rel_slash(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(rel) => {
            let parts: Vec<String> = rel
                .components()
                .filter_map(|c| match c {
                    Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect();
            if parts.is_empty() {
                ".".to_string()
            } else {
                parts.join("/")
            }
        }
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Sorted names of the entries in `dir` accepted by `keep`.
pub(crate) fn list_dir(dir: &Path, keep: impl Fn(&Path) -> bool) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| keep(&e.path()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancestors_stop_at_boundary_and_vcs_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let deep = root.join("a/b");
        fs::create_dir_all(&deep).expect("mkdir");
        let dirs = ancestors(&deep, Some(&root.join("a")));
        assert_eq!(dirs, vec![deep.clone(), root.join("a")]);

        fs::create_dir(root.join(".git")).expect("mkdir");
        assert_eq!(ancestors(&deep, None).last(), Some(&root.to_path_buf()));
        // A boundary that does not contain the start falls back to VCS.
        assert_eq!(walk_root(&deep, Some(Path::new("/elsewhere"))), root);
    }

    #[test]
    fn rel_slash_formats() {
        let base = Path::new("/p");
        assert_eq!(rel_slash(&base.join("a").join("b.py"), base), "a/b.py");
        assert_eq!(rel_slash(base, base), ".");
    }
}
