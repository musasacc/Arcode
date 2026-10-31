//! Repository-level operations for the source control view: initializing a
//! repository, creating branches, discarding changes, amending commits and
//! finding repositories below a folder. All local; none touch the network.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::git_command;
use crate::commit::identity_error;
use crate::exec::{exec, exec_text, repo_path};
use crate::{FileStatus, GitError, GitResult, Head, Repo, find_repo, status};

/// Options for [`commit_with`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommitOptions {
    /// Replace the last commit (`git commit --amend`) instead of adding one.
    /// Amending with nothing staged only changes the message.
    pub amend: bool,
}

/// Creates a repository in `dir` (`git init`) and returns it.
///
/// Only call this when the user asked for it. Fails when git is missing or
/// `dir` is not a folder.
pub fn init_repo(dir: &Path) -> GitResult<Repo> {
    if !dir.is_dir() {
        return Err(GitError::WorkdirMissing(dir.to_path_buf()));
    }
    let output = git_command(dir)
        .args(["init", "-q"])
        .env("LANGUAGE", "en")
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|err| GitError::from_spawn(err, dir))?;
    if !output.status.success() {
        return Err(GitError::Command {
            args: "init".into(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    find_repo(dir).ok_or_else(|| GitError::Parse("git init created no repository".into()))
}

/// Creates the local branch `name` at `HEAD` and switches to it
/// (`git switch -c`). Fails with [`GitError::InvalidBranchName`] for a name
/// git would reject.
pub fn create_branch(repo: &Repo, name: &str) -> GitResult<()> {
    let name = name.trim();
    if !is_valid_branch_name(repo, name) {
        return Err(GitError::InvalidBranchName(name.to_owned()));
    }
    exec(repo, &["switch", "-q", "-c", name], None).map(drop)
}

/// True when git accepts `name` as a new branch name.
fn is_valid_branch_name(repo: &Repo, name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && exec(repo, &["check-ref-format", "--branch", name], None).is_ok()
}

/// Throws away the working tree changes of `rel_path`.
///
/// A tracked file is restored from the index (`git restore --worktree`),
/// so staged changes stay. An untracked file is deleted. This cannot be
/// undone; ask the user first.
pub fn discard_changes(repo: &Repo, rel_path: &Path) -> GitResult<()> {
    let path = repo_path(repo, rel_path)?;
    let in_index = !exec(
        repo,
        &["--literal-pathspecs", "ls-files", "-z", "--", &path.git],
        None,
    )?
    .is_empty();
    if in_index {
        let args = [
            "--literal-pathspecs",
            "restore",
            "--worktree",
            "--",
            &path.git,
        ];
        return exec(repo, &args, None).map(drop);
    }
    let file = repo.workdir.join(&path.rel);
    match fs::symlink_metadata(&file) {
        Ok(meta) if meta.is_dir() => Err(GitError::InvalidPath(path.rel)),
        Ok(_) => fs::remove_file(&file).map_err(GitError::Io),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(GitError::Io(err)),
    }
}

/// Commits the index like [`crate::commit`], optionally amending the last
/// commit. Returns the new commit's short id.
pub fn commit_with(repo: &Repo, message: &str, options: CommitOptions) -> GitResult<String> {
    if !options.amend {
        return crate::commit(repo, message);
    }
    if message.trim().is_empty() {
        return Err(GitError::EmptyMessage);
    }
    if matches!(repo.head(), Head::Unborn(_)) {
        return Err(GitError::NothingToAmend);
    }
    let unmerged: Vec<PathBuf> = status(repo)?
        .into_iter()
        .filter(FileStatus::is_conflicted)
        .map(|e| e.path)
        .collect();
    if !unmerged.is_empty() {
        return Err(GitError::UnmergedPaths(unmerged));
    }
    exec(
        repo,
        &["commit", "-q", "--amend", "--file=-"],
        Some(message.as_bytes()),
    )
    .map_err(identity_error)?;
    let sha = exec_text(repo, &["rev-parse", "--short", "HEAD"], None)?;
    Ok(sha.trim().to_owned())
}

/// The full message of the `HEAD` commit (for amending), or `None` in a
/// repository without commits.
pub fn last_commit_message(repo: &Repo) -> GitResult<Option<String>> {
    if matches!(repo.head(), Head::Unborn(_)) {
        return Ok(None);
    }
    let text = exec_text(repo, &["log", "-1", "--format=%B"], None)?;
    Ok(Some(text.trim_end().to_owned()))
}

/// Stages every change (`git add -A`), or only changes to tracked files
/// (`git add -u`) when `tracked_only`.
pub fn stage_all(repo: &Repo, tracked_only: bool) -> GitResult<()> {
    let flag = if tracked_only { "-u" } else { "-A" };
    exec(repo, &["add", flag], None).map(drop)
}

/// Removes every staged change from the index, keeping the working tree
/// (`git reset`, or emptying the index in a repository without commits).
pub fn unstage_all(repo: &Repo) -> GitResult<()> {
    if matches!(repo.head(), Head::Unborn(_)) {
        let args = [
            "rm",
            "--cached",
            "-r",
            "-q",
            "-f",
            "--ignore-unmatch",
            "--",
            ".",
        ];
        return exec(repo, &args, None).map(drop);
    }
    exec(repo, &["reset", "-q"], None).map(drop)
}

/// Folder names never searched for nested repositories.
const SKIPPED: &[&str] = &["node_modules", "target", "build", "dist", "vendor"];
/// Upper bound on folders visited by [`discover_repos`].
const MAX_DIRS: usize = 4000;

/// The repositories relevant to a project folder: the one containing
/// `root` (if any) followed by repositories in sub-folders up to
/// `max_depth` levels below `root`, sorted by path.
///
/// Hidden folders and common build output folders are skipped and the
/// walk stops after a few thousand folders. Only the file system is read.
pub fn discover_repos(root: &Path, max_depth: usize) -> Vec<Repo> {
    let mut repos = Vec::new();
    if let Some(repo) = find_repo(root) {
        repos.push(repo);
    }
    let mut nested = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut visited = 0;
    while let Some((dir, depth)) = stack.pop() {
        visited += 1;
        if visited > MAX_DIRS {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with('.') || SKIPPED.contains(&name) {
                continue;
            }
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let path = entry.path();
            if path.join(".git").exists() {
                if let Some(repo) = find_repo(&path).filter(|r| r.workdir == path) {
                    nested.push(repo);
                }
                // A nested repository's own sub-repositories are its
                // submodules; they are not listed separately.
                continue;
            }
            if depth + 1 < max_depth {
                stack.push((path, depth + 1));
            }
        }
    }
    nested.sort_by(|a, b| a.workdir.cmp(&b.workdir));
    nested.retain(|r| !repos.contains(r));
    repos.extend(nested);
    repos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_nested_repositories_only_by_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for repo in ["a", "b/c", "node_modules/x", ".hidden/y"] {
            let git = root.join(repo).join(".git");
            fs::create_dir_all(&git).unwrap();
            fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        }
        let repos = discover_repos(root, 3);
        let names: Vec<_> = repos
            .iter()
            .map(|r| r.workdir.strip_prefix(root).unwrap().to_path_buf())
            .collect();
        assert_eq!(names, [PathBuf::from("a"), Path::new("b").join("c")]);
        assert!(
            discover_repos(root, 1)
                .iter()
                .all(|r| r.workdir != root.join("b/c"))
        );
    }
}
