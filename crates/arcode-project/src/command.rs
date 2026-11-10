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
    /// A single source file run on its own (Run Current File).
    File,
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
    /// A step that must succeed first: compiling a single source file into
    /// the program this command runs. [`RunCommand::command_line`] chains
    /// both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile: Option<Box<RunCommand>>,
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
            compile: None,
        }
    }

    /// Adds a compile step that runs first (see [`RunCommand::compile`]).
    pub fn with_compile(mut self, compile: RunCommand) -> Self {
        self.compile = Some(Box::new(compile));
        self
    }

    /// Replaces the label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// The command line, quoted for display or for typing into this
    /// platform's default shell (see [`RunCommand::command_line_in`]).
    pub fn command_line(&self) -> String {
        self.command_line_in(Shell::platform())
    }

    /// The command line, quoted for `shell`. A compile step comes first
    /// and must succeed: `a && b` in POSIX shells and `cmd.exe`,
    /// `a; if ($?) { b }` in PowerShell (5.1 has no `&&`) and
    /// `a; if $env.LAST_EXIT_CODE == 0 { b }` in Nushell.
    pub fn command_line_in(&self, shell: Shell) -> String {
        let line = command_line_in(shell, &self.program, &self.args);
        let Some(first) = &self.compile else {
            return line;
        };
        let first = first.command_line_in(shell);
        match shell {
            Shell::Posix | Shell::Cmd => format!("{first} && {line}"),
            Shell::PowerShell => format!("{first}; if ($?) {{ {line} }}"),
            Shell::Nu => format!("{first}; if $env.LAST_EXIT_CODE == 0 {{ {line} }}"),
        }
    }

    /// A short form for menus: the command line, or for a compiled single
    /// file its compile step without the output path
    /// (`rustc --edition 2024 -g test.rs`).
    pub fn display_line(&self) -> String {
        let Some(compile) = &self.compile else {
            return self.command_line();
        };
        let mut shown = Vec::with_capacity(compile.args.len());
        let mut args = compile.args.iter().peekable();
        while let Some(arg) = args.next() {
            if arg == "-o" && args.peek().is_some_and(|out| self.runs_output(out)) {
                args.next();
                continue;
            }
            shown.push(arg.clone());
        }
        command_line(&compile.program, &shown)
    }

    /// Whether this command runs the compile step's output `out`: the same
    /// path, or `./out` in the same folder.
    fn runs_output(&self, out: &str) -> bool {
        let program = self.program.as_str();
        program == out
            || program
                .strip_prefix("./")
                .or_else(|| program.strip_prefix(".\\"))
                == Some(out)
    }
}

/// The kind of shell a command line is typed into: it decides how
/// arguments are quoted and how a compile step is chained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// `sh`, `bash`, `zsh`, `fish` and other POSIX-style shells (Git Bash
    /// on Windows too).
    Posix,
    /// Windows PowerShell or PowerShell 7 (`pwsh`).
    PowerShell,
    /// `cmd.exe`.
    Cmd,
    /// Nushell.
    Nu,
}

impl Shell {
    /// The shell the terminal starts when none is configured: PowerShell
    /// on Windows, a POSIX shell elsewhere.
    pub fn platform() -> Self {
        if cfg!(windows) {
            Self::PowerShell
        } else {
            Self::Posix
        }
    }

    /// The kind of the shell program `program`, a name or a path
    /// (`/usr/bin/fish`, `pwsh.exe`, `C:\Windows\System32\cmd.exe`).
    pub fn of_program(program: &str) -> Self {
        let program = program.trim().trim_matches('"');
        let name = program
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(program)
            .to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        match name {
            "" => Self::platform(),
            "pwsh" | "pwsh-preview" | "powershell" => Self::PowerShell,
            "cmd" => Self::Cmd,
            "nu" | "nushell" => Self::Nu,
            _ => Self::Posix,
        }
    }
}

/// Joins a program and its arguments into one shell-style line, quoting
/// arguments that contain spaces or shell metacharacters, for this
/// platform's default shell (see [`command_line_in`]).
pub fn command_line(program: &str, args: &[String]) -> String {
    command_line_in(Shell::platform(), program, args)
}

/// Joins a program and its arguments into one line for `shell`. A quoted
/// program gets PowerShell's call operator (`& 'C:\Program Files\x.exe'
/// arg`) or Nushell's `^`, since a line that starts with a string is an
/// expression there, not a command.
pub fn command_line_in(shell: Shell, program: &str, args: &[String]) -> String {
    let quoted = quote_arg_in(shell, program);
    let mut line = match (shell, &quoted) {
        (Shell::PowerShell, Cow::Owned(_)) => format!("& {quoted}"),
        (Shell::Nu, Cow::Owned(_)) => format!("^{quoted}"),
        _ => quoted.into_owned(),
    };
    for arg in args {
        line.push(' ');
        line.push_str(&quote_arg_in(shell, arg));
    }
    line
}

