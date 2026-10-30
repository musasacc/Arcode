//! `blame`, `branches` and `checkout_branch` against real repositories.

mod common;

use arcode_git::{GitError, Head, blame, branches, checkout_branch};
use common::{TestRepo, p};

/// Two commits by different authors: lines 0-1 by Ann, line 2 by Bob.
fn two_authors(t: &TestRepo) -> (String, String) {
    t.write("f.txt", "one\ntwo\n");
    t.git(&["add", "f.txt"]);
    let ann = [
        ("GIT_AUTHOR_NAME", "Ann"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
    ];
    assert!(
        t.git_raw(&["commit", "-q", "-m", "Ann's commit"], &ann)
            .status
            .success()
    );
    let first = t.git(&["rev-parse", "--short=7", "HEAD"]).trim().to_owned();
    t.write("f.txt", "one\ntwo\nthree\n");
    t.git(&["add", "f.txt"]);
    let bob = [
        ("GIT_AUTHOR_NAME", "Bob"),
        ("GIT_AUTHOR_DATE", "1700000500 +0000"),
    ];
    assert!(
        t.git_raw(&["commit", "-q", "-m", "Bob adds three\n\nbody"], &bob)
            .status
            .success()
    );
    let second = t.git(&["rev-parse", "--short=7", "HEAD"]).trim().to_owned();
    (first, second)
}

#[test]
fn blame_committed_file() {
    let Some(t) = TestRepo::new() else { return };
    let (first, second) = two_authors(&t);
    let lines = blame(&t.repo(), p("f.txt"), None).unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines.iter().map(|l| l.line).collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(lines[0].commit, first);
    assert_eq!(lines[0].author, "Ann");
    assert_eq!(lines[0].author_time, 1_700_000_000);
    assert_eq!(lines[0].summary, "Ann's commit");
    assert_eq!(lines[2].commit, second);
    assert_eq!(lines[2].author, "Bob");
    assert_eq!(lines[2].summary, "Bob adds three");
    assert!(lines.iter().all(|l| !l.is_uncommitted));
}

#[test]
fn blame_dirty_buffer_and_disk() {
    let Some(t) = TestRepo::new() else { return };
    let (first, second) = two_authors(&t);
    let repo = t.repo();
    // Buffer with an inserted first line: numbers follow the buffer.
    let lines = blame(&repo, p("f.txt"), Some("new\none\ntwo\nthree\n")).unwrap();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].is_uncommitted);
    assert_eq!(lines[0].commit, "0000000");
    assert_eq!(
        (lines[1].line, lines[1].commit.as_str()),
        (1, first.as_str())
    );
    assert_eq!(lines[3].commit, second);

    // Without contents the modified file on disk is blamed.
    t.write("f.txt", "one\nTWO\nthree\n");
    let lines = blame(&repo, p("f.txt"), None).unwrap();
    assert!(lines[1].is_uncommitted);
    assert!(!lines[0].is_uncommitted && !lines[2].is_uncommitted);
}

#[test]
fn blame_new_files_are_uncommitted() {
    let Some(t) = TestRepo::new() else { return };
    let repo = t.repo();
    // No commits at all.
    t.write("n.txt", "a\nb");
    let lines = blame(&repo, p("n.txt"), None).unwrap();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| l.is_uncommitted));
    let lines = blame(&repo, p("n.txt"), Some("x\n")).unwrap();
    assert_eq!(lines.len(), 1);
    assert!(matches!(
        blame(&repo, p("missing.txt"), None),
        Err(GitError::Io(_))
    ));
}

