//! Error type for git operations that can fail in user-visible ways.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Result alias for fallible git operations.
pub type GitResult<T> = Result<T, GitError>;

/// Why a git operation failed. `Display` gives a message suitable for a
/// notification.
#[derive(Debug)]
#[non_exhaustive]
pub enum GitError {
    /// The `git` executable was not found on `PATH`.
    GitNotFound,
    /// Spawning or talking to git failed.
    Io(io::Error),
    /// The repository's working directory no longer exists (e.g. the
    /// project folder was moved or deleted while open).
    WorkdirMissing(PathBuf),
    /// The path is outside the working tree or not valid UTF-8.
    InvalidPath(PathBuf),
    /// git exited unsuccessfully; `stderr` holds its message (trimmed).
    Command {
        /// The git subcommand and arguments, for diagnostics.
        args: String,
        /// The exit code, if the process was not killed by a signal.
        code: Option<i32>,
        /// git's error output.
        stderr: String,
    },
    /// git produced output this crate could not parse.
    Parse(String),
    /// The file is not tracked in the index (stage the whole file instead).
    NotInIndex(PathBuf),
    /// The file content is not UTF-8 text.
    Binary(PathBuf),
    /// The hunk no longer matches the index; recompute the diff.
    StaleHunk,
    /// `commit` was called with an empty (whitespace-only) message.
    EmptyMessage,
    /// `commit` was called with nothing staged.
    NothingToCommit,
    /// The index has unmerged (conflicted) paths.
    UnmergedPaths(Vec<PathBuf>),
    /// `user.name` / `user.email` are not configured; holds git's hint.
    IdentityNotConfigured(String),
    /// No local branch with this name exists.
    BranchNotFound(String),
    /// The operation would need to contact a remote, which Arcode never does.
    NetworkRequired(String),
    /// git rejects this name for a new branch.
    InvalidBranchName(String),
    /// `--amend` was requested in a repository without commits.
    NothingToAmend,
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitNotFound => f.write_str("git is not installed or not on PATH"),
            Self::Io(err) => write!(f, "failed to run git: {err}"),
            Self::WorkdirMissing(dir) => {
                write!(f, "the folder {} no longer exists", dir.display())
            }
            Self::InvalidPath(path) => write!(f, "invalid path: {}", path.display()),
            Self::Command { args, stderr, .. } if stderr.is_empty() => {
                write!(f, "git {args} failed")
            }
            Self::Command { args, stderr, .. } => write!(f, "git {args} failed: {stderr}"),
            Self::Parse(what) => write!(f, "unexpected git output: {what}"),
            Self::NotInIndex(path) => write!(f, "{} is not tracked", path.display()),
            Self::Binary(path) => write!(f, "{} is not a text file", path.display()),
            Self::StaleHunk => f.write_str("the change no longer matches the index; refresh"),
            Self::EmptyMessage => f.write_str("the commit message is empty"),
            Self::NothingToCommit => f.write_str("nothing staged to commit"),
            Self::UnmergedPaths(paths) => {
                write!(f, "{} file(s) have unresolved conflicts", paths.len())
            }
            Self::IdentityNotConfigured(hint) => write!(
                f,
                "git user identity is not configured (set user.name and user.email): {hint}"
            ),
            Self::BranchNotFound(name) => write!(f, "no local branch named {name:?}"),
            Self::NetworkRequired(msg) => {
                write!(f, "operation would need network access: {msg}")
            }
            Self::InvalidBranchName(name) => write!(f, "{name:?} is not a valid branch name"),
            Self::NothingToAmend => f.write_str("there is no commit to amend yet"),
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl GitError {
    /// Maps an error from spawning git in `workdir`. A missing `current_dir`
    /// is checked first: Unix reports it as `NotFound` (the same kind as a
    /// missing git binary), Windows as an invalid-directory error.
    pub(crate) fn from_spawn(err: io::Error, workdir: &Path) -> Self {
        if !workdir.is_dir() {
            return Self::WorkdirMissing(workdir.to_path_buf());
        }
        match err.kind() {
            io::ErrorKind::NotFound => Self::GitNotFound,
            _ => Self::Io(err),
        }
    }
}

impl From<io::Error> for GitError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}
