//! Staging edge cases: paths missing from the index, hunks sharing a git
//! line, branch-name validation and a vanished working directory.

mod common;

use arcode_git::{
    GitError, Head, Repo, branches, checkout_branch, diff_against_index, stage_hunk, staged_hunks,
    unstage_hunk,
};
use common::{TestRepo, p};

#[test]
fn staging_after_rm_cached_applies_hunk_to_head_version() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("f.txt", "a\nb\nc\nd\ne\nf\ng\n", "init");
    t.git(&["rm", "-q", "--cached", "f.txt"]);
    let repo = t.repo();
    let buffer = "A\nb\nc\nd\ne\nf\nG\n";
    let hunks = diff_against_index(&repo, p("f.txt"), buffer);
    assert_eq!(hunks.len(), 2);
    stage_hunk(&repo, p("f.txt"), &hunks[1], buffer).unwrap();
    assert_eq!(t.staged("f.txt"), "a\nb\nc\nd\ne\nf\nG\n");
    // Now the index exists again and the remaining hunk stages normally.
    let hunks = diff_against_index(&repo, p("f.txt"), buffer);
    assert_eq!(hunks.len(), 1);
    stage_hunk(&repo, p("f.txt"), &hunks[0], buffer).unwrap();
    assert_eq!(t.staged("f.txt"), buffer);
}

#[test]
fn unstaging_staged_deletion_restores_head_entry() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("f.txt", "a\nb\n", "init");
    t.git(&["rm", "-q", "f.txt"]);
    let repo = t.repo();
    let staged = staged_hunks(&repo, p("f.txt")).unwrap();
    unstage_hunk(&repo, p("f.txt"), &staged[0]).unwrap();
    assert_eq!(t.staged("f.txt"), "a\nb\n");
    assert!(staged_hunks(&repo, p("f.txt")).unwrap().is_empty());
    // The working tree file stays deleted.
    assert!(!t.dir.join("f.txt").exists());
}

#[test]
fn hunks_sharing_a_git_line_are_staged_together() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("f.txt", "A\nU\nY\n", "init");
    let repo = t.repo();
    let buffer = "B1\nB2\rU\rX\n";
    let hunks = diff_against_index(&repo, p("f.txt"), buffer);
    stage_hunk(&repo, p("f.txt"), &hunks[1], buffer).unwrap();
    assert_eq!(t.staged("f.txt"), buffer);
    assert!(diff_against_index(&repo, p("f.txt"), buffer).is_empty());
    // And back: unstaging either hunk unstages both, restoring HEAD exactly.
    let staged = staged_hunks(&repo, p("f.txt")).unwrap();
    assert_eq!(staged.len(), 2);
    unstage_hunk(&repo, p("f.txt"), &staged[1]).unwrap();
    assert_eq!(t.staged("f.txt"), "A\nU\nY\n");
    assert!(staged_hunks(&repo, p("f.txt")).unwrap().is_empty());
    assert!(matches!(
        unstage_hunk(&repo, p("f.txt"), &staged[0]),
        Err(GitError::StaleHunk)
    ));
}

#[test]
fn checkout_branch_switches_to_real_branches_only() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("a.txt", "1\n", "one");
    t.git(&["branch", "feat/x"]);
    t.git(&["tag", "v1"]);
    let repo = t.repo();
    for name in [
        "v1",
        "HEAD",
        "@{-1}",
        "feat",
        "",
        "-f",
        "refs/heads/main",
        "main..x",
    ] {
        let result = checkout_branch(&repo, name);
        assert!(
            matches!(result, Err(GitError::BranchNotFound(_))),
            "{name:?}: {result:?}"
        );
    }
    checkout_branch(&repo, "feat/x").unwrap();
    assert_eq!(repo.head(), Head::Branch("feat/x".into()));
    let current: Vec<_> = branches(&repo)
        .unwrap()
        .into_iter()
        .filter(|b| b.is_current)
        .map(|b| b.name)
        .collect();
    assert_eq!(current, ["feat/x"]);
    checkout_branch(&repo, "main").unwrap();
    assert_eq!(repo.head(), Head::Branch("main".into()));
}

#[test]
fn missing_workdir_has_its_own_error() {
    let Some(t) = TestRepo::new() else { return };
    let repo = Repo {
        workdir: t.dir.join("gone"),
        git_dir: t.dir.join(".git"),
        common_dir: t.dir.join(".git"),
    };
    let err = arcode_git::status(&repo).unwrap_err();
    assert!(matches!(err, GitError::WorkdirMissing(ref d) if d.ends_with("gone")));
    assert!(err.to_string().contains("no longer exists"), "{err}");
}
