//! Staging and unstaging single hunks or whole files.

use std::path::Path;

use crate::exec::{RepoPath, exec, exec_text, repo_path};
use crate::hunk::{Base, blob_text, diff_hunks};
use crate::patch::{hunk_patch, new_file_patch};
use crate::{GitError, GitResult, Head, Hunk, Repo};

/// Stages one gutter hunk (from [`crate::diff_against_index`]) of
/// `rel_path`, taking the new lines from the editor buffer `current_text`.
///
/// Builds a one-hunk unified diff against the index and applies it with
/// `git apply --cached --unidiff-zero`; the working tree file is not read
/// or written. For an untracked file the buffer is staged as a new file.
/// When git converts line endings for the path (`text`/`eol` attributes or
/// `core.autocrlf`), CRLF in the buffer is staged as LF.
///
/// Fails with [`GitError::StaleHunk`] when `hunk` is not one of the hunks
/// between the current index and `current_text` (recompute and retry).
pub fn stage_hunk(repo: &Repo, rel_path: &Path, hunk: &Hunk, current_text: &str) -> GitResult<()> {
    let path = repo_path(repo, rel_path)?;
    let index = match blob_text(repo, rel_path, ":0:") {
        Base::Text(text) => Some(text),
        Base::Binary => return Err(GitError::Binary(path.rel)),
        Base::Missing => None,
    };
    if index.is_none() && is_unmerged(repo, &path)? {
        return Err(GitError::UnmergedPaths(vec![path.rel]));
    }
    let base = index.as_deref().unwrap_or_default();
    let normalized;
    let target =
        if !base.contains('\r') && current_text.contains("\r\n") && converts_eol(repo, &path)? {
            normalized = current_text.replace("\r\n", "\n");
            normalized.as_str()
        } else {
            current_text
        };
    if !diff_hunks(base, target).contains(hunk) {
        return Err(GitError::StaleHunk);
    }
    let patch = match index {
        Some(_) => hunk_patch(&path.git, base, target, hunk.old_range(), hunk.new_range())?,
        None => new_file_patch(&path.git, target, is_executable(repo, &path)),
    };
    apply_cached(repo, &patch)
}

/// Unstages one hunk from [`crate::staged_hunks`] (`HEAD` against the
/// index), restoring the `HEAD` lines in the index only.
pub fn unstage_hunk(repo: &Repo, rel_path: &Path, hunk: &Hunk) -> GitResult<()> {
    let path = repo_path(repo, rel_path)?;
    let index = match blob_text(repo, rel_path, ":0:") {
        Base::Text(text) => text,
        Base::Binary => return Err(GitError::Binary(path.rel)),
        _ => return Err(GitError::NotInIndex(path.rel)),
    };
    let head = match blob_text(repo, rel_path, "HEAD:") {
        Base::Text(text) => Some(text),
        Base::Binary => return Err(GitError::Binary(path.rel)),
        Base::Missing => None,
    };
    let head_text = head.as_deref().unwrap_or_default();
    let hunks = diff_hunks(head_text, &index);
    if !hunks.contains(hunk) {
        return Err(GitError::StaleHunk);
    }
    if head.is_none() && hunks.len() == 1 {
        // The whole new file: drop it from the index instead of emptying it.
        return unstage_file(repo, rel_path);
    }
    let patch = hunk_patch(
        &path.git,
        &index,
        head_text,
        hunk.new_range(),
        hunk.old_range(),
    )?;
    apply_cached(repo, &patch)
}

/// Stages the working tree state of `rel_path` (`git add -A -- <path>`),
/// including a deletion. Pathspec magic is disabled.
pub fn stage_file(repo: &Repo, rel_path: &Path) -> GitResult<()> {
    let path = repo_path(repo, rel_path)?;
    run_literal(repo, &["add", "-A", "--", &path.git])
}

/// Removes staged changes of `rel_path` from the index, keeping the working
/// tree: `git restore --staged`, or `git rm --cached` in a repository
/// without commits.
pub fn unstage_file(repo: &Repo, rel_path: &Path) -> GitResult<()> {
    let path = repo_path(repo, rel_path)?;
    if matches!(repo.head(), Head::Unborn(_)) {
        run_literal(repo, &["rm", "--cached", "-f", "-q", "--", &path.git])
    } else {
        run_literal(repo, &["restore", "--staged", "--", &path.git])
    }
}

/// Runs a pathspec-taking command with `--literal-pathspecs`, so names
/// containing `*`, `?` or `:(...)` match only themselves.
fn run_literal(repo: &Repo, args: &[&str]) -> GitResult<()> {
    let mut all = vec!["--literal-pathspecs"];
    all.extend_from_slice(args);
    exec(repo, &all, None).map(drop)
}

fn apply_cached(repo: &Repo, patch: &str) -> GitResult<()> {
    let args = [
        "apply",
        "--cached",
        "--unidiff-zero",
        "--whitespace=nowarn",
        "-",
    ];
    exec(repo, &args, Some(patch.as_bytes())).map(drop)
}

/// True when the index holds conflict stages for the path.
fn is_unmerged(repo: &Repo, path: &RepoPath) -> GitResult<bool> {
    let args = [
        "--literal-pathspecs",
        "ls-files",
        "-u",
        "-z",
        "--",
        &path.git,
    ];
    Ok(!exec(repo, &args, None)?.is_empty())
}

/// True when git normalizes line endings of the path to LF in the index.
fn converts_eol(repo: &Repo, path: &RepoPath) -> GitResult<bool> {
    let args = ["check-attr", "-z", "text", "eol", "--", &path.git];
    let out = exec_text(repo, &args, None)?;
    let fields: Vec<&str> = out.split('\0').collect();
    // Records are `path NUL attribute NUL value NUL`.
    let value = |attr: &str| {
        fields
            .chunks(3)
            .find(|rec| rec.get(1) == Some(&attr))
            .and_then(|rec| rec.get(2).copied())
            .unwrap_or("unspecified")
    };
    match value("text") {
        "unset" => return Ok(false),
        "unspecified" => {}
        _ => return Ok(true),
    }
    if value("eol") != "unspecified" {
        return Ok(true);
    }
    let autocrlf = exec_text(repo, &["config", "--get", "core.autocrlf"], None)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    Ok(matches!(
        autocrlf.as_str(),
        "true" | "yes" | "on" | "1" | "input"
    ))
}

#[cfg(unix)]
fn is_executable(repo: &Repo, path: &RepoPath) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(repo.workdir.join(&path.rel))
        .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_repo: &Repo, _path: &RepoPath) -> bool {
    false
}
