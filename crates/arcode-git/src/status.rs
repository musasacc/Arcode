//! Working tree status from `git status --porcelain=v2 -z`.

use std::path::PathBuf;

use crate::exec::{exec, path_from_git};
use crate::{GitError, GitResult, Repo};

/// The state of one side (index or working tree) of a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusCode {
    /// No change on this side.
    Unmodified,
    /// Content changed.
    Modified,
    /// File type changed (e.g. regular file to symlink).
    TypeChanged,
    /// Newly added.
    Added,
    /// Removed.
    Deleted,
    /// Renamed from [`FileStatus::orig_path`].
    Renamed,
    /// Copied from [`FileStatus::orig_path`].
    Copied,
    /// Not tracked (both sides report this).
    Untracked,
    /// Ignored (both sides; only with [`status_with_ignored`]).
    Ignored,
    /// Unmerged: part of a merge conflict (both sides report this).
    Unmerged,
}

/// Which sides of an unmerged path changed (git's two-letter conflict code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConflictKind {
    /// `UU`: modified on both sides.
    BothModified,
    /// `AA`: added on both sides.
    BothAdded,
    /// `DD`: deleted on both sides.
    BothDeleted,
    /// `AU`: added by us.
    AddedByUs,
    /// `UA`: added by them.
    AddedByThem,
    /// `DU`: deleted by us.
    DeletedByUs,
    /// `UD`: deleted by them.
    DeletedByThem,
}

/// The status of one path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileStatus {
    /// Path relative to the working tree root (native separators).
    pub path: PathBuf,
    /// Staged state (HEAD vs index).
    pub index: StatusCode,
    /// Unstaged state (index vs working tree).
    pub worktree: StatusCode,
    /// The source path of a rename or copy.
    pub orig_path: Option<PathBuf>,
    /// Set for unmerged paths.
    pub conflict: Option<ConflictKind>,
}

impl FileStatus {
    /// True when the path is part of an unresolved merge conflict.
    pub fn is_conflicted(&self) -> bool {
        self.conflict.is_some()
    }

    /// True when the index differs from `HEAD` for this path.
    pub fn is_staged(&self) -> bool {
        !matches!(
            self.index,
            StatusCode::Unmodified
                | StatusCode::Untracked
                | StatusCode::Ignored
                | StatusCode::Unmerged
        )
    }

    /// True when the working tree differs from the index (tracked files).
    pub fn is_unstaged(&self) -> bool {
        !matches!(
            self.worktree,
            StatusCode::Unmodified
                | StatusCode::Untracked
                | StatusCode::Ignored
                | StatusCode::Unmerged
        )
    }
}

/// Counts for a status bar or SCM panel header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct StatusSummary {
    /// Paths with staged changes.
    pub staged: usize,
    /// Tracked paths with unstaged changes.
    pub unstaged: usize,
    /// Untracked paths.
    pub untracked: usize,
    /// Unmerged paths.
    pub conflicted: usize,
    /// Ignored paths (only counted when listed).
    pub ignored: usize,
}

impl StatusSummary {
    /// Tallies `entries`; a path can count as both staged and unstaged.
    pub fn from_entries(entries: &[FileStatus]) -> Self {
        let mut sum = Self::default();
        for entry in entries {
            match entry.index {
                StatusCode::Untracked => sum.untracked += 1,
                StatusCode::Ignored => sum.ignored += 1,
                StatusCode::Unmerged => sum.conflicted += 1,
                _ => {
                    sum.staged += usize::from(entry.is_staged());
                    sum.unstaged += usize::from(entry.is_unstaged());
                }
            }
        }
        sum
    }

    /// True when nothing is changed, untracked or conflicted.
    pub fn is_clean(&self) -> bool {
        self.staged + self.unstaged + self.untracked + self.conflicted == 0
    }
}

/// Lists changed, untracked and unmerged paths (not ignored ones).
///
/// Runs `git status --porcelain=v2 -z --untracked-files=all`. Untracked
/// directories are expanded into their files.
pub fn status(repo: &Repo) -> GitResult<Vec<FileStatus>> {
    run_status(repo, false)
}

/// Like [`status`], but also lists ignored paths ([`StatusCode::Ignored`]).
/// Ignored directories are reported as one entry ending at the directory.
pub fn status_with_ignored(repo: &Repo) -> GitResult<Vec<FileStatus>> {
    run_status(repo, true)
}

/// Runs [`status`] and returns its [`StatusSummary`].
pub fn status_summary(repo: &Repo) -> GitResult<StatusSummary> {
    Ok(StatusSummary::from_entries(&status(repo)?))
}

