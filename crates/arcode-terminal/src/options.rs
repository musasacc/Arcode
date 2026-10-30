//! Spawn options and default shell discovery.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// How to start a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalOptions {
    /// Program to run; `None` picks [`default_shell`].
    pub shell: Option<String>,
    /// Arguments for the program.
    pub args: Vec<String>,
    /// Working directory; `None` inherits the current one.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables (override the defaults).
    pub env: Vec<(String, String)>,
    /// Grid width in cells.
    pub cols: u16,
    /// Grid height in cells.
    pub rows: u16,
    /// Cell width in pixels (reported to the PTY).
    pub cell_width: u16,
    /// Cell height in pixels (reported to the PTY).
    pub cell_height: u16,
    /// Lines of scrollback history.
    pub scrollback: usize,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            shell: None,
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            cols: 80,
            rows: 24,
            cell_width: 8,
            cell_height: 16,
            scrollback: 10_000,
        }
    }
}

impl TerminalOptions {
    /// Options that run `program` with `args` instead of the default shell.
    pub fn command(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            shell: Some(program.into()),
            args: args.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// The program that will run.
    pub fn program(&self) -> String {
        self.shell
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(default_shell)
    }

    /// The child environment: `TERM`/`COLORTERM` defaults, then [`Self::env`].
    pub fn environment(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("TERM".to_owned(), "xterm-256color".to_owned());
        env.insert("COLORTERM".to_owned(), "truecolor".to_owned());
        for (key, value) in &self.env {
            env.insert(key.clone(), value.clone());
        }
        env
    }
}

/// The default shell: `$SHELL` or `/bin/sh` on Unix; on Windows `pwsh.exe`
/// if on `PATH`, else `powershell.exe`, else `%COMSPEC%`, else `cmd.exe`.
pub fn default_shell() -> String {
    #[cfg(windows)]
    {
        windows_shell(
            std::env::var_os("PATH").as_deref(),
            std::env::var("COMSPEC").ok(),
        )
    }
    #[cfg(not(windows))]
    {
        unix_shell(std::env::var("SHELL").ok())
    }
}

/// Picks the Unix shell from the value of `$SHELL`.
#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn unix_shell(shell_var: Option<String>) -> String {
    shell_var
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned())
}

/// Picks the Windows shell from `PATH` and `COMSPEC`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_shell(path_var: Option<&OsStr>, comspec: Option<String>) -> String {
    for name in ["pwsh.exe", "powershell.exe"] {
        if path_var.and_then(|p| find_in_path(name, p)).is_some() {
            // The bare name lets CreateProcess search PATH and avoids quoting
            // issues with paths like `C:\Program Files\...`.
            return name.to_owned();
        }
    }
    comspec
        .map(|c| c.trim().to_owned())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "cmd.exe".to_owned())
}

/// Quotes a Windows program path containing spaces, since the ConPTY
/// command line is built from the program verbatim.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn quote_windows_program(program: &str) -> String {
    if program.contains([' ', '\t']) && !program.starts_with('"') {
        format!("\"{program}\"")
    } else {
        program.to_owned()
    }
}

/// Finds an executable file `name` in a `PATH`-style list of directories.
pub fn find_in_path(name: &str, path_var: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(name))
        .find(|candidate| is_file(candidate))
}

fn is_file(path: &Path) -> bool {
    path.metadata().map(|m| m.is_file()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn defaults() {
        let o = TerminalOptions::default();
        assert_eq!((o.cols, o.rows), (80, 24));
        assert_eq!(o.scrollback, 10_000);
        assert!(o.shell.is_none() && o.args.is_empty() && o.cwd.is_none());
    }

    #[test]
    fn command_constructor() {
        let o = TerminalOptions::command("/bin/sh", ["-c", "true"]);
        assert_eq!(o.program(), "/bin/sh");
        assert_eq!(o.args, vec!["-c".to_owned(), "true".to_owned()]);
    }

    #[test]
    fn empty_shell_falls_back() {
        let o = TerminalOptions {
            shell: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(o.program(), default_shell());
    }

    #[test]
    fn environment_defaults_and_overrides() {
        let mut o = TerminalOptions::default();
        let env = o.environment();
        assert_eq!(env["TERM"], "xterm-256color");
        assert_eq!(env["COLORTERM"], "truecolor");
        o.env = vec![("TERM".into(), "dumb".into()), ("FOO".into(), "bar".into())];
        let env = o.environment();
        assert_eq!(env["TERM"], "dumb");
        assert_eq!(env["FOO"], "bar");
    }

    #[test]
    fn windows_program_quoting() {
        assert_eq!(quote_windows_program("cmd.exe"), "cmd.exe");
        assert_eq!(
            quote_windows_program("C:\\Program Files\\x.exe"),
            "\"C:\\Program Files\\x.exe\""
        );
        assert_eq!(
            quote_windows_program("\"C:\\a b\\x.exe\""),
            "\"C:\\a b\\x.exe\""
        );
    }

    #[test]
    fn unix_shell_choice() {
        assert_eq!(unix_shell(Some("/bin/zsh".into())), "/bin/zsh");
        assert_eq!(unix_shell(Some("  ".into())), "/bin/sh");
        assert_eq!(unix_shell(None), "/bin/sh");
    }

    #[test]
    fn find_in_path_finds_files_only() {
        let a = std::env::temp_dir().join(format!("arcode-term-path-a-{}", std::process::id()));
        let b = std::env::temp_dir().join(format!("arcode-term-path-b-{}", std::process::id()));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(b.join("pwsh.exe")).unwrap();
        std::fs::write(a.join("pwsh.exe"), b"").unwrap();
        let path: OsString = std::env::join_paths([&b, &a]).unwrap();
        assert_eq!(find_in_path("pwsh.exe", &path), Some(a.join("pwsh.exe")));
        assert_eq!(find_in_path("nope.exe", &path), None);

        assert_eq!(windows_shell(Some(&path), None), "pwsh.exe");
        let only_b: OsString = std::env::join_paths([&b]).unwrap();
        assert_eq!(
            windows_shell(Some(&only_b), Some("C:\\cmd.exe".into())),
            "C:\\cmd.exe"
        );
        assert_eq!(windows_shell(None, None), "cmd.exe");
        std::fs::remove_dir_all(&a).unwrap();
        std::fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn windows_shell_prefers_pwsh_then_powershell() {
        let dir = std::env::temp_dir().join(format!("arcode-term-ps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("powershell.exe"), b"").unwrap();
        let path: OsString = std::env::join_paths([&dir]).unwrap();
        assert_eq!(
            windows_shell(Some(&path), Some("cmd".into())),
            "powershell.exe"
        );
        std::fs::write(dir.join("pwsh.exe"), b"").unwrap();
        assert_eq!(windows_shell(Some(&path), Some("cmd".into())), "pwsh.exe");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
