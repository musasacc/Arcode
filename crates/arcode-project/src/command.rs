//! Commands the UI runs in the integrated terminal, and how to display them.

use std::borrow::Cow;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What a [`RunCommand`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunKind {
    /// Starts the program (or a dev server).
    Run,
    /// Runs tests.
    Test,
    /// Builds without running.
    Build,
}

/// Which tool a [`RunCommand`] was derived from (for grouping in the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunSource {
    /// `Cargo.toml`.
    Cargo,
    /// `package.json` (npm, pnpm, yarn or bun).
    Node,
    /// `go.mod`.
    Go,
    /// Python files, `pytest` configuration.
    Python,
    /// `Makefile` targets.
    Make,
    /// `justfile` recipes.
    Just,
}

/// A command to run in the integrated terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCommand {
    /// Short label for menus, usually the command line itself.
    pub label: String,
    /// Program name (resolved via `PATH`) or absolute path.
    pub program: String,
    /// Arguments, unquoted.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
    /// Run, test or build.
    pub kind: RunKind,
    /// Tool the command came from.
    pub source: RunSource,
}

impl RunCommand {
    /// Builds a command whose label is its own command line.
    pub fn new(
        program: impl Into<String>,
        args: Vec<String>,
        cwd: impl Into<PathBuf>,
        kind: RunKind,
        source: RunSource,
    ) -> Self {
        let program = program.into();
        let label = command_line(&program, &args);
        Self {
            label,
            program,
            args,
            cwd: cwd.into(),
            kind,
            source,
        }
    }

    /// Replaces the label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// The command line, quoted for display or for typing into a shell.
    pub fn command_line(&self) -> String {
        command_line(&self.program, &self.args)
    }
}

/// Joins a program and its arguments into one shell-style line, quoting
/// arguments that contain spaces or shell metacharacters.
///
/// On Windows the line targets PowerShell: a quoted program is prefixed with
/// the call operator (`& 'C:\Program Files\x.exe' arg`), since a line that
/// starts with a string is an expression there, not a command.
pub fn command_line(program: &str, args: &[String]) -> String {
    let quoted = quote_arg(program);
    let mut line = if cfg!(windows) && matches!(quoted, Cow::Owned(_)) {
        format!("& {quoted}")
    } else {
        quoted.into_owned()
    };
    for arg in args {
        line.push(' ');
        line.push_str(&quote_arg(arg));
    }
    line
}

/// Quotes one argument with single quotes when needed.
///
/// Single quotes work in POSIX shells and in PowerShell; only the escaping
/// of an embedded `'` differs, so it follows the target platform. A `\` is
/// a path separator on Windows but an escape in POSIX shells, where it
/// forces quoting.
pub fn quote_arg(arg: &str) -> Cow<'_, str> {
    let safe = !arg.is_empty()
        && arg.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | '=' | ':' | '@' | '%' | '+' | ',')
                || (c == '\\' && cfg!(windows))
        });
    if safe {
        return Cow::Borrowed(arg);
    }
    let escaped = if cfg!(windows) {
        arg.replace('\'', "''")
    } else {
        arg.replace('\'', r"'\''")
    };
    Cow::Owned(format!("'{escaped}'"))
}

/// Converts `&str` arguments into owned strings.
pub(crate) fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_args_are_not_quoted() {
        assert_eq!(
            command_line("cargo", &args(&["run", "-p", "a-b"])),
            "cargo run -p a-b"
        );
    }

    #[test]
    fn special_args_are_quoted() {
        assert_eq!(quote_arg("^TestA$"), "'^TestA$'");
        assert_eq!(quote_arg("two words"), "'two words'");
        assert_eq!(quote_arg(""), "''");
        if cfg!(windows) {
            assert_eq!(quote_arg("it's"), "'it''s'");
        } else {
            assert_eq!(quote_arg("it's"), r"'it'\''s'");
        }
    }

    #[test]
    fn backslash_quoting_follows_the_shell() {
        if cfg!(windows) {
            assert_eq!(quote_arg(r"C:\dir\x.exe"), r"C:\dir\x.exe");
        } else {
            assert_eq!(quote_arg(r"v1\.2"), r"'v1\.2'");
        }
    }

    #[test]
    fn quoted_program_uses_call_operator_on_windows() {
        let line = command_line("/my dir/py", &args(&["-m", "x"]));
        if cfg!(windows) {
            assert_eq!(line, "& '/my dir/py' -m x");
        } else {
            assert_eq!(line, "'/my dir/py' -m x");
        }
        assert_eq!(command_line("py", &args(&["a b"])), "py 'a b'");
    }

    #[test]
    fn label_defaults_to_command_line() {
        let cmd = RunCommand::new(
            "go",
            args(&["test", "./..."]),
            "/p",
            RunKind::Test,
            RunSource::Go,
        );
        assert_eq!(cmd.label, "go test ./...");
        assert_eq!(cmd.with_label("x").label, "x");
    }
}