/// The paths with unresolved merge conflicts.
pub fn conflicted_files(repo: &Repo) -> GitResult<Vec<PathBuf>> {
    let entries = status(repo)?;
    Ok(entries
        .into_iter()
        .filter(FileStatus::is_conflicted)
        .map(|e| e.path)
        .collect())
}

fn run_status(repo: &Repo, ignored: bool) -> GitResult<Vec<FileStatus>> {
    let mut args = vec!["status", "--porcelain=v2", "-z", "--untracked-files=all"];
    if ignored {
        args.push("--ignored=matching");
    }
    parse_porcelain_v2(&exec(repo, &args, None)?)
}

/// Parses NUL-separated `git status --porcelain=v2 -z` output.
pub(crate) fn parse_porcelain_v2(out: &[u8]) -> GitResult<Vec<FileStatus>> {
    let mut records = out.split(|&b| b == 0).filter(|r| !r.is_empty());
    let mut entries = Vec::new();
    while let Some(record) = records.next() {
        let bad = || GitError::Parse(String::from_utf8_lossy(record).into_owned());
        let (kind, rest) = record.split_first().ok_or_else(bad)?;
        let entry = match kind {
            b'1' => ordinary(rest, 7).ok_or_else(bad)?,
            b'2' => {
                let mut entry = ordinary(rest, 8).ok_or_else(bad)?;
                let orig = records.next().ok_or_else(bad)?;
                entry.orig_path = Some(path_from_git(orig));
                entry
            }
            b'u' => unmerged(rest).ok_or_else(bad)?,
            b'?' => whole(rest, StatusCode::Untracked).ok_or_else(bad)?,
            b'!' => whole(rest, StatusCode::Ignored).ok_or_else(bad)?,
            // Headers (`# branch.*`) and future record types.
            _ => continue,
        };
        entries.push(entry);
    }
    Ok(entries)
}

/// Splits ` field1 field2 ... path` into `fields` fields and the path (which
/// may contain spaces).
fn fields(rest: &[u8], fields: usize) -> Option<(Vec<&[u8]>, &[u8])> {
    let rest = rest.strip_prefix(b" ")?;
    let mut parts = rest.splitn(fields + 1, |&b| b == b' ');
    let head: Vec<&[u8]> = parts.by_ref().take(fields).collect();
    let path = parts.next().filter(|p| !p.is_empty())?;
    (head.len() == fields).then_some((head, path))
}

/// `1 XY sub mH mI mW hH hI path` (7 fields) or `2 ... Xscore path` (8).
fn ordinary(rest: &[u8], count: usize) -> Option<FileStatus> {
    let (head, path) = fields(rest, count)?;
    let xy = head.first()?;
    if xy.len() != 2 {
        return None;
    }
    Some(FileStatus {
        path: path_from_git(path),
        index: code(xy[0])?,
        worktree: code(xy[1])?,
        orig_path: None,
        conflict: None,
    })
}

/// `u XY sub m1 m2 m3 mW h1 h2 h3 path`.
fn unmerged(rest: &[u8]) -> Option<FileStatus> {
    let (head, path) = fields(rest, 9)?;
    let conflict = match *head.first()? {
        b"UU" => ConflictKind::BothModified,
        b"AA" => ConflictKind::BothAdded,
        b"DD" => ConflictKind::BothDeleted,
        b"AU" => ConflictKind::AddedByUs,
        b"UA" => ConflictKind::AddedByThem,
        b"DU" => ConflictKind::DeletedByUs,
        b"UD" => ConflictKind::DeletedByThem,
        _ => return None,
    };
    Some(FileStatus {
        path: path_from_git(path),
        index: StatusCode::Unmerged,
        worktree: StatusCode::Unmerged,
        orig_path: None,
        conflict: Some(conflict),
    })
}

/// `? path` / `! path`.
fn whole(rest: &[u8], code: StatusCode) -> Option<FileStatus> {
    let path = rest.strip_prefix(b" ").filter(|p| !p.is_empty())?;
    Some(FileStatus {
        path: path_from_git(path),
        index: code,
        worktree: code,
        orig_path: None,
        conflict: None,
    })
}

fn code(byte: u8) -> Option<StatusCode> {
    Some(match byte {
        b'.' => StatusCode::Unmodified,
        b'M' => StatusCode::Modified,
        b'T' => StatusCode::TypeChanged,
        b'A' => StatusCode::Added,
        b'D' => StatusCode::Deleted,
        b'R' => StatusCode::Renamed,
        b'C' => StatusCode::Copied,
        b'U' => StatusCode::Unmerged,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
