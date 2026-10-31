//! New-project templates: an empty folder, or a folder plus one command
//! that sets it up (`cargo init`, `npm init -y`, ...).
//!
//! Creating the folder never runs anything. The template command is only
//! returned; the UI runs it in the integrated terminal when the user
//! clicks Create with that template selected, so its output is visible.

use std::io;
use std::path::{Path, PathBuf};

use crate::command::command_line;
use crate::files::validate_name;

/// A way to start a new project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectTemplate {
    /// Stable identifier (`empty`, `rust-bin`, ...).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// One-line description.
    pub description: &'static str,
    /// Program run in the new folder, if any.
    pub program: Option<&'static str>,
    /// Its arguments; `{name}` is replaced by the package name.
    pub args: &'static [&'static str],
}

const PYTHON: &str = if cfg!(windows) { "python" } else { "python3" };

static TEMPLATES: &[ProjectTemplate] = &[
    ProjectTemplate {
        id: "empty",
        name: "Empty folder",
        description: "Just the folder; nothing is run.",
        program: None,
        args: &[],
    },
    ProjectTemplate {
        id: "rust-bin",
        name: "Rust application",
        description: "cargo init (needs Rust installed)",
        program: Some("cargo"),
        args: &["init", "--name", "{name}"],
    },
    ProjectTemplate {
        id: "rust-lib",
        name: "Rust library",
        description: "cargo init --lib (needs Rust installed)",
        program: Some("cargo"),
        args: &["init", "--lib", "--name", "{name}"],
    },
    ProjectTemplate {
        id: "node",
        name: "Node.js package",
        description: "npm init -y (needs Node.js installed)",
        program: Some("npm"),
        args: &["init", "-y"],
    },
    ProjectTemplate {
        id: "python",
        name: "Python virtual environment",
        description: "python -m venv .venv (needs Python installed)",
        program: Some(PYTHON),
        args: &["-m", "venv", ".venv"],
    },
    ProjectTemplate {
        id: "go",
        name: "Go module",
        description: "go mod init (needs Go installed)",
        program: Some("go"),
        args: &["mod", "init", "{name}"],
    },
];

/// Every template; the first is the empty folder.
pub fn templates() -> &'static [ProjectTemplate] {
    TEMPLATES
}

/// The template with `id`.
pub fn template(id: &str) -> Option<&'static ProjectTemplate> {
    TEMPLATES.iter().find(|t| t.id == id)
}

impl ProjectTemplate {
    /// The command line to type into a terminal opened in the new folder,
    /// or `None` for templates that run nothing.
    pub fn command_line(&self, folder_name: &str) -> Option<String> {
        let program = self.program?;
        let package = package_name(folder_name);
        let args: Vec<String> = self
            .args
            .iter()
            .map(|a| a.replace("{name}", &package))
            .collect();
        Some(command_line(program, &args))
    }
}

/// A package name derived from a folder name: lowercase ASCII letters,
/// digits, `-` and `_`; other characters become `-`. Never empty and never
/// starting with a digit.
pub fn package_name(folder_name: &str) -> String {
    let mut name: String = folder_name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    name = name.trim_matches('-').to_owned();
    if name.is_empty() {
        name = "app".to_owned();
    }
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert_str(0, "app-");
    }
    name
}

/// Creates the folder `name` in `parent` for a new project. Fails when the
/// name is invalid or the folder exists.
pub fn create_project(parent: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name).map_err(|m| io::Error::new(io::ErrorKind::InvalidInput, m))?;
    if !parent.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} is not a folder", parent.display()),
        ));
    }
    let path = parent.join(name);
    std::fs::create_dir(&path).map_err(|err| {
        if err.kind() == io::ErrorKind::AlreadyExists {
            io::Error::new(err.kind(), format!("{} already exists", path.display()))
        } else {
            err
        }
    })?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_and_commands() {
        assert_eq!(templates()[0].id, "empty");
        assert_eq!(template("empty").unwrap().command_line("x"), None);
        assert_eq!(
            template("rust-bin").unwrap().command_line("My App"),
            Some("cargo init --name my-app".to_owned())
        );
        assert_eq!(
            template("go").unwrap().command_line("svc"),
            Some("go mod init svc".to_owned())
        );
        assert!(template("nope").is_none());
        let ids: std::collections::HashSet<_> = templates().iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), templates().len());
    }

    #[test]
    fn package_names() {
        assert_eq!(package_name("Hello World!"), "hello-world");
        assert_eq!(package_name("___"), "___");
        assert_eq!(package_name("¿?"), "app");
        assert_eq!(package_name("2048"), "app-2048");
    }

    #[test]
    fn creating_project_folders() {
        let dir = tempfile::tempdir().unwrap();
        let made = create_project(dir.path(), "demo").unwrap();
        assert!(made.is_dir());
        let err = create_project(dir.path(), "demo").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(create_project(dir.path(), "a/b").is_err());
        assert!(create_project(&dir.path().join("missing"), "x").is_err());
    }
}
