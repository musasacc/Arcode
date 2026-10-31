//! Creating commits from the index.

use crate::exec::{exec, exec_text};
use crate::{FileStatus, GitError, GitResult, Repo, status};

/// git messages that mean the user identity is missing or unusable.
const IDENTITY_ERRORS: &[&str] = &[
    "Please tell me who you are",
    "Author identity unknown",
    "Committer identity unknown",
    "empty ident name",
    "unable to auto-detect email address",
    "no email was given",
    "no name was given",
];

/// Commits the index with `message` and returns the new commit's short id.
///
/// Uses the configured `user.name` / `user.email` (never overridden) and
/// runs the repository's hooks. Fails with:
/// - [`GitError::EmptyMessage`] for a blank message,
/// - [`GitError::UnmergedPaths`] while conflicts are unresolved,
/// - [`GitError::NothingToCommit`] when nothing is staged (unless a merge
///   is being concluded),
/// - [`GitError::IdentityNotConfigured`] when git has no usable identity.
pub fn commit(repo: &Repo, message: &str) -> GitResult<String> {
    if message.trim().is_empty() {
        return Err(GitError::EmptyMessage);
    }
    let entries = status(repo)?;
    let unmerged: Vec<_> = entries
        .iter()
        .filter(|e| e.is_conflicted())
        .map(|e| e.path.clone())
        .collect();
    if !unmerged.is_empty() {
        return Err(GitError::UnmergedPaths(unmerged));
    }
    let merging = repo.git_dir.join("MERGE_HEAD").is_file();
    if !merging && !entries.iter().any(FileStatus::is_staged) {
        return Err(GitError::NothingToCommit);
    }
    for var in ["GIT_AUTHOR_IDENT", "GIT_COMMITTER_IDENT"] {
        exec(repo, &["var", var], None).map_err(identity_error)?;
    }
    // The message goes through stdin: no argument length or quoting limits.
    exec(
        repo,
        &["commit", "-q", "--file=-"],
        Some(message.as_bytes()),
    )
    .map_err(identity_error)?;
    let sha = exec_text(repo, &["rev-parse", "--short", "HEAD"], None)?;
    Ok(sha.trim().to_owned())
}

/// Turns git's identity complaints into [`GitError::IdentityNotConfigured`].
pub(crate) fn identity_error(err: GitError) -> GitError {
    match err {
        GitError::Command { stderr, .. } if IDENTITY_ERRORS.iter().any(|m| stderr.contains(m)) => {
            let hint = stderr
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or_default()
                .trim()
                .to_owned();
            GitError::IdentityNotConfigured(hint)
        }
        other => other,
    }
}
