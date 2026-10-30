//! Runnable items in a source file (mains, tests, benchmarks) and the
//! command that runs each one.
//!
//! Locating the items is not done here: tree-sitter queries in
//! `arcode-syntax` find them and report a [`RunnableKind`] and a name; this
//! module turns that into a [`RunCommand`].

mod go;
mod js;
mod python;
mod rust;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::command::RunCommand;

/// What kind of runnable item a gutter button stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunnableKind {
    /// A program entry point (`fn main`, `func main`, `if __name__ == ...`).
    Main,
    /// A single test function.
    Test,
    /// A group of tests: a Rust `mod tests`, a Python test class, a JS
    /// `describe` block, or the whole file when the name is empty.
    TestModule,
    /// A benchmark.
    Benchmark,
}

/// A runnable item at a line of a file, with its command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runnable {
    /// Zero-based line of the item (where the gutter button goes).
    pub line: usize,
    /// Main, test, test module or benchmark.
    pub kind: RunnableKind,
    /// Item name as passed to [`command_for_runnable`].
    pub name: String,
    /// The command that runs it.
    pub command: RunCommand,
}

impl Runnable {
    /// Builds a runnable with its command; `None` when the language is not
    /// supported or the project layout gives no command.
    pub fn new(
        language_id: &str,
        file: &Path,
        project_root: &Path,
        line: usize,
        kind: RunnableKind,
        name: &str,
    ) -> Option<Self> {
        let command = command_for_runnable(language_id, file, project_root, kind, name)?;
        Some(Self {
            line,
            kind,
            name: name.to_string(),
            command,
        })
    }
}

/// Builds the command that runs one runnable item.
///
/// `language_id` is the LSP id (`rust`, `go`, `python`, `javascript`,
/// `typescript`, `javascriptreact`, `typescriptreact`). `name` is:
///
/// - Rust: the item path inside the file's module, `::`-separated
///   (`it_works`, `tests::it_works`); for a test module, the module path
///   (`tests`). The module path of the file itself is added from its location
///   (`src/foo/bar.rs` → `foo::bar::`). Gives `cargo test --package <pkg>
///   [--lib|--bin b|--test t] -- <path> --exact --nocapture`, `cargo bench`,
///   or `cargo run --package <pkg> --bin <bin>` / `--example <ex>`.
/// - Go: the function name (`TestFoo`, `TestFoo/sub` for subtests,
///   `BenchmarkX`); gives `go test -run '^TestFoo$' ./pkg`, `go run ./pkg`.
/// - Python: the test (`test_x`, `TestCls::test_x` or `TestCls.test_x`);
///   gives `python -m pytest <file>::<name>` or `python <file>`.
/// - JS/TS: the test or `describe` title; uses vitest or jest when the
///   nearest `package.json` depends on it, else `node --test` (or
///   `bun test` in bun projects).
///
/// `name` is ignored for [`RunnableKind::Main`]. Returns `None` for other
/// languages or when no project is found (e.g. a Rust file outside any
/// Cargo package).
pub fn command_for_runnable(
    language_id: &str,
    file: &Path,
    project_root: &Path,
    kind: RunnableKind,
    name: &str,
) -> Option<RunCommand> {
    match language_id {
        "rust" => rust::command(file, project_root, kind, name),
        "go" => go::command(file, project_root, kind, name),
        "python" => python::command(file, project_root, kind, name),
        "javascript" | "typescript" | "javascriptreact" | "typescriptreact" => {
            js::command(file, project_root, kind, name)
        }
        _ => None,
    }
}

/// The language ids [`command_for_runnable`] supports.
pub const RUNNABLE_LANGUAGES: [&str; 7] = [
    "rust",
    "go",
    "python",
    "javascript",
    "typescript",
    "javascriptreact",
    "typescriptreact",
];

/// A short label such as `test tests::it_works`.
pub(crate) fn label(kind: RunnableKind, name: &str) -> String {
    let verb = match kind {
        RunnableKind::Main => return "run".to_string(),
        RunnableKind::Test => "test",
        RunnableKind::TestModule => "tests",
        RunnableKind::Benchmark => "bench",
    };
    if name.is_empty() {
        verb.to_string()
    } else {
        format!("{verb} {name}")
    }
}
