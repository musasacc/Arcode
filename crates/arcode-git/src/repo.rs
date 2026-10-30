//! Repository discovery: walks up from a path to find `.git`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::paths;

/// A discovered git repository (or linked worktree / submodule checkout).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Repo {
    /// The working tree root: the directory containing `.git`.
    pub workdir: PathBuf,
    /// The repository's git directory. For a linked worktree or submodule
    /// this is the directory the `.git` file points to.
    pub git_dir: PathBuf,
    /// The shared git directory (from `git_dir/commondir`), holding refs,
    /// objects and config. Equal to `git_dir` for ordinary repositories.
    pub common_dir: PathBuf,
}

/// Finds the repository containing `path`, which may be a file or directory
/// (it does not need to exist).
///
/// Walks up from `path` looking for a `.git` directory, or a `.git` file of
/// the form `gitdir: <path>` (linked worktrees, submodules); relative gitdir
/// paths are resolved against the directory holding the `.git` file. Only
/// the filesystem is read; no process is spawned.
///
/// Returns `None` if no repository is found, or if the nearest `.git` file
/// points to a missing git directory (git itself refuses such a checkout).
pub fn find_repo(path: &Path) -> Option<Repo> {
    let start = paths::absolute(path);
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start
    };
    loop {
        match probe(&dir) {
            Probe::Found(repo) => return Some(repo),
            Probe::Broken => return None,
            Probe::NotHere => {}
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Outcome of checking a single directory for `.git`.
enum Probe {
    Found(Repo),
    Broken,
    NotHere,
}

fn probe(dir: &Path) -> Probe {
    let dot_git = dir.join(".git");
    let Ok(meta) = fs::metadata(&dot_git) else {
        return Probe::NotHere;
    };
    if meta.is_dir() {
        if is_git_dir(&dot_git) {
            return Probe::Found(Repo::from_parts(dir.to_path_buf(), dot_git));
        }
        return Probe::NotHere;
    }
    if !meta.is_file() {
        return Probe::NotHere;
    }
    match read_gitdir_file(&dot_git) {
        Some(target) => {
            let git_dir = paths::resolve(dir, &target);
            if is_git_dir(&git_dir) {
                Probe::Found(Repo::from_parts(dir.to_path_buf(), git_dir))
            } else {
                Probe::Broken
            }
        }
        None => Probe::Broken,
    }
}

/// Parses a `.git` file (`gitdir: <path>`), returning the raw target path.
fn read_gitdir_file(file: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(file).ok()?;
    parse_gitdir_line(&text)
}

/// Extracts the path from `gitdir: <path>` (first line, surrounding
/// whitespace trimmed).
fn parse_gitdir_line(text: &str) -> Option<PathBuf> {
    let line = text.lines().next()?;
    let target = line.strip_prefix("gitdir:")?.trim();
    if target.is_empty() {
        None
    } else {
        Some(PathBuf::from(target))
    }
}

/// A directory counts as a git dir when it has a `HEAD` file. This is the
/// minimum git needs to resolve a branch and keeps detection cheap.
fn is_git_dir(dir: &Path) -> bool {
    dir.join("HEAD").is_file()
}

/// Reads `git_dir/commondir` (relative to `git_dir` unless absolute).
fn read_common_dir(git_dir: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(git_dir.join("commondir")).ok()?;
    let target = text.lines().next()?.trim();
    if target.is_empty() {
        return None;
    }
    Some(paths::resolve(git_dir, Path::new(target)))
}

impl Repo {
    fn from_parts(workdir: PathBuf, git_dir: PathBuf) -> Self {
        let common_dir = read_common_dir(&git_dir).unwrap_or_else(|| git_dir.clone());
        Self {
            workdir,
            git_dir,
            common_dir,
        }
    }

    /// Returns `path` relative to the working tree, or `None` when it lies
    /// outside it. Relative inputs are returned normalized as-is.
    pub fn relative_path(&self, path: &Path) -> Option<PathBuf> {
        if path.is_relative() {
            let rel = paths::normalize(path);
            return paths::to_git_path(&rel).map(|_| rel);
        }
        let abs = paths::normalize(path);
        let rel = abs.strip_prefix(&self.workdir).ok()?;
        paths::to_git_path(rel).map(|_| rel.to_path_buf())
    }

    /// True when this checkout is a linked worktree or submodule, i.e. its
    /// git dir is not `workdir/.git`.
    pub fn is_linked(&self) -> bool {
        self.git_dir != self.workdir.join(".git")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gitdir_lines() {
        assert_eq!(
            parse_gitdir_line("gitdir: ../.git/worktrees/w\n"),
            Some(PathBuf::from("../.git/worktrees/w"))
        );
        assert_eq!(
            parse_gitdir_line("gitdir:/abs/path\r\n"),
            Some(PathBuf::from("/abs/path"))
        );
        assert_eq!(parse_gitdir_line("gitdir:   \n"), None);
        assert_eq!(parse_gitdir_line("nonsense"), None);
        assert_eq!(parse_gitdir_line(""), None);
    }

    #[test]
    fn relative_path_strips_workdir() {
        let workdir = std::path::absolute("/w/repo").unwrap();
        let repo = Repo {
            git_dir: workdir.join(".git"),
            common_dir: workdir.join(".git"),
            workdir: workdir.clone(),
        };
        let file = workdir.join("src").join("lib.rs");
        assert_eq!(
            repo.relative_path(&file),
            Some(PathBuf::from("src").join("lib.rs"))
        );
        assert_eq!(repo.relative_path(&workdir), None);
        let outside = std::path::absolute("/w/other/x").unwrap();
        assert_eq!(repo.relative_path(&outside), None);
        assert_eq!(
            repo.relative_path(Path::new("a/./b")),
            Some(PathBuf::from("a").join("b"))
        );
        assert_eq!(repo.relative_path(Path::new("../x")), None);
        assert!(!repo.is_linked());
    }
}
