//! Small filesystem helpers.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes `contents` to `path` atomically: the data goes to a temporary file
/// in the same directory, is flushed to disk, then renamed over `path`.
/// Parent directories are created as needed. When `path` is a symbolic link
/// (a dotfiles checkout), the file it points to is replaced and the link
/// stays.
pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let resolved = resolve_links(path)?;
    let path = resolved.as_path();
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a file path: {}", path.display()),
        )
    })?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;

    let unique = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(
        ".{}.{}.{unique}.tmp",
        file_name.to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        // `fs::rename` replaces an existing destination on all platforms
        // (MoveFileExW with MOVEFILE_REPLACE_EXISTING on Windows).
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Most links followed before giving up (a loop).
const MAX_LINKS: usize = 32;

/// The file `path` names after following symbolic links (the final target
/// may not exist yet). A path that is not a link is returned as it is.
fn resolve_links(path: &Path) -> io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_LINKS {
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = fs::read_link(&current)?;
                current = match current.parent() {
                    Some(dir) if target.is_relative() => dir.join(target),
                    _ => target,
                };
            }
            _ => return Ok(current),
        }
    }
    Err(io::Error::other(format!(
        "too many symbolic links: {}",
        path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces_without_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deeper/file.toml");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let entries: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(entries.len(), 1, "temporary files must not remain");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_kept_and_its_target_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let dotfiles = dir.path().join("dotfiles");
        fs::create_dir(&dotfiles).unwrap();
        let real = dotfiles.join("config.toml");
        fs::write(&real, "old").unwrap();
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink("dotfiles/config.toml", &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
        let looped = dir.path().join("loop");
        std::os::unix::fs::symlink(&looped, &looped).unwrap();
        assert!(write_atomic(&looped, b"x").is_err());
    }

    #[test]
    fn rejects_paths_without_file_name() {
        assert!(write_atomic(Path::new("/"), b"x").is_err());
    }
}
