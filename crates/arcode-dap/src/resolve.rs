//! Finding adapter executables on `PATH`.
//!
//! On Windows every `PATHEXT` extension is tried (so `.cmd`/`.bat` shims are
//! found by bare name, which `std::process::Command` does not do); on Unix
//! a candidate must be an executable file.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// `PATHEXT` used on Windows when the variable is unset.
pub(crate) const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// Looks `command` up the way a shell would, using `PATH` (and on Windows
/// `PATHEXT`) from `env` overrides or the current process.
///
/// A command containing a directory separator is returned as is if it
/// names an existing file. Returns `None` if nothing matches.
pub fn find_executable(command: &str, env: &[(String, String)]) -> Option<PathBuf> {
    let lookup = |name: &str| -> Option<OsString> {
        env.iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| OsString::from(v))
            .or_else(|| std::env::var_os(name))
    };
    let path = lookup("PATH").unwrap_or_default();
    let pathext = if cfg!(windows) {
        lookup("PATHEXT")
            .and_then(|v| v.into_string().ok())
            .unwrap_or_else(|| DEFAULT_PATHEXT.to_string())
    } else {
        String::new()
    };
    search(command, std::env::split_paths(&path), &pathext, is_executable)
}

/// The platform-neutral search behind [`find_executable`]. `pathext` is a
/// `;`-separated extension list (empty on Unix).
pub(crate) fn search(
    command: &str,
    dirs: impl IntoIterator<Item = PathBuf>,
    pathext: &str,
    is_exec: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }
    let exts: Vec<&str> = pathext
        .split(';')
        .map(str::trim)
        .filter(|e| e.starts_with('.') && e.len() > 1)
        .collect();
    let lower = command.to_ascii_lowercase();
    let try_plain = exts.is_empty() || exts.iter().any(|e| lower.ends_with(&e.to_ascii_lowercase()));
    let candidates = |base: PathBuf| {
        let mut list = Vec::new();
        if try_plain {
            list.push(base.clone());
        }
        for ext in &exts {
            let mut name = base.clone().into_os_string();
            name.push(ext.to_ascii_lowercase());
            list.push(PathBuf::from(name));
        }
        list
    };
    if command.contains(['/', '\\']) || has_drive_prefix(command) {
        return candidates(PathBuf::from(command))
            .into_iter()
            .find(|c| is_exec(c));
    }
    dirs.into_iter()
        .filter(|d| !d.as_os_str().is_empty())
        .flat_map(|d| candidates(d.join(command)))
        .find(|c| is_exec(c))
}

fn has_drive_prefix(command: &str) -> bool {
    let b = command.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            std::fs::write(dir.path().join(f), b"").unwrap();
        }
        dir
    }

    fn win(command: &str, dirs: &[&Path]) -> Option<PathBuf> {
        search(
            command,
            dirs.iter().map(|d| d.to_path_buf()),
            DEFAULT_PATHEXT,
            Path::is_file,
        )
    }

    #[test]
    fn windows_finds_shims_in_pathext_and_path_order() {
        let a = setup(&["dlv.bat"]);
        let b = setup(&["dlv.exe", "dlv.cmd"]);
        assert_eq!(win("dlv", &[a.path(), b.path()]), Some(a.path().join("dlv.bat")));
        assert_eq!(win("dlv", &[b.path()]), Some(b.path().join("dlv.exe")));
        assert_eq!(win("dlv.cmd", &[b.path()]), Some(b.path().join("dlv.cmd")));
        assert_eq!(win("missing", &[a.path()]), None);
    }

    #[test]
    fn unix_style_search_without_extensions() {
        let d = setup(&["codelldb"]);
        let found = search("codelldb", [d.path().to_path_buf()], "", Path::is_file);
        assert_eq!(found, Some(d.path().join("codelldb")));
        assert_eq!(search("", [d.path().to_path_buf()], "", Path::is_file), None);
    }

    #[test]
    fn explicit_paths_are_checked_directly() {
        let d = setup(&["adapter.cmd"]);
        let plain = d.path().join("adapter");
        let found = search(&plain.to_string_lossy(), [], DEFAULT_PATHEXT, Path::is_file);
        assert_eq!(found, Some(d.path().join("adapter.cmd")));
        let missing = d.path().join("nope");
        assert_eq!(
            search(&missing.to_string_lossy(), [], "", Path::is_file),
            None
        );
    }

    #[test]
    fn directories_are_not_executables() {
        let d = setup(&[]);
        std::fs::create_dir(d.path().join("tool")).unwrap();
        assert_eq!(find_executable("tool", &path_env(d.path())), None);
    }

    fn path_env(dir: &Path) -> Vec<(String, String)> {
        vec![("PATH".into(), dir.to_string_lossy().into_owned())]
    }

    #[cfg(unix)]
    #[test]
    fn unix_requires_the_executable_bit() {
        use std::os::unix::fs::PermissionsExt;
        let d = setup(&["lldb-dap"]);
        let file = d.path().join("lldb-dap");
        assert_eq!(find_executable("lldb-dap", &path_env(d.path())), None);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_executable("lldb-dap", &path_env(d.path())), Some(file));
    }

    #[cfg(windows)]
    #[test]
    fn windows_env_override_uses_pathext() {
        let d = setup(&["debugpy-adapter.cmd"]);
        assert_eq!(
            find_executable("debugpy-adapter", &path_env(d.path())),
            Some(d.path().join("debugpy-adapter.cmd"))
        );
    }
}
