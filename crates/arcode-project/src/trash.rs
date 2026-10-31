//! Moving files to the trash.
//!
//! - Linux and other FreeDesktop systems: the home trash
//!   (`$XDG_DATA_HOME/Trash`, see the FreeDesktop trash specification),
//!   falling back to `gio trash` for files on other file systems.
//! - macOS: Finder (through `osascript`), so "Put Back" works.
//! - Windows: the Recycle Bin (through PowerShell and
//!   `Microsoft.VisualBasic.FileIO`).
//!
//! When none of these works the caller gets [`TrashError::Unavailable`] and
//! can offer a permanent delete instead.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Why a file could not be moved to the trash.
#[derive(Debug)]
pub enum TrashError {
    /// There is no usable trash for this file (no trash on the platform,
    /// another file system, the helper is missing). Deleting permanently
    /// may still work.
    Unavailable(String),
    /// The file itself could not be moved (permissions, it vanished).
    Io(io::Error),
}

impl std::fmt::Display for TrashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrashError::Unavailable(why) => write!(f, "the trash is unavailable: {why}"),
            TrashError::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for TrashError {}

/// Moves `path` (file or folder) to the platform's trash.
pub fn move_to_trash(path: &Path) -> Result<(), TrashError> {
    if path.symlink_metadata().is_err() {
        return Err(TrashError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} does not exist", path.display()),
        )));
    }
    platform::trash(path)
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::TrashError;

    pub fn trash(path: &Path) -> Result<(), TrashError> {
        let output = Command::new("osascript")
            .args(["-e", "on run argv"])
            .args([
                "-e",
                "tell application \"Finder\" to delete (POSIX file (item 1 of argv) as alias)",
            ])
            .args(["-e", "end run"])
            .arg(path)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| TrashError::Unavailable(format!("cannot run osascript: {err}")))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(TrashError::Unavailable(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ))
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::TrashError;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const SCRIPT: &str = "Add-Type -AssemblyName Microsoft.VisualBasic; \
        $p = $env:ARCODE_TRASH_PATH; \
        if (Test-Path -LiteralPath $p -PathType Container) { \
          [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($p, 'OnlyErrorDialogs', 'SendToRecycleBin') \
        } else { \
          [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p, 'OnlyErrorDialogs', 'SendToRecycleBin') \
        }";

    pub fn trash(path: &Path) -> Result<(), TrashError> {
        // The path travels in an environment variable, so no quoting.
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
            .env("ARCODE_TRASH_PATH", crate::files::external_path(path))
            .stdin(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|err| TrashError::Unavailable(format!("cannot run PowerShell: {err}")))?;
        if output.status.success() && !path.exists() {
            Ok(())
        } else {
            Err(TrashError::Unavailable(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ))
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use super::{TrashError, trash_into};

    fn home_trash() -> Option<PathBuf> {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(data.join("Trash"))
    }

    pub fn trash(path: &Path) -> Result<(), TrashError> {
        let home_err = match home_trash() {
            Some(dir) => match trash_into(&dir, path) {
                Ok(_) => return Ok(()),
                Err(TrashError::Io(err)) if err.kind() == std::io::ErrorKind::CrossesDevices => {
                    "the file is on another file system".to_owned()
                }
                Err(err) => return Err(err),
            },
            None => "no home folder".to_owned(),
        };
        // `gio` knows the per-volume trash folders (`.Trash-$UID`).
        match Command::new("gio")
            .arg("trash")
            .arg("--")
            .arg(path)
            .stdin(Stdio::null())
            .output()
        {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(TrashError::Unavailable(format!(
                "{home_err}; gio: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))),
            Err(_) => Err(TrashError::Unavailable(home_err)),
        }
    }
}

/// Moves `path` into the FreeDesktop trash folder `trash` (its `files` and
/// `info` folders are created as needed) and returns where it went.
///
/// The `.trashinfo` file is written first with `create_new`, which
/// reserves the name; it is removed again when the move fails.
pub fn trash_into(trash: &Path, path: &Path) -> Result<PathBuf, TrashError> {
    let files = trash.join("files");
    let info = trash.join("info");
    if let Some(parent) = trash.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| TrashError::Unavailable(format!("{}: {err}", parent.display())))?;
    }
    for dir in [trash, &files, &info] {
        create_private_dir(dir)
            .map_err(|err| TrashError::Unavailable(format!("{}: {err}", dir.display())))?;
    }
    let absolute = std::path::absolute(path).map_err(TrashError::Io)?;
    // The raw name, not a lossy copy: a name that is not UTF-8 must keep
    // its exact bytes in the trash.
    let name = absolute
        .file_name()
        .map(OsStr::to_os_string)
        .ok_or_else(|| TrashError::Io(io::Error::other("cannot trash a root folder")))?;
    let date = deletion_date(SystemTime::now());
    let body = format!(
        "[Trash Info]\nPath={}\nDeletionDate={date}\n",
        encode_path(&absolute)
    );
    for n in 1..10_000 {
        let mut candidate = name.clone();
        if n > 1 {
            candidate.push(format!(".{n}"));
        }
        let mut info_name = candidate.clone();
        info_name.push(".trashinfo");
        let info_file = info.join(info_name);
        let target = files.join(&candidate);
        let reserved = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info_file);
        let mut file = match reserved {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(TrashError::Unavailable(format!(
                    "{}: {err}",
                    info.display()
                )));
            }
        };
        if target.symlink_metadata().is_ok() {
            drop(file);
            std::fs::remove_file(&info_file).ok();
            continue;
        }
        use std::io::Write as _;
        if let Err(err) = file.write_all(body.as_bytes()) {
            std::fs::remove_file(&info_file).ok();
            return Err(TrashError::Unavailable(err.to_string()));
        }
        drop(file);
        return match std::fs::rename(&absolute, &target) {
            Ok(()) => Ok(target),
            Err(err) => {
                std::fs::remove_file(&info_file).ok();
                Err(TrashError::Io(err))
            }
        };
    }
    Err(TrashError::Unavailable(
        "too many files with this name in the trash".into(),
    ))
}

