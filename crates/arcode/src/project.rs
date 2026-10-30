//! An open project: its root folder, git repository and file listing.

use std::path::{Path, PathBuf};

use arcode_git::Repo;
use ignore::WalkBuilder;

/// Folder names never shown in listings, even without a `.gitignore`.
const SKIPPED_DIRS: &[&str] = &[".git", "target", "node_modules"];

/// Files that mark a project root when no git repository is found.
const ROOT_MARKERS: &[&str] = &["Cargo.toml", "package.json", "go.mod", "pyproject.toml"];

/// An open project folder.
pub struct Project {
    /// Canonical root folder.
    pub root: PathBuf,
    /// Display name (the folder name).
    pub name: String,
    /// The git repository containing the root, if any.
    pub repo: Option<Repo>,
}

impl Project {
    /// Opens `root` (which must be a directory).
    pub fn open(root: &Path) -> std::io::Result<Self> {
        let root = std::fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                format!("{} is not a folder", root.display()),
            ));
        }
        let name = arcode_config::state::project_name(&root);
        let repo = arcode_git::find_repo(&root);
        Ok(Self { root, name, repo })
    }

    /// The current branch for display (`main`, `detached@abc1234`).
    pub fn branch(&self) -> Option<String> {
        self.repo.as_ref().map(Repo::branch_display)
    }

    /// The command that runs this project, if one can be detected.
    pub fn run_command(&self) -> Option<&'static str> {
        detect_run_command(&self.root)
    }

    /// `path` relative to the root, with `/` separators, for display.
    pub fn relative_display(&self, path: &Path) -> String {
        relative_display(&self.root, path)
    }
}

/// `path` relative to `root` with `/` separators (the full path if outside).
pub fn relative_display(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect();
    parts.join("/")
}

/// Detects how to run the project in `root` from its package files.
pub fn detect_run_command(root: &Path) -> Option<&'static str> {
    [
        ("Cargo.toml", "cargo run"),
        ("package.json", "npm start"),
        ("go.mod", "go run ."),
        ("main.py", "python main.py"),
    ]
    .into_iter()
    .find(|(file, _)| root.join(file).is_file())
    .map(|(_, command)| command)
}

/// The project root for a file opened from the command line: the nearest
/// ancestor that is a git work tree, else the nearest one containing a
/// package file, else the file's parent folder.
pub fn root_for_file(file: &Path) -> PathBuf {
    let parent = file.parent().unwrap_or(file).to_path_buf();
    if let Some(dir) = parent.ancestors().find(|dir| dir.join(".git").exists()) {
        return dir.to_path_buf();
    }
    parent
        .ancestors()
        .find(|dir| ROOT_MARKERS.iter().any(|m| dir.join(m).is_file()))
        .map(Path::to_path_buf)
        .unwrap_or(parent)
}

/// Whether a folder name is always hidden from listings.
pub fn is_skipped_dir(name: &str) -> bool {
    SKIPPED_DIRS.contains(&name)
}

fn walker(dir: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(dir);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .require_git(false)
        .parents(true)
        .filter_entry(|entry| {
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            !(is_dir && entry.file_name().to_str().is_some_and(is_skipped_dir))
        });
    builder
}

/// Every file in the project (respecting `.gitignore`), as paths relative
/// to `root` with `/` separators, sorted. Stops after `limit` files.
pub fn list_files(root: &Path, limit: usize) -> Vec<String> {
    let mut files = Vec::new();
    for entry in walker(root).build().flatten() {
        if entry.file_type().is_some_and(|t| t.is_file()) {
            files.push(relative_display(root, entry.path()));
            if files.len() >= limit {
                log::warn!("file listing stopped at {limit} files");
                break;
            }
        }
    }
    files.sort_unstable();
    files
}

/// A direct child of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    /// Absolute path.
    pub path: PathBuf,
    /// File name.
    pub name: String,
    /// Whether it is a folder.
    pub is_dir: bool,
}

/// The children of `dir` (respecting `.gitignore`): folders first, then
/// files, each sorted case-insensitively.
pub fn read_dir_sorted(dir: &Path) -> Vec<DirEntry> {
    let mut entries: Vec<DirEntry> = walker(dir)
        .max_depth(Some(1))
        .build()
        .flatten()
        .filter(|e| e.depth() == 1)
        .map(|e| DirEntry {
            is_dir: e.file_type().is_some_and(|t| t.is_dir()),
            name: e.file_name().to_string_lossy().into_owned(),
            path: e.into_path(),
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    entries
}

/// Creates a new, uniquely named project folder inside `parent`.
pub fn create_project_dir(parent: &Path) -> std::io::Result<PathBuf> {
    for n in 1..1000 {
        let name = if n == 1 {
            "untitled-project".to_string()
        } else {
            format!("untitled-project-{n}")
        };
        let path = parent.join(name);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::other("too many untitled projects"))
}

#[cfg(test)]
mod tests;
