//! Source control view operations: init, branches, discard, amend.

mod common;

use arcode_git::{
    CommitOptions, GitError, Head, branches, commit_with, create_branch, discard_changes,
    init_repo, last_commit_message, stage_all, stage_file, stage_paths, status, unstage_all,
};
use common::{TestRepo, p};

#[test]
fn init_creates_a_repository_on_request() {
    let Some(_) = TestRepo::new() else { return };
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path()).unwrap();
    assert!(matches!(repo.head(), Head::Unborn(_)));
    assert!(dir.path().join(".git").is_dir());
    let missing = dir.path().join("missing");
    assert!(matches!(
        init_repo(&missing),
        Err(GitError::WorkdirMissing(_))
    ));
}

#[test]
fn create_branch_switches_to_it() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("a.txt", "a\n", "first");
    let repo = t.repo();
    create_branch(&repo, "feature/x").unwrap();
    assert_eq!(repo.head(), Head::Branch("feature/x".into()));
    let names: Vec<_> = branches(&repo)
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(names, ["feature/x", "main"]);
    for bad in ["", "-x", "a..b", "with space"] {
        assert!(
            matches!(
                create_branch(&repo, bad),
                Err(GitError::InvalidBranchName(_))
            ),
            "{bad:?}"
        );
    }
    assert!(create_branch(&repo, "main").is_err(), "existing branch");
}

#[test]
fn discard_restores_tracked_and_deletes_untracked() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("a.txt", "one\n", "first");
    let repo = t.repo();
    t.write("a.txt", "staged\n");
    stage_file(&repo, p("a.txt")).unwrap();
    t.write("a.txt", "worktree\n");
    t.write("new.txt", "new\n");
    discard_changes(&repo, p("a.txt")).unwrap();
    assert_eq!(t.read("a.txt"), "staged\n", "staged changes stay");
    discard_changes(&repo, p("new.txt")).unwrap();
    assert!(!t.dir.join("new.txt").exists());
    std::fs::remove_file(t.dir.join("a.txt")).unwrap();
    discard_changes(&repo, p("a.txt")).unwrap();
    assert_eq!(t.read("a.txt"), "staged\n", "a deleted file comes back");
}

#[test]
fn amend_replaces_the_last_commit() {
    let Some(t) = TestRepo::new() else { return };
    let repo = t.repo();
    assert_eq!(last_commit_message(&repo).unwrap(), None);
    assert!(matches!(
        commit_with(&repo, "x", CommitOptions { amend: true }),
        Err(GitError::NothingToAmend)
    ));
    t.commit_file("a.txt", "a\n", "first\n\nbody");
    assert_eq!(
        last_commit_message(&repo).unwrap().as_deref(),
        Some("first\n\nbody")
    );
    t.write("b.txt", "b\n");
    stage_file(&repo, p("b.txt")).unwrap();
    commit_with(&repo, "first, amended", CommitOptions { amend: true }).unwrap();
    assert_eq!(t.git(&["rev-list", "--count", "HEAD"]).trim(), "1");
    assert_eq!(
        last_commit_message(&repo).unwrap().as_deref(),
        Some("first, amended")
    );
    assert!(status(&repo).unwrap().is_empty());
    // Message-only amend with nothing staged.
    commit_with(&repo, "renamed", CommitOptions { amend: true }).unwrap();
    assert_eq!(
        last_commit_message(&repo).unwrap().as_deref(),
        Some("renamed")
    );
}

#[test]
fn stage_and_unstage_everything() {
    let Some(t) = TestRepo::new() else { return };
    let repo = t.repo();
    t.write("a.txt", "a\n");
    unstage_all(&repo).unwrap();
    stage_all(&repo, true).unwrap();
    assert!(
        status(&repo).unwrap().iter().all(|e| !e.is_staged()),
        "untracked stay"
    );
    stage_all(&repo, false).unwrap();
    assert!(status(&repo).unwrap().iter().all(|e| e.is_staged()));
    unstage_all(&repo).unwrap();
    assert!(
        status(&repo).unwrap().iter().all(|e| !e.is_staged()),
        "unborn"
    );
    t.commit_file("a.txt", "a\n", "first");
    t.write("a.txt", "b\n");
    stage_all(&repo, true).unwrap();
    assert!(status(&repo).unwrap()[0].is_staged());
    unstage_all(&repo).unwrap();
    assert!(!status(&repo).unwrap()[0].is_staged());
}

#[test]
fn stage_paths_stages_exactly_the_given_paths() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("a.txt", "a\n", "first");
    t.commit_file("gone.txt", "g\n", "second");
    let repo = t.repo();
    t.write("a.txt", "changed\n");
    t.write("new one.txt", "n\n");
    // Brackets are glob magic: without literal pathspecs "[a].txt" would
    // also stage a.txt.
    t.write("[a].txt", "literal\n");
    std::fs::remove_file(t.dir.join("gone.txt")).unwrap();
    stage_paths(&repo, &[]).unwrap();
    let paths = ["[a].txt", "new one.txt", "gone.txt"].map(|s| p(s).to_path_buf());
    stage_paths(&repo, &paths).unwrap();
    let mut staged: Vec<_> = status(&repo)
        .unwrap()
        .into_iter()
        .filter(|e| e.is_staged())
        .map(|e| e.path)
        .collect();
    staged.sort();
    assert_eq!(
        staged,
        ["[a].txt", "gone.txt", "new one.txt"].map(|s| p(s).to_path_buf())
    );
}
