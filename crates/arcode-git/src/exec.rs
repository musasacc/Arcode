//! Running git for operations that report errors: captures stderr, feeds
//! optional stdin, and maps failures to [`GitError`].

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::cli::git_command;
use crate::{GitError, GitResult, Repo, paths};

/// Runs git with `args` in the repository's working tree and returns stdout.
///
/// `input`, when given, is written to git's stdin from a helper thread (so a
/// large input cannot deadlock against git's output); otherwise stdin is
/// null. Messages are forced to English (`LANGUAGE=en`) so callers can
/// recognize them.
pub(crate) fn exec<S: AsRef<OsStr>>(
    repo: &Repo,
    args: &[S],
    input: Option<&[u8]>,
) -> GitResult<Vec<u8>> {
    let mut cmd = git_command(&repo.workdir);
    cmd.args(args).env("LANGUAGE", "en").stderr(Stdio::piped());
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take();
    let output = std::thread::scope(|scope| {
        if let (Some(mut pipe), Some(bytes)) = (stdin, input) {
            // A write error means git exited early; its status reports why.
            scope.spawn(move || {
                let _ = pipe.write_all(bytes);
            });
        }
        child.wait_with_output()
    })?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    Err(GitError::Command {
        args: describe(args),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

/// Like [`exec`] but returns stdout as UTF-8 text.
pub(crate) fn exec_text<S: AsRef<OsStr>>(
    repo: &Repo,
    args: &[S],
    input: Option<&[u8]>,
) -> GitResult<String> {
    let out = exec(repo, args, input)?;
    String::from_utf8(out).map_err(|_| GitError::Parse("output is not UTF-8".into()))
}

/// Joins arguments for error messages.
fn describe<S: AsRef<OsStr>>(args: &[S]) -> String {
    args.iter()
        .map(|a| a.as_ref().to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A path checked to lie inside the working tree.
pub(crate) struct RepoPath {
    /// Relative to the working tree, with native separators.
    pub rel: PathBuf,
    /// Relative, `/`-separated, as git spells it.
    pub git: String,
}

/// Validates `path` (relative to the working tree, or absolute inside it).
pub(crate) fn repo_path(repo: &Repo, path: &Path) -> GitResult<RepoPath> {
    let invalid = || GitError::InvalidPath(path.to_path_buf());
    let rel = repo.relative_path(path).ok_or_else(invalid)?;
    let git = paths::to_git_path(&rel).ok_or_else(invalid)?;
    Ok(RepoPath { rel, git })
}

/// Converts a `/`-separated path printed by git into a native relative path.
pub(crate) fn path_from_git(bytes: &[u8]) -> PathBuf {
    // git prints raw bytes: UTF-8 on Windows and almost always on Unix.
    let text = String::from_utf8_lossy(bytes);
    let mut out = PathBuf::new();
    for part in text.split('/').filter(|p| !p.is_empty()) {
        out.push(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_paths_become_native() {
        let expected: PathBuf = ["src", "a b.rs"].iter().collect();
        assert_eq!(path_from_git(b"src/a b.rs"), expected);
        assert_eq!(path_from_git("ü.txt".as_bytes()), PathBuf::from("ü.txt"));
    }

    #[test]
    fn describe_joins_args() {
        assert_eq!(describe(&["apply", "--cached"]), "apply --cached");
    }
}
