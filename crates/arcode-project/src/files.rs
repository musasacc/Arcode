//! File operations behind the file tree: create, rename, delete (to the
//! trash when the platform has one) and showing a file in the system file
//! manager.
//!
//! Everything here blocks (file system, helper processes); call it off the
//! UI thread. Nothing touches the network.

use std::io;
use std::path::{Path, PathBuf};

pub use crate::trash::{TrashError, move_to_trash, trash_into};

/// Checks a file or folder name typed by the user: not empty, not `.` or
/// `..`, no path separators and no characters Windows rejects.
pub fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("A name is required.".into());
    }
    if trimmed == "." || trimmed == ".." {
        return Err(format!("`{trimmed}` is not a valid name."));
    }
    if let Some(c) = name.chars().find(|c| {
        matches!(
            c,
            '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        )
    }) {
        return Err(format!("A name cannot contain `{c}`."));
    }
    if name.chars().any(char::is_control) {
        return Err("A name cannot contain control characters.".into());
    }
    if name != trimmed {
        return Err("A name cannot start or end with spaces.".into());
    }
    Ok(())
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Creates an empty file `name` in `dir`; fails if it exists.
pub fn create_file(dir: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name).map_err(invalid)?;
    let path = dir.join(name);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok(path)
}

/// Creates the folder `name` in `dir`; fails if it exists.
pub fn create_dir(dir: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name).map_err(invalid)?;
    let path = dir.join(name);
    std::fs::create_dir(&path)?;
    Ok(path)
}

/// Renames `from` to `new_name` in the same folder. Refuses to replace an
/// existing entry. A change of letter case only is allowed when the
/// "existing" entry is `from` itself (a case-insensitive file system);
/// on a case-sensitive one `A.txt` next to `a.txt` is another file and the
/// rename is refused.
pub fn rename(from: &Path, new_name: &str) -> io::Result<PathBuf> {
    validate_name(new_name).map_err(invalid)?;
    let parent = from
        .parent()
        .ok_or_else(|| invalid("cannot rename a root folder".into()))?;
    let to = parent.join(new_name);
    if to == from {
        return Ok(to);
    }
    if to.symlink_metadata().is_ok() && !is_case_change_of_same_entry(from, parent, new_name) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("`{new_name}` already exists"),
        ));
    }
    std::fs::rename(from, &to)?;
    Ok(to)
}

/// Whether `parent/new_name` exists only because the file system ignores
/// letter case, so it is `from` itself: the names differ only in case and
/// the folder has no entry spelled exactly `new_name`. On a case-sensitive
/// file system `a.txt` and `A.txt` are two files, and the exact-case entry
/// is listed, so renaming onto it is refused instead of replacing it.
fn is_case_change_of_same_entry(from: &Path, parent: &Path, new_name: &str) -> bool {
    let case_only = from
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|old| old != new_name && old.to_lowercase() == new_name.to_lowercase());
    if !case_only {
        return false;
    }
    match std::fs::read_dir(parent) {
        Ok(entries) => !entries
            .flatten()
            .any(|entry| entry.file_name() == std::ffi::OsStr::new(new_name)),
        Err(_) => false,
    }
}

/// Moves `from` into the folder `dir`, keeping its name. Refuses to replace
/// an existing entry or to move a folder into itself.
pub fn move_into(from: &Path, dir: &Path) -> io::Result<PathBuf> {
    let name = from
        .file_name()
        .ok_or_else(|| invalid("nothing to move".into()))?;
    if dir.starts_with(from) {
        return Err(invalid("cannot move a folder into itself".into()));
    }
    let to = dir.join(name);
    if to == from {
        return Ok(to);
    }
    if to.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("`{}` already exists there", name.to_string_lossy()),
        ));
    }
    std::fs::rename(from, &to)?;
    Ok(to)
}

/// Deletes a file or a whole folder without the trash.
pub fn delete_permanently(path: &Path) -> io::Result<()> {
    let meta = path.symlink_metadata()?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// `path` after a rename or move of `from` to `to`: unchanged unless it is
/// `from` or inside it.
pub fn renamed_path(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(rest)
    })
}

/// A `file://` URI for an absolute path (percent-encoded).
pub fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            b':' if cfg!(windows) => uri.push(':'),
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    uri
}

/// `path` as other programs expect it: on Windows the verbatim prefix that
/// `std::fs::canonicalize` adds (`\\?\C:\...`, `\\?\UNC\server\...`) is
/// removed, since Explorer, PowerShell and `cmd.exe` do not accept it.
/// Elsewhere (and for paths without the prefix) it is returned unchanged.
pub fn external_path(path: &Path) -> PathBuf {
    match strip_verbatim(&path.to_string_lossy()) {
        Some(plain) if cfg!(windows) => PathBuf::from(plain),
        _ => path.to_path_buf(),
    }
}

/// The path text without a Windows verbatim prefix, when it had one that
/// can be dropped safely (a drive letter or a UNC share).
fn strip_verbatim(text: &str) -> Option<String> {
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{rest}"));
    }
    let rest = text.strip_prefix(r"\\?\")?;
    let bytes = rest.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    // Verbatim paths may hold names Win32 cannot (e.g. a trailing dot or
    // over 260 characters); keep those as they are.
    let plain_ok = rest.len() < 260
        && !rest
            .split('\\')
            .any(|c| c.ends_with('.') && c != "." && c != "..");
    (drive && plain_ok).then(|| rest.to_owned())
}

/// Shows `path` selected in the system file manager (Finder, Explorer, or
/// a FreeDesktop file manager; else its folder is opened).
pub fn reveal_in_file_manager(path: &Path) -> io::Result<()> {
    reveal::reveal(&external_path(path))
}

mod reveal {
    use std::io;
    use std::path::Path;
    use std::process::Command;

    #[cfg(not(windows))]
    fn run(cmd: &mut Command) -> io::Result<()> {
        let status = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("exited with {status}")))
        }
    }

    #[cfg(target_os = "macos")]
    pub fn reveal(path: &Path) -> io::Result<()> {
        run(Command::new("open").arg("-R").arg(path))
    }

    #[cfg(windows)]
    pub fn reveal(path: &Path) -> io::Result<()> {
        use std::os::windows::process::CommandExt;
        // Explorer wants `/select,"path"` as one raw argument, and it exits
        // with 1 even on success, so only spawning is checked.
        Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(|_| ())
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn reveal(path: &Path) -> io::Result<()> {
        let uri = super::file_uri(path);
        let shown = run(Command::new("dbus-send")
            .arg("--session")
            .arg("--print-reply")
            .arg("--dest=org.freedesktop.FileManager1")
            .arg("--type=method_call")
            .arg("/org/freedesktop/FileManager1")
            .arg("org.freedesktop.FileManager1.ShowItems")
            .arg(format!("array:string:{uri}"))
            .arg("string:"));
        if shown.is_ok() {
            return Ok(());
        }
        let folder = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        run(Command::new("xdg-open").arg(folder))
    }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
