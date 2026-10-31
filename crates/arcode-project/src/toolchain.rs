//! Toolchain hints read from project files: Python virtual environments,
//! Node and Python version files, the Rust toolchain channel.

use std::fs;
use std::path::{Path, PathBuf};

use crate::fsutil;

/// Directory names checked for an in-project virtual environment, in order.
pub const VENV_DIR_NAMES: [&str; 3] = [".venv", "venv", "env"];

/// A Python virtual environment inside a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonVenv {
    /// The environment directory, e.g. `<root>/.venv`.
    pub dir: PathBuf,
    /// Its interpreter: `bin/python`, or `Scripts\python.exe` on Windows.
    pub python: PathBuf,
}

/// The interpreter path inside a virtual environment for this platform.
pub fn venv_python(venv_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        venv_dir.join("Scripts").join("python.exe")
    } else {
        venv_dir.join("bin").join("python")
    }
}

/// The Python program to use when there is no virtual environment:
/// `python` on Windows, `python3` elsewhere (many Linux systems lack `python`).
pub fn default_python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

impl PythonVenv {
    /// Whether [`PythonVenv::python`] exists. It does not for a venv created
    /// on another OS (e.g. a Windows venv opened from WSL), which cannot be
    /// used here.
    pub fn has_interpreter(&self) -> bool {
        self.python.is_file()
    }
}

/// Finds a virtual environment directly inside `root` (`.venv`, `venv`, `env`).
///
/// A directory counts when it has a `pyvenv.cfg` or the interpreter exists;
/// check [`PythonVenv::has_interpreter`] before running its interpreter, or
/// use [`find_usable_python_venv`].
pub fn detect_python_venv(root: &Path) -> Option<PythonVenv> {
    venvs_in(root).next()
}

fn venvs_in(root: &Path) -> impl Iterator<Item = PythonVenv> + '_ {
    VENV_DIR_NAMES.iter().filter_map(move |name| {
        let dir = root.join(name);
        let python = venv_python(&dir);
        (dir.join("pyvenv.cfg").is_file() || python.is_file()).then_some(PythonVenv { dir, python })
    })
}

/// Finds the nearest virtual environment in `start` or its ancestors, up to
/// `boundary` (or the version-control root).
pub fn find_python_venv(start: &Path, boundary: Option<&Path>) -> Option<PythonVenv> {
    fsutil::ancestors(start, boundary)
        .iter()
        .find_map(|dir| detect_python_venv(dir))
}

/// Like [`find_python_venv`], but only environments whose interpreter for
/// this platform exists.
pub fn find_usable_python_venv(start: &Path, boundary: Option<&Path>) -> Option<PythonVenv> {
    fsutil::ancestors(start, boundary)
        .iter()
        .find_map(|dir| venvs_in(dir).find(PythonVenv::has_interpreter))
}

/// The interpreter to run for files under `start`: the nearest usable
/// virtual environment's interpreter, else [`default_python`].
pub fn python_program(start: &Path, boundary: Option<&Path>) -> String {
    match find_usable_python_venv(start, boundary) {
        Some(venv) => venv.python.to_string_lossy().into_owned(),
        None => default_python().to_string(),
    }
}

/// The Node version requested by `.nvmrc` or `.node-version` in `root`.
pub fn detect_node_version_file(root: &Path) -> Option<String> {
    [".nvmrc", ".node-version"]
        .iter()
        .find_map(|name| first_line(&root.join(name)))
}

/// The Python version requested by `.python-version` in `root`.
pub fn detect_python_version_file(root: &Path) -> Option<String> {
    first_line(&root.join(".python-version"))
}

/// The Rust channel pinned by `rust-toolchain.toml` (or the legacy
/// `rust-toolchain` file, plain or TOML) in `root`.
pub fn rust_toolchain_channel(root: &Path) -> Option<String> {
    let toml_channel = |path: &Path| {
        let table = fsutil::read_toml(path)?;
        let channel = table.get("toolchain")?.get("channel")?.as_str()?;
        Some(channel.to_string())
    };
    if let Some(channel) = toml_channel(&root.join("rust-toolchain.toml")) {
        return Some(channel);
    }
    let legacy = root.join("rust-toolchain");
    toml_channel(&legacy).or_else(|| first_line(&legacy).filter(|l| !l.contains('[')))
}

/// First non-empty, non-comment line of a small text file, trimmed.
fn first_line(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
}
