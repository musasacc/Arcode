//! Running a single source file on its own (Run Current File, Debug
//! Current File), for folders that have no project file to detect run
//! commands from.
//!
//! [`file_command`] picks the toolchain for the file's language from
//! `PATH` (a project's virtual environment for Python) and returns the
//! command to run in the integrated terminal. Compiled languages are built
//! next to the source and run from its folder, or into a per-file folder
//! of a cache folder (see [`ProgramOutput`]); a name next to the source
//! that Arcode did not build is never written over. When nothing can run the
//! file, the error says what to install. Inside a Cargo package or a Go
//! module the project's own tool is used instead.
//!
//! Blocking (looks up programs, reads the file's first line, may run
//! `node --version`): call it off the UI thread.

mod langs;
mod output;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::command::RunCommand;
use crate::fsutil;

pub use output::{
    BUILD_WINDOW, BuiltPrograms, NameTaken, ProgramOutput, abandon_started_build, check_next_to,
    finish_started_build, program_next_to, record_build_started, record_built, taken_note,
};

/// The languages [`file_command`] can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileLanguage {
    /// `python file.py` (the project's virtualenv when it has one).
    Python,
    /// `rustc`, then the program; `cargo run` for a file of a Cargo
    /// package.
    Rust,
    /// `cc -g`, then the program.
    C,
    /// `c++ -g`, then the program.
    Cpp,
    /// `go run file.go` (the package inside a Go module).
    Go,
    /// `node file.js`.
    JavaScript,
    /// `node --experimental-strip-types file.ts` (Node.js 22.6+), Bun or
    /// Deno.
    TypeScript,
    /// `java File.java` (single-file source launch, JDK 11+).
    Java,
    /// `ruby file.rb`.
    Ruby,
    /// `php file.php`.
    Php,
    /// `lua file.lua`.
    Lua,
    /// `bash file.sh` (or the shell its `#!` line names).
    Shell,
}

impl FileLanguage {
    /// The language of an LSP language id.
    pub fn from_language_id(id: &str) -> Option<Self> {
        Some(match id {
            "python" => Self::Python,
            "rust" => Self::Rust,
            "c" => Self::C,
            "cpp" => Self::Cpp,
            "go" => Self::Go,
            "javascript" | "javascriptreact" => Self::JavaScript,
            "typescript" | "typescriptreact" => Self::TypeScript,
            "java" => Self::Java,
            "ruby" => Self::Ruby,
            "php" => Self::Php,
            "lua" => Self::Lua,
            "shellscript" | "bash" | "sh" | "zsh" => Self::Shell,
            _ => return None,
        })
    }

    /// The language of a file name's extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "py" | "pyw" => Self::Python,
            "rs" => Self::Rust,
            "c" => Self::C,
            "cc" | "cpp" | "cxx" | "c++" => Self::Cpp,
            "go" => Self::Go,
            "js" | "mjs" | "cjs" => Self::JavaScript,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "java" => Self::Java,
            "rb" => Self::Ruby,
            "php" => Self::Php,
            "lua" => Self::Lua,
            "sh" | "bash" | "zsh" => Self::Shell,
            _ => return None,
        })
    }

    /// The language of a file opened as `language_id`, else by its
    /// extension (a `.py` file opened as plain text still runs). A C or
    /// C++ header has none: it does not run on its own.
    pub fn detect(language_id: &str, path: &Path) -> Option<Self> {
        if is_header(path) {
            return None;
        }
        Self::from_language_id(language_id).or_else(|| Self::from_path(path))
    }

    /// The name shown in messages.
    pub fn name(self) -> &'static str {
        match self {
            Self::Python => "Python",
            Self::Rust => "Rust",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Go => "Go",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Java => "Java",
            Self::Ruby => "Ruby",
            Self::Php => "PHP",
            Self::Lua => "Lua",
            Self::Shell => "shell",
        }
    }

    /// Whether Debug Current File can debug it (with an installed
    /// adapter): Python (debugpy), Rust, C and C++ (built with debug
    /// info, then a native adapter) and Go (Delve).
    pub fn can_debug(self) -> bool {
        matches!(
            self,
            Self::Python | Self::Rust | Self::C | Self::Cpp | Self::Go
        )
    }
}

/// Finds the programs [`file_command`] needs. [`SystemTools`] looks on
/// `PATH`; tests pass their own.
pub trait Tools {
    /// The program's path when it is installed.
    fn find(&self, program: &str) -> Option<PathBuf>;
    /// The `(major, minor)` version of `node`.
    fn node_version(&self) -> Option<(u32, u32)>;
}

/// [`Tools`] of this system: `PATH` lookups and `node --version`.
pub struct SystemTools;

impl Tools for SystemTools {
    fn find(&self, program: &str) -> Option<PathBuf> {
        crate::packs::find_executable(program)
    }

    fn node_version(&self) -> Option<(u32, u32)> {
        let out = Command::new(self.find("node")?)
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        parse_node_version(&String::from_utf8_lossy(&out.stdout))
    }
}