#[test]
fn list_and_checkout_branches() {
    let Some(t) = TestRepo::new() else { return };
    let repo = t.repo();
    assert!(
        branches(&repo).unwrap().is_empty(),
        "unborn branch has no ref"
    );
    t.commit_file("a.txt", "a\n", "first");
    t.git(&["branch", "feature/x"]);
    // A remote that was never reachable: nothing may contact it.
    t.git(&[
        "config",
        "remote.origin.url",
        "https://invalid.example/x.git",
    ]);
    let refspec = "+refs/heads/*:refs/remotes/origin/*";
    t.git(&["config", "remote.origin.fetch", refspec]);
    t.git(&["update-ref", "refs/remotes/origin/remote-only", "HEAD"]);
    t.git(&[
        "branch",
        "--set-upstream-to=origin/remote-only",
        "feature/x",
    ]);

    let list = branches(&repo).unwrap();
    let names: Vec<_> = list.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["feature/x", "main"]);
    assert!(list[1].is_current && !list[0].is_current);
    assert_eq!(list[1].summary, "first");
    assert_eq!(list[0].upstream.as_deref(), Some("origin/remote-only"));
    assert_eq!((list[0].ahead, list[0].behind), (0, 0));
    assert_eq!(
        list[1].commit,
        t.git(&["rev-parse", "--short", "HEAD"]).trim()
    );

    checkout_branch(&repo, "feature/x").unwrap();
    assert_eq!(repo.head(), Head::Branch("feature/x".into()));

    // Only local branches: no DWIM from remote-tracking refs, no options.
    for name in ["remote-only", "nope", "-f", ""] {
        assert!(
            matches!(
                checkout_branch(&repo, name),
                Err(GitError::BranchNotFound(_))
            ),
            "{name}"
        );
    }

    // Local changes that would be overwritten make git refuse.
    t.commit_file("a.txt", "on feature\n", "feature change");
    t.write("a.txt", "dirty\n");
    assert!(matches!(
        checkout_branch(&repo, "main"),
        Err(GitError::Command { .. })
    ));
    assert_eq!(repo.head(), Head::Branch("feature/x".into()));
}

#[test]
fn checkout_never_lazy_fetches_in_partial_clone() {
    let Some(origin) = TestRepo::new() else {
        return;
    };
    origin.git(&["config", "uploadpack.allowFilter", "true"]);
    origin.commit_file("a.txt", "main\n", "main");
    origin.git(&["checkout", "-q", "-b", "other"]);
    origin.commit_file("a.txt", "only on the remote\n", "other");
    origin.git(&["checkout", "-q", "main"]);

    let parent = origin.dir.parent().unwrap();
    let dir = origin.dir.display().to_string().replace('\\', "/");
    let url = if dir.starts_with('/') {
        format!("file://{dir}")
    } else {
        format!("file:///{dir}")
    };
    let out = origin.git_raw(
        &["clone", "-q", "--filter=blob:none", &url, "../clone"],
        &[],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let clone = arcode_git::find_repo(&parent.join("clone")).unwrap();
    // Creating the local branch is purely local.
    let out = std::process::Command::new("git")
        .current_dir(&clone.workdir)
        .args(["branch", "-q", "other", "origin/other"])
        .output()
        .unwrap();
    assert!(out.status.success());

    let err = checkout_branch(&clone, "other").unwrap_err();
    assert!(matches!(err, GitError::NetworkRequired(_)), "{err:?}");
    assert_eq!(clone.head(), Head::Branch("main".into()));
}

#[test]
fn ahead_and_behind_are_computed_locally() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("a.txt", "1\n", "one");
    t.git(&[
        "config",
        "remote.origin.url",
        "https://invalid.example/x.git",
    ]);
    t.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);
    t.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    t.git(&["branch", "-q", "--set-upstream-to=origin/main"]);
    t.commit_file("a.txt", "2\n", "two");
    t.commit_file("a.txt", "3\n", "three");
    let list = branches(&t.repo()).unwrap();
    assert_eq!((list[0].ahead, list[0].behind), (2, 0));
    assert!(!list[0].upstream_gone);
    t.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    assert!(branches(&t.repo()).unwrap()[0].upstream_gone);
}
