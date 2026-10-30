//! Commands for Go runnables (`go test -run`, `go run`).

use std::path::Path;

use super::{RunnableKind, label};
use crate::command::{RunCommand, RunKind, RunSource, args};
use crate::fsutil;

pub(super) fn command(
    file: &Path,
    root: &Path,
    kind: RunnableKind,
    name: &str,
) -> Option<RunCommand> {
    let dir = fsutil::start_dir(file);
    let module = fsutil::ancestors(&dir, Some(root))
        .into_iter()
        .find(|d| d.join("go.mod").is_file())
        .unwrap_or_else(|| root.to_path_buf());
    let rel = fsutil::rel_slash(&dir, &module);
    let pkg = if rel == "." {
        ".".to_string()
    } else {
        format!("./{rel}")
    };
    let pattern = anchored(name);
    let (list, run_kind) = match kind {
        RunnableKind::Main => (args(&["run", &pkg]), RunKind::Run),
        RunnableKind::Test if !name.is_empty() => {
            (args(&["test", "-run", &pattern, &pkg]), RunKind::Test)
        }
        RunnableKind::Test | RunnableKind::TestModule => (args(&["test", &pkg]), RunKind::Test),
        RunnableKind::Benchmark => (
            args(&["test", "-run", "^$", "-bench", &pattern, &pkg]),
            RunKind::Test,
        ),
    };
    let shown = if kind == RunnableKind::Main { "" } else { name };
    Some(
        RunCommand::new("go", list, &module, run_kind, RunSource::Go)
            .with_label(label(kind, shown)),
    )
}

/// `TestA/sub case` → `^TestA$/^sub_case$` (go test turns spaces into `_`).
fn anchored(name: &str) -> String {
    if name.is_empty() {
        return ".".to_string();
    }
    name.split('/')
        .map(|part| format!("^{}$", regex_escape(&part.replace(' ', "_"))))
        .collect::<Vec<_>>()
        .join("/")
}

/// Escapes regex metacharacters.
pub(super) fn regex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
