//! Reviewer tests for the status/staging/commit/blame/branch extension.
//! Each test demonstrates a defect found during review.

mod common;

use arcode_git::{
    GitError, Head, Repo, checkout_branch, diff_against_index, stage_hunk, staged_hunks, status,
    unstage_hunk,
};
use common::{TestRepo, p};

/// `checkout_branch` must only switch to an existing local branch. A
/// revision expression such as `main~1` resolves under `refs/heads/` in
/// the existence check, so git then checks out the commit and detaches HEAD.
#[test]
fn checkout_branch_rejects_revision_expressions() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("a.txt", "1\n", "one");
    t.commit_file("a.txt", "2\n", "two");
    let repo = t.repo();
    for name in ["main~1", "main^", "main@{1}"] {
        let result = checkout_branch(&repo, name);
        assert!(
            matches!(result, Err(GitError::BranchNotFound(_))),
            "{name}: {result:?}, head is now {:?}",
            repo.head()
        );
        assert_eq!(repo.head(), Head::Branch("main".into()), "{name}");
    }
}

/// After `git rm --cached`, the gutter diffs against HEAD (documented
/// fallback), but `stage_hunk` diffs against an empty base, so every gutter
/// hunk is rejected as stale forever (refreshing gives the same hunks).
#[test]
fn hunks_of_file_removed_from_index_can_be_staged() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("f.txt", "a\nb\nc\n", "init");
    t.git(&["rm", "-q", "--cached", "f.txt"]);
    let repo = t.repo();
    let buffer = "a\nB\nc\n";
    let hunks = diff_against_index(&repo, p("f.txt"), buffer);
    assert_eq!(hunks.len(), 1, "gutter diffs against HEAD");
    let result = stage_hunk(&repo, p("f.txt"), &hunks[0], buffer);
    assert!(result.is_ok(), "{result:?}");
}

/// `staged_hunks` reports a staged deletion (`git rm`) as one deleted hunk
/// and says to pass it to `unstage_hunk`, which then fails with
/// `NotInIndex`.
#[test]
fn staged_deletion_hunk_can_be_unstaged() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("f.txt", "a\nb\n", "init");
    t.git(&["rm", "-q", "f.txt"]);
    let repo = t.repo();
    let staged = staged_hunks(&repo, p("f.txt")).unwrap();
    assert_eq!(staged.len(), 1);
    let result = unstage_hunk(&repo, p("f.txt"), &staged[0]);
    assert!(result.is_ok(), "{result:?}");
}

/// Widening a hunk to `\n` boundaries moves old and new starts in lockstep.
/// When the widening crosses into an earlier hunk of a different size, the
/// patch removes base lines and adds target lines that do not correspond,
/// leaving an index that is neither the old index nor the buffer (here
/// the first buffer line `B1` is lost while `A` is removed).
#[test]
fn lone_cr_widening_does_not_corrupt_index() {
    let Some(t) = TestRepo::new() else { return };
    let base = "A\nU\nY\n";
    t.commit_file("f.txt", base, "init");
    let repo = t.repo();
    // Editor lines: [B1, B2, U, X]; git lines: ["B1", "B2\rU\rX"].
    let buffer = "B1\nB2\rU\rX\n";
    let hunks = diff_against_index(&repo, p("f.txt"), buffer);
    assert_eq!(hunks.len(), 2, "{hunks:?}");
    let result = stage_hunk(&repo, p("f.txt"), &hunks[1], buffer);
    let staged = t.staged("f.txt");
    match result {
        Ok(()) => assert_eq!(staged, buffer, "partial, corrupted stage"),
        Err(GitError::StaleHunk) => assert_eq!(staged, base),
        Err(other) => panic!("{other:?}"),
    }
}

/// A missing working directory is reported as "git is not installed",
/// because spawn's `NotFound` (from the bad `current_dir`) is mapped to
/// `GitNotFound` unconditionally.
#[test]
fn missing_workdir_is_not_reported_as_missing_git() {
    let Some(t) = TestRepo::new() else { return };
    let repo = Repo {
        workdir: t.dir.join("does-not-exist"),
        git_dir: t.dir.join(".git"),
        common_dir: t.dir.join(".git"),
    };
    let result = status(&repo);
    assert!(!matches!(result, Err(GitError::GitNotFound)), "{result:?}");
}