/// Quotes one argument for this platform's default shell (see
/// [`quote_arg_in`]).
pub fn quote_arg(arg: &str) -> Cow<'_, str> {
    quote_arg_in(Shell::platform(), arg)
}

/// Quotes one argument for `shell` when needed: single quotes in POSIX
/// shells (`'\''` for an embedded `'`) and PowerShell (`''`), double
/// quotes in `cmd.exe` and Nushell. A `\` is a path separator for
/// PowerShell and `cmd.exe` but an escape elsewhere, where it forces
/// quoting.
pub fn quote_arg_in(shell: Shell, arg: &str) -> Cow<'_, str> {
    let backslash_ok = matches!(shell, Shell::PowerShell | Shell::Cmd);
    let safe = !arg.is_empty()
        && arg.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | '=' | ':' | '@' | '%' | '+' | ',')
                || (c == '\\' && backslash_ok)
        });
    if safe {
        return Cow::Borrowed(arg);
    }
    Cow::Owned(match shell {
        Shell::Posix => format!("'{}'", arg.replace('\'', r"'\''")),
        Shell::PowerShell => format!("'{}'", arg.replace('\'', "''")),
        Shell::Cmd => format!("\"{}\"", arg.replace('"', "\"\"")),
        Shell::Nu => format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\"")),
    })
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

    #[test]
    fn a_compile_step_runs_first() {
        let compile = RunCommand::new(
            "cc",
            args(&["-g", "a.c", "-o", "/b/a"]),
            "/p",
            RunKind::Build,
            RunSource::File,
        );
        let run = RunCommand::new("/b/a", Vec::new(), "/p", RunKind::Run, RunSource::File)
            .with_compile(compile);
        if cfg!(windows) {
            assert_eq!(run.command_line(), "cc -g a.c -o /b/a; if ($?) { /b/a }");
        } else {
            assert_eq!(run.command_line(), "cc -g a.c -o /b/a && /b/a");
        }
    }

    #[test]
    fn a_compile_step_is_chained_for_the_shell_in_use() {
        let compile = RunCommand::new(
            "rustc",
            args(&["-g", "my file.rs", "-o", "/b/my file"]),
            "/p",
            RunKind::Build,
            RunSource::File,
        );
        let run = RunCommand::new(
            "/b/my file",
            Vec::new(),
            "/p",
            RunKind::Run,
            RunSource::File,
        )
        .with_compile(compile);
        assert_eq!(
            run.command_line_in(Shell::Posix),
            "rustc -g 'my file.rs' -o '/b/my file' && '/b/my file'"
        );
        assert_eq!(
            run.command_line_in(Shell::PowerShell),
            "rustc -g 'my file.rs' -o '/b/my file'; if ($?) { & '/b/my file' }"
        );
        assert_eq!(
            run.command_line_in(Shell::Cmd),
            "rustc -g \"my file.rs\" -o \"/b/my file\" && \"/b/my file\""
        );
        assert_eq!(
            run.command_line_in(Shell::Nu),
            "rustc -g \"my file.rs\" -o \"/b/my file\"; \
             if $env.LAST_EXIT_CODE == 0 { ^\"/b/my file\" }"
        );
        // Menus leave out the build folder.
        assert_eq!(
            run.display_line(),
            command_line("rustc", &args(&["-g", "my file.rs"]))
        );
    }

    #[test]
    fn shells_are_told_apart_by_program() {
        assert_eq!(Shell::of_program("/bin/bash"), Shell::Posix);
        assert_eq!(Shell::of_program("/usr/bin/fish"), Shell::Posix);
        assert_eq!(
            Shell::of_program(r"C:\Program Files\Git\bin\bash.exe"),
            Shell::Posix
        );
        assert_eq!(Shell::of_program("pwsh.exe"), Shell::PowerShell);
        assert_eq!(Shell::of_program("powershell"), Shell::PowerShell);
        assert_eq!(
            Shell::of_program(r"C:\Windows\system32\CMD.EXE"),
            Shell::Cmd
        );
        assert_eq!(Shell::of_program("/opt/homebrew/bin/nu"), Shell::Nu);
        assert_eq!(Shell::of_program(" "), Shell::platform());
        // Backslashes are path separators only where the shell keeps them.
        assert_eq!(quote_arg_in(Shell::Cmd, r"C:\a\b.exe"), r"C:\a\b.exe");
        assert_eq!(quote_arg_in(Shell::Posix, r"C:\a"), r"'C:\a'");
        assert_eq!(quote_arg_in(Shell::Nu, r"C:\a"), r#""C:\\a""#);
    }
}