/// Creates `dir` if missing; the FreeDesktop trash folders are private
/// (0700) on Unix. An existing folder keeps its permissions.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// The raw bytes of a path: exact on Unix, UTF-8 elsewhere.
#[cfg(unix)]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt as _;
    std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    match path.to_string_lossy() {
        std::borrow::Cow::Borrowed(s) => std::borrow::Cow::Borrowed(s.as_bytes()),
        std::borrow::Cow::Owned(s) => std::borrow::Cow::Owned(s.into_bytes()),
    }
}

/// Percent-encodes a path for a `.trashinfo` file (`/` kept), byte by
/// byte, so names that are not UTF-8 are recorded exactly.
fn encode_path(path: &Path) -> String {
    let mut out = String::new();
    for &byte in path_bytes(path).iter() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// `YYYY-MM-DDThh:mm:ss` (UTC) for `time`.
fn deletion_date(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Gregorian date of a day count since 1970-01-01 (Howard Hinnant's
/// algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_formatted() {
        assert_eq!(deletion_date(UNIX_EPOCH), "1970-01-01T00:00:00");
        let t = UNIX_EPOCH + std::time::Duration::from_secs(1_709_210_096);
        assert_eq!(deletion_date(t), "2024-02-29T12:34:56");
    }

    #[test]
    fn trashing_moves_the_file_and_writes_info() {
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join("Trash");
        let file = dir.path().join("a b.txt");
        std::fs::write(&file, "x").unwrap();
        let to = trash_into(&trash, &file).unwrap();
        assert!(!file.exists());
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "x");
        let info = std::fs::read_to_string(trash.join("info/a b.txt.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath=/"), "{info}");
        assert!(info.contains("a%20b.txt\nDeletionDate="), "{info}");

        // A second file of the same name gets a new slot; folders work too.
        let folder = dir.path().join("a b.txt");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("inner"), "y").unwrap();
        let to = trash_into(&trash, &folder).unwrap();
        assert_eq!(to, trash.join("files/a b.txt.2"));
        assert!(to.join("inner").is_file());
        assert!(trash.join("info/a b.txt.2.trashinfo").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_new_trash_is_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join("share/Trash");
        let file = dir.path().join("f");
        std::fs::write(&file, "x").unwrap();
        trash_into(&trash, &file).unwrap();
        for d in [trash.clone(), trash.join("files"), trash.join("info")] {
            let mode = std::fs::metadata(&d).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}", d.display());
        }
    }

    #[test]
    fn a_failed_move_leaves_no_info_file() {
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join("Trash");
        let missing = dir.path().join("missing");
        assert!(matches!(
            trash_into(&trash, &missing),
            Err(TrashError::Io(_))
        ));
        assert_eq!(std::fs::read_dir(trash.join("info")).unwrap().count(), 0);
        assert!(matches!(
            move_to_trash(&missing),
            Err(TrashError::Io(e)) if e.kind() == io::ErrorKind::NotFound
        ));
    }
}
