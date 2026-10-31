//! Resource operations of workspace edits on disk: create, rename and
//! delete files and folders.

use std::fs;
use std::path::Path;

/// Creates an empty file (and its folders). An existing file is kept when
/// `ignore_if_exists`, emptied when `overwrite`, an error otherwise.
pub(crate) fn create_file(
    path: &Path,
    overwrite: bool,
    ignore_if_exists: bool,
) -> Result<bool, String> {
    if path.exists() {
        if overwrite {
            return fs::write(path, "")
                .map(|_| true)
                .map_err(|e| format!("cannot overwrite {}: {e}", path.display()));
        }
        if ignore_if_exists {
            return Ok(false);
        }
        return Err(format!("{} already exists", path.display()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    fs::write(path, "")
        .map(|_| true)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))
}

/// Moves `from` to `to` (creating folders). An existing target is
/// replaced when `overwrite`, kept (nothing moves) when
/// `ignore_if_exists`, an error otherwise.
pub(crate) fn rename_path(
    from: &Path,
    to: &Path,
    overwrite: bool,
    ignore_if_exists: bool,
) -> Result<bool, String> {
    if to.exists() && !overwrite && !is_case_change(from, to) {
        if ignore_if_exists {
            return Ok(false);
        }
        return Err(format!("{} already exists", to.display()));
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    fs::rename(from, to)
        .map(|_| true)
        .map_err(|e| format!("cannot move {} to {}: {e}", from.display(), to.display()))
}

/// Whether `to` names `from` itself with a different letter case, as on a
/// case-insensitive filesystem (the macOS and Windows defaults), where
/// `to.exists()` is true for `foo.rs` -> `Foo.rs`. That is a rename, not a
/// clash with another file.
pub(crate) fn is_case_change(from: &Path, to: &Path) -> bool {
    let (a, b) = (from.to_string_lossy(), to.to_string_lossy());
    a != b && a.to_lowercase() == b.to_lowercase() && same_file(from, to)
}

/// Whether `a` and `b` are the same file on disk.
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(x), Ok(y)) = (fs::metadata(a), fs::metadata(b)) {
            return x.dev() == y.dev() && x.ino() == y.ino();
        }
    }
    // Elsewhere the canonical path carries the case stored on disk.
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// Deletes a file, or a folder (with its contents only when `recursive`).
/// A missing path is fine when `ignore_if_not_exists`.
pub(crate) fn delete_path(
    path: &Path,
    recursive: bool,
    ignore_if_not_exists: bool,
) -> Result<bool, String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) if ignore_if_not_exists => return Ok(false),
        Err(e) => return Err(format!("cannot delete {}: {e}", path.display())),
    };
    let result = if meta.is_dir() {
        if recursive {
            fs::remove_dir_all(path)
        } else {
            fs::remove_dir(path)
        }
    } else {
        fs::remove_file(path)
    };
    result
        .map(|_| true)
        .map_err(|e| format!("cannot delete {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_rename_delete_follow_their_options() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("sub/a.rs");
        assert_eq!(create_file(&a, false, false), Ok(true));
        assert!(a.exists());
        fs::write(&a, "x").unwrap();
        assert_eq!(create_file(&a, false, true), Ok(false));
        assert!(create_file(&a, false, false).is_err());
        assert_eq!(create_file(&a, true, false), Ok(true));
        assert_eq!(fs::read_to_string(&a).unwrap(), "");

        let b = dir.path().join("moved/b.rs");
        assert_eq!(rename_path(&a, &b, false, false), Ok(true));
        assert!(!a.exists() && b.exists());
        create_file(&a, false, false).unwrap();
        assert_eq!(rename_path(&a, &b, false, true), Ok(false));
        assert!(rename_path(&a, &b, false, false).is_err());
        assert_eq!(rename_path(&a, &b, true, false), Ok(true));

        let sub = dir.path().join("moved");
        assert!(delete_path(&sub, false, false).is_err(), "not empty");
        assert_eq!(delete_path(&sub, true, false), Ok(true));
        assert!(!sub.exists());
        assert_eq!(delete_path(&sub, false, true), Ok(false));
        assert!(delete_path(&sub, false, false).is_err());
    }

    #[test]
    fn a_case_only_rename_is_not_a_clash() {
        let dir = tempfile::tempdir().unwrap();
        let foo = dir.path().join("foo.rs");
        let upper = dir.path().join("Foo.rs");
        fs::write(&foo, "x").unwrap();
        let case_sensitive = !upper.exists();
        if case_sensitive {
            assert!(!is_case_change(&foo, &upper), "Foo.rs does not exist here");
            // On a case-insensitive filesystem `Foo.rs` is `foo.rs` itself;
            // a hard link reproduces that on a case-sensitive one (Unix).
            if cfg!(windows) || fs::hard_link(&foo, &upper).is_err() {
                return;
            }
        }
        assert!(is_case_change(&foo, &upper));
        assert_eq!(rename_path(&foo, &upper, false, false), Ok(true));
        assert_eq!(fs::read_to_string(&upper).unwrap(), "x");
        if case_sensitive {
            // A different file that only differs in case is still a clash.
            let other = dir.path().join("Bar.rs");
            let lower = dir.path().join("bar.rs");
            fs::write(&other, "1").unwrap();
            fs::write(&lower, "2").unwrap();
            assert!(rename_path(&lower, &other, false, false).is_err());
        }
    }
}