/// `v22.6.0` → `(22, 6)`.
pub fn parse_node_version(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// What to run.
#[derive(Debug, Clone, Copy)]
pub struct FileRunInput<'a> {
    /// The source file (absolute).
    pub file: &'a Path,
    /// The editor's LSP language id (the extension decides when it is not
    /// a known one).
    pub language_id: &'a str,
    /// The open project folder, if any: the working directory for files
    /// inside it, and where Cargo packages, Go modules and virtualenvs are
    /// looked for.
    pub project_root: Option<&'a Path>,
    /// Where compiled programs go.
    pub output: ProgramOutput<'a>,
}

/// A command from [`file_run`], and a note for the user when its program
/// could not go where it was asked to (a name next to the source is
/// taken: see [`taken_note`]).
#[derive(Debug, Clone)]
pub struct FileRun {
    /// What to run.
    pub command: RunCommand,
    /// One line saying where the program went instead, and why.
    pub note: Option<String>,
}

/// The command that runs `input.file` in a terminal, labelled
/// `run <file name>`. A compiled language's command has a
/// [`RunCommand::compile`] step that builds the program, with debug
/// info, where [`FileRunInput::output`] says: next to the source as
/// `./<stem>` run from the file's folder, or in a folder of the cache.
/// Create the folder with [`create_output_dir`] before running it.
///
/// Errors are short sentences for the user: the language is not supported
/// (`Cannot run Markdown files: …`) or its toolchain is missing
/// (`Install Python 3 to run test.py`).
pub fn file_run(input: &FileRunInput<'_>, tools: &dyn Tools) -> Result<FileRun, String> {
    let language = FileLanguage::detect(input.language_id, input.file)
        .ok_or_else(|| unsupported_message(input.file, input.language_id))?;
    langs::command(language, input, tools)
}

/// [`file_run`] without the note.
pub fn file_command(input: &FileRunInput<'_>, tools: &dyn Tools) -> Result<RunCommand, String> {
    file_run(input, tools).map(|run| run.command)
}

/// The program a compiled single-file command runs, as a full path (a
/// program next to the source is `./<stem>` in the command's folder).
/// `None` for other commands.
pub fn compiled_program(command: &RunCommand) -> Option<PathBuf> {
    command.compile.as_ref()?;
    let program = Path::new(&command.program);
    let program = program.strip_prefix(".").unwrap_or(program);
    Some(command.cwd.join(program))
}

/// Creates the folder a compiled single-file program goes to (nothing to
/// do for other commands). Cheap: one `mkdir -p`.
pub fn create_output_dir(command: &RunCommand) -> std::io::Result<()> {
    match compiled_program(command).as_deref().and_then(Path::parent) {
        Some(dir) if !dir.as_os_str().is_empty() => std::fs::create_dir_all(dir),
        _ => Ok(()),
    }
}

/// Whether `path` is a C or C++ header (`.h`, `.hpp`, ...), which is
/// included by source files rather than compiled and run.
fn is_header(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(
        ext.as_deref(),
        Some("h" | "hh" | "hpp" | "hxx" | "h++" | "inl" | "ipp" | "tpp")
    )
}

/// What to do with a header instead of running or debugging it.
fn header_message(verb: &str, file: &Path) -> String {
    format!(
        "Cannot {verb} {}: a header does not run on its own. {} a .c or .cpp file \
         that includes it.",
        file_name(file),
        if verb == "run" { "Run" } else { "Debug" }
    )
}

/// Why a file of an unsupported language (`language_name`, as the editor
/// names it) cannot run.
pub fn unsupported_message(file: &Path, language_name: &str) -> String {
    if is_header(file) {
        return header_message("run", file);
    }
    format!(
        "Cannot run {}: there is no runner for {language_name} files. Set a run command \
         to run something else.",
        file_name(file)
    )
}

/// Why Debug Current File cannot debug a file of `language` (`None`: a
/// language it does not know at all, named `language_name`).
pub fn debug_unsupported_message(
    language: Option<FileLanguage>,
    language_name: &str,
    file: &Path,
) -> String {
    if is_header(file) {
        return header_message("debug", file);
    }
    let name = file_name(file);
    let shown = language.map_or(language_name, |l| l.name());
    format!(
        "Cannot debug {name}: there is no debugger for {shown} files. Add a configuration \
         to .arcode/launch.toml to debug another program."
    )
}

/// The working directory for a file: the project folder when the file is
/// inside it, else the file's folder.
pub(crate) fn working_dir(file: &Path, project_root: Option<&Path>) -> PathBuf {
    match project_root {
        Some(root) if file.starts_with(root) => root.to_path_buf(),
        _ => fsutil::start_dir(file),
    }
}

/// The file's name for messages and labels.
pub(crate) fn file_name(file: &Path) -> String {
    file.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.display().to_string())
}

#[cfg(test)]
mod output_side_tests;
#[cfg(test)]
mod output_tests;
#[cfg(test)]
mod tests;
