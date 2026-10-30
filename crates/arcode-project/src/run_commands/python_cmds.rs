//! Run commands for Python projects.

use std::fs;
use std::path::Path;

use crate::command::{RunCommand, RunKind, RunSource, args};
use crate::{fsutil, toolchain};

/// Entry scripts in the project root, with extra arguments.
const ENTRY_POINTS: [(&str, &[&str]); 4] = [
    ("main.py", &[]),
    ("app.py", &[]),
    ("manage.py", &["runserver"]),
    ("__main__.py", &[]),
];

/// `python <entry>` for `main.py`, `app.py`, `manage.py runserver` and
/// `__main__.py`, then `python -m pytest` when pytest is configured. Uses the
/// project's virtualenv interpreter when there is one.
pub(super) fn detect(root: &Path) -> Vec<RunCommand> {
    let python = toolchain::python_program(root, Some(root));
    let cmd = |list: Vec<String>, kind| {
        RunCommand::new(python.clone(), list, root, kind, RunSource::Python)
    };
    let mut out = Vec::new();
    for (file, extra) in ENTRY_POINTS {
        if root.join(file).is_file() {
            let mut list = vec![file.to_string()];
            list.extend(extra.iter().map(|s| (*s).to_string()));
            out.push(cmd(list, RunKind::Run).with_label(label(file, extra)));
        }
    }
    if uses_pytest(root) {
        out.push(cmd(args(&["-m", "pytest"]), RunKind::Test).with_label("python -m pytest"));
    }
    out
}

/// A label without the (possibly long) virtualenv path.
fn label(file: &str, extra: &[&str]) -> String {
    std::iter::once("python")
        .chain(std::iter::once(file))
        .chain(extra.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

/// pytest is configured explicitly, or `tests/` holds pytest-style files.
pub(crate) fn uses_pytest(root: &Path) -> bool {
    if root.join("pytest.ini").is_file() || root.join("conftest.py").is_file() {
        return true;
    }
    let mentions = |file: &str, needle: &str| {
        fs::read_to_string(root.join(file)).is_ok_and(|t| t.contains(needle))
    };
    if mentions("pyproject.toml", "[tool.pytest")
        || mentions("setup.cfg", "[tool:pytest]")
        || mentions("tox.ini", "[pytest]")
    {
        return true;
    }
    let tests = root.join("tests");
    let names = fsutil::list_dir(&tests, |p| p.is_file());
    names.iter().any(|n| {
        n.ends_with(".py")
            && (n.starts_with("test_") || n.ends_with("_test.py") || n == "conftest.py")
    })
}
