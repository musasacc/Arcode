//! Finding a server executable the way a Windows shell would.
//!
//! Rust's `Command` on Windows only appends `.exe` when searching `PATH`, so
//! the `.cmd`/`.bat` shims npm installs (`typescript-language-server`,
//! `pyright-langserver`, ...) are not found by bare name. [`resolve_command`]
//! searches `PATH` with every `PATHEXT` extension; std then runs a resolved
//! `.cmd`/`.bat` through `cmd.exe` with safe argument escaping.
//!
//! The search itself is platform-neutral so it is tested everywhere; it is
//! only wired into process spawning on Windows.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// `PATHEXT` used when the variable is unset.
pub(crate) const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// Looks `command` up in `dirs` using the extensions in `pathext`
/// (`;`-separated, e.g. `.COM;.EXE;.BAT;.CMD`).
///
/// Returns `None` (leave the command to the OS) when `command` contains a
/// directory component or nothing matches. A command that already has one
/// of the extensions is tried as is first. `is_file` decides existence.
pub(crate) fn resolve_command(
    command: &str,
    dirs: impl IntoIterator<Item = PathBuf>,
    pathext: &str,
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if command.is_empty() || command.contains(['/', '\\']) || has_drive_prefix(command) {
        return None;
    }
    let exts: Vec<&str> = pathext
        .split(';')
        .map(str::trim)
        .filter(|e| e.starts_with('.') && e.len() > 1)
        .collect();
    let lower = command.to_ascii_lowercase();
    let has_known_ext = exts
        .iter()
        .any(|e| lower.ends_with(&e.to_ascii_lowercase()));
    for dir in dirs {
        if dir.as_os_str().is_empty() {
            continue;
        }
        if has_known_ext {
            let candidate = dir.join(command);
            if is_file(&candidate) {
                return Some(candidate);
            }
        }
        for ext in &exts {
            let candidate = dir.join(format!("{command}{}", ext.to_ascii_lowercase()));
            if is_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn has_drive_prefix(command: &str) -> bool {
    let b = command.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Resolves `command` against the child's `PATH` (from `env` overrides or
/// the current process) and `PATHEXT`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn resolve_native(command: &str, env: &[(String, String)]) -> Option<PathBuf> {
    let lookup = |name: &str| {
        env.iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| std::ffi::OsString::from(v))
            .or_else(|| std::env::var_os(name))
    };
    let path = lookup("PATH")?;
    let pathext = lookup("PATHEXT");
    let pathext = pathext
        .as_deref()
        .and_then(OsStr::to_str)
        .unwrap_or(DEFAULT_PATHEXT);
    resolve_command(command, std::env::split_paths(&path), pathext, |p| {
        p.is_file()
    })
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

    fn resolve(command: &str, dirs: &[&Path]) -> Option<PathBuf> {
        resolve_command(
            command,
            dirs.iter().map(|d| d.to_path_buf()),
            DEFAULT_PATHEXT,
            |p| p.is_file(),
        )
    }

    #[test]
    fn finds_cmd_shims_by_bare_name() {
        let d = setup(&["typescript-language-server.cmd"]);
        assert_eq!(
            resolve("typescript-language-server", &[d.path()]),
            Some(d.path().join("typescript-language-server.cmd"))
        );
    }

    #[test]
    fn respects_pathext_order_and_path_order() {
        let a = setup(&["srv.bat"]);
        let b = setup(&["srv.exe", "srv.cmd"]);
        // First PATH dir wins even with a "later" extension.
        assert_eq!(
            resolve("srv", &[a.path(), b.path()]),
            Some(a.path().join("srv.bat"))
        );
        // Within one dir, PATHEXT order decides (.EXE before .CMD).
        assert_eq!(resolve("srv", &[b.path()]), Some(b.path().join("srv.exe")));
    }

    #[test]
    fn explicit_extension_is_tried_as_is() {
        let d = setup(&["pyright-langserver.cmd"]);
        assert_eq!(
            resolve("pyright-langserver.cmd", &[d.path()]),
            Some(d.path().join("pyright-langserver.cmd"))
        );
    }

    #[test]
    fn paths_and_misses_are_left_to_the_os() {
        let d = setup(&["x.cmd"]);
        assert_eq!(resolve("missing", &[d.path()]), None);
        assert_eq!(resolve(r"C:\tools\x", &[d.path()]), None);
        assert_eq!(resolve("./x", &[d.path()]), None);
        assert_eq!(resolve("", &[d.path()]), None);
        // Directories are not executables.
        std::fs::create_dir(d.path().join("dir.exe")).unwrap();
        assert_eq!(resolve("dir", &[d.path()]), None);
    }

    #[test]
    fn custom_pathext_and_empty_entries() {
        let d = setup(&["tool.ps1"]);
        let found = resolve_command(
            "tool",
            [PathBuf::new(), d.path().to_path_buf()],
            ".EXE; ;.PS1;bogus",
            |p| p.is_file(),
        );
        assert_eq!(found, Some(d.path().join("tool.ps1")));
    }

    #[test]
    fn env_override_of_path_is_used() {
        let d = setup(&["only-here.cmd"]);
        let env = vec![("Path".to_string(), d.path().to_string_lossy().into_owned())];
        assert_eq!(
            resolve_native("only-here", &env),
            Some(d.path().join("only-here.cmd"))
        );
    }
}
