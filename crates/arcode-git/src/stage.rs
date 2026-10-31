//! Staging and unstaging single hunks or whole files.

use std::path::{Path, PathBuf};

use crate::exec::{RepoPath, exec, exec_text, repo_path};
use crate::hunk::{Base, blob_text, diff_hunks};
use crate::patch::{Block, apply_hunk, hunk_patch, new_file_patch};
use crate::{GitError, GitResult, Head, Hunk, Repo};

/// Stages one gutter hunk (from [`crate::diff_against_index`]) of
/// `rel_path`, taking the new lines from the editor buffer `current_text`.
///
/// Builds a one-hunk unified diff against the index and applies it with
/// `git apply --cached --unidiff-zero`; the working tree file is not read
/// or written. For an untracked file the buffer is staged as a new file;
/// for a tracked file missing from the index (`git rm --cached`) the hunk
/// is applied to the `HEAD` version, which is staged as a new file.
/// When git converts line endings for the path (`text`/`eol` attributes or
/// `core.autocrlf`), CRLF in the buffer is staged as LF.
///
/// git only splits lines at `\n`: when the hunk shares such a line with a
/// neighbouring hunk (a lone `\r` or U+2028 in the text), both are staged.
///
/// Fails with [`GitError::StaleHunk`] when `hunk` is not one of the hunks
/// between the current index and `current_text` (recompute and retry).
pub fn stage_hunk(repo: &Repo, rel_path: &Path, hunk: &Hunk, current_text: &str) -> GitResult<()> {
    let path = repo_path(repo, rel_path)?;
    let index = text_of(repo, &path, ":0:")?;
    let head = match index {
        Some(_) => None,
        None if is_unmerged(repo, &path)? => return Err(GitError::UnmergedPaths(vec![path.rel])),
        None => text_of(repo, &path, "HEAD:")?,
    };
    let base = index.as_deref().or(head.as_deref()).unwrap_or_default();
    let normalized;
    let target =
        if !base.contains('\r') && current_text.contains("\r\n") && converts_eol(repo, &path)? {
            normalized = current_text.replace("\r\n", "\n");
            normalized.as_str()
        } else {
            current_text
        };
    let all = diff_hunks(base, target);
    if !all.contains(hunk) {
        return Err(GitError::StaleHunk);
    }
    let blocks: Vec<Block> = all.iter().map(|h| (h.old_range(), h.new_range())).collect();
    let this = (hunk.old_range(), hunk.new_range());
    let patch = if index.is_some() {
        hunk_patch(&path.git, base, target, this, &blocks)?
    } else {
        let text = match head {
            Some(_) => apply_hunk(base, target, this, &blocks)?,
            None => target.to_owned(),
        };
        new_file_patch(&path.git, &text, is_executable(repo, &path))
    };
    apply_cached(repo, &patch)
}

/// Unstages one hunk from [`crate::staged_hunks`] (`HEAD` against the
/// index), restoring the `HEAD` lines in the index only. Unstaging a
/// staged new file or a staged deletion restores the path's `HEAD` state
/// (like [`unstage_file`]).
pub fn unstage_hunk(repo: &Repo, rel_path: &Path, hunk: &Hunk) -> GitResult<()> {
    let path = repo_path(repo, rel_path)?;
    let index = text_of(repo, &path, ":0:")?;
    if index.is_none() && is_unmerged(repo, &path)? {
        return Err(GitError::UnmergedPaths(vec![path.rel]));
    }
    let head = text_of(repo, &path, "HEAD:")?;
    if index.is_none() && head.is_none() {
        return Err(GitError::NotInIndex(path.rel));
    }
    let (head_text, index_text) = (
        head.as_deref().unwrap_or_default(),
        index.as_deref().unwrap_or_default(),
    );
    let hunks = diff_hunks(head_text, index_text);
    if !hunks.contains(hunk) {
        return Err(GitError::StaleHunk);
    }
    if index.is_none() || (head.is_none() && hunks.len() == 1) {
        // A staged deletion, or the whole new file: restore the HEAD entry
        // (or drop the path) instead of patching text.
        return unstage_file(repo, rel_path);
    }
    // Reversed: the patch turns the index back into HEAD.
    let blocks: Vec<Block> = hunks
        .iter()
        .map(|h| (h.new_range(), h.old_range()))
        .collect();
    let this = (hunk.new_range(), hunk.old_range());
    let patch = hunk_patch(&path.git, index_text, head_text, this, &blocks)?;
    apply_cached(repo, &patch)
}

/// The UTF-8 blob `<prefix><path>`, `None` when missing.
fn text_of(repo: &Repo, path: &RepoPath, prefix: &str) -> GitResult<Option<String>> {
    match blob_text(repo, &path.rel, prefix) {
        Base::Text(text) => Ok(Some(text)),
        Base::Binary => Err(GitError::Binary(path.rel.clone())),
        Base::Missing => Ok(None),
    }
}

/// Stages the working tree state of `rel_path` (`git add -A -- <path>`),
/// including a deletion. Pathspec magic is disabled.
pub fn stage_file(repo: &Repo, rel_path: &Path) -> GitResult<()> {
    let path = repo_path(repo, rel_path)?;
    run_literal(repo, &["add", "-A", "--", &path.git])
}

/// Stages the working tree state of exactly `rel_paths` (`git add -A` with
/// the paths fed on stdin, so any number of them fits): modifications,
/// deletions and untracked files, nothing else. An unmerged path in the
/// list is marked resolved, so callers leave conflicts out unless the user
/// picked them. Pathspec magic is disabled.
pub fn stage_paths(repo: &Repo, rel_paths: &[PathBuf]) -> GitResult<()> {
    if rel_paths.is_empty() {
        return Ok(());
    }
    let mut input = Vec::new();
    for rel in rel_paths {
        input.extend_from_slice(repo_path(repo, rel)?.git.as_bytes());
        input.push(0);
    }
    let args = [
        "--literal-pathspecs",
        "add",
        "-A",
        "--pathspec-from-file=-",
        "--pathspec-file-nul",
    ];
    exec(repo, &args, Some(&input)).map(drop)
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
pub(crate) fn is_unmerged(repo: &Repo, path: &RepoPath) -> GitResult<bool> {
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
