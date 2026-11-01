//! Running a single source file: the fallback of the Run button when the
//! folder has no project file (`Cargo.toml`, `package.json`, ...) to take
//! a command from. Compiled languages are built first, then run.

use std::path::Path;

use crate::command::quote_arg;

/// A shell line that builds (when the language needs it) and runs `file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRun {
    /// Short name for the terminal tab.
    pub label: String,
    /// The line to type into a shell.
    pub line: String,
}

/// The command that runs `file` on its own, chosen by its extension;
/// `None` for files that are not programs.
pub fn run_file_command(file: &Path) -> Option<FileRun> {
    let name = file.file_name()?.to_str()?;
    let ext = file.extension()?.to_str()?.to_ascii_lowercase();
    let f = quote_arg(file.to_str()?).into_owned();
    // The build output goes next to nothing the user owns.
    let out = "\"${TMPDIR:-/tmp}/arcode-run-$$\"";
    let cc = |compiler: &str| format!("{compiler} {f} -o {out} && {out}; rm -f {out}");
    let line = match ext.as_str() {
        "rs" => cc("rustc"),
        "c" => cc("cc"),
        "cc" | "cpp" | "cxx" => cc("c++"),
        "py" => format!("python3 {f}"),
        "js" | "mjs" | "cjs" => format!("node {f}"),
        "ts" | "mts" => format!("npx --yes tsx {f}"),
        "go" => format!("go run {f}"),
        "sh" | "bash" => format!("bash {f}"),
        "rb" => format!("ruby {f}"),
        "php" => format!("php {f}"),
        "lua" => format!("lua {f}"),
        "pl" => format!("perl {f}"),
        "java" => format!("java {f}"),
        "kts" => format!("kotlin {f}"),
        "swift" => format!("swift {f}"),
        "dart" => format!("dart run {f}"),
        "r" => format!("Rscript {f}"),
        _ => return None,
    };
    Some(FileRun {
        label: name.to_string(),
        line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_get_a_command() {
        let py = run_file_command(Path::new("/p/a b.py")).unwrap();
        assert_eq!(py.line, "python3 '/p/a b.py'");
        assert_eq!(py.label, "a b.py");
        let rs = run_file_command(Path::new("/p/main.rs")).unwrap();
        assert!(rs.line.starts_with("rustc /p/main.rs -o "), "{}", rs.line);
        assert!(rs.line.contains("&& "), "{}", rs.line);
        assert!(run_file_command(Path::new("/p/notes.txt")).is_none());
    }
}
