//! Gutter hunks, staging/unstaging hunks and files, and commits.

mod common;

use arcode_git::{
    ChangeKind, GitError, StatusCode, commit, diff_against_index, revert_hunk, stage_file,
    stage_hunk, staged_hunks, status, unstage_file, unstage_hunk,
};
use common::{TestRepo, numbered, p};

/// A 20-line committed file and a buffer changing lines 2 and 15.
fn two_changes(t: &TestRepo) -> String {
    t.commit_file("f.txt", &numbered(20), "init");
    let buffer = numbered(20)
        .replace("line 2\n", "line two\n")
        .replace("line 15\n", "line 15\ninserted\n");
    t.write("f.txt", &buffer);
    buffer
}

#[test]
fn stage_single_hunk_then_unstage() {
    let Some(t) = TestRepo::new() else { return };
    let buffer = two_changes(&t);
    let repo = t.repo();

    let hunks = diff_against_index(&repo, p("f.txt"), &buffer);
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].kind, ChangeKind::Modified);
    assert_eq!(hunks[0].old_text, "line 2\n");
    assert_eq!(hunks[1].kind, ChangeKind::Added);
    assert_eq!(hunks[1].new_range(), 15..16);

    stage_hunk(&repo, p("f.txt"), &hunks[0], &buffer).unwrap();
    let cached = t.git(&["diff", "--cached", "-U0"]);
    assert!(cached.contains("-line 2\n+line two\n"), "{cached}");
    assert!(!cached.contains("inserted"), "{cached}");
    assert_eq!(
        t.staged("f.txt"),
        numbered(20).replace("line 2\n", "line two\n")
    );

    // Only the second change is left in the gutter.
    let left = diff_against_index(&repo, p("f.txt"), &buffer);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].kind, ChangeKind::Added);

    // The old hunk is stale now.
    assert!(matches!(
        stage_hunk(&repo, p("f.txt"), &hunks[0], &buffer),
        Err(GitError::StaleHunk)
    ));

    let staged = staged_hunks(&repo, p("f.txt")).unwrap();
    assert_eq!(staged.len(), 1);
    unstage_hunk(&repo, p("f.txt"), &staged[0]).unwrap();
    assert_eq!(t.git(&["diff", "--cached"]), "");
    assert_eq!(diff_against_index(&repo, p("f.txt"), &buffer).len(), 2);
}

#[test]
fn stage_second_hunk_with_unsaved_buffer() {
    let Some(t) = TestRepo::new() else { return };
    let buffer = two_changes(&t);
    // The file on disk is stale; staging takes lines from the buffer.
    t.write("f.txt", &numbered(20));
    let repo = t.repo();
    let hunks = diff_against_index(&repo, p("f.txt"), &buffer);
    stage_hunk(&repo, p("f.txt"), &hunks[1], &buffer).unwrap();
    assert_eq!(
        t.staged("f.txt"),
        numbered(20).replace("line 15\n", "line 15\ninserted\n")
    );
}

#[test]
fn files_without_trailing_newline() {
    let Some(t) = TestRepo::new() else { return };
    let repo = t.repo();

    // Appending after an unterminated last line.
    t.commit_file("a.txt", "one\ntwo", "a");
    let buffer = "one\ntwo\nthree";
    let hunks = diff_against_index(&repo, p("a.txt"), buffer);
    assert_eq!(hunks.len(), 1);
    stage_hunk(&repo, p("a.txt"), &hunks[0], buffer).unwrap();
    assert_eq!(t.staged("a.txt"), "one\ntwo\nthree");

    // Unstaging restores the unterminated original.
    let staged = staged_hunks(&repo, p("a.txt")).unwrap();
    assert_eq!(staged.len(), 1);
    unstage_hunk(&repo, p("a.txt"), &staged[0]).unwrap();
    assert_eq!(t.staged("a.txt"), "one\ntwo");

    // Changing the last line and dropping its newline.
    t.commit_file("b.txt", "x\ny\n", "b");
    let hunks = diff_against_index(&repo, p("b.txt"), "x\nY");
    stage_hunk(&repo, p("b.txt"), &hunks[0], "x\nY").unwrap();
    assert_eq!(t.staged("b.txt"), "x\nY");
}

#[test]
fn deletion_hunk_and_awkward_path() {
    let Some(t) = TestRepo::new() else { return };
    // `*` is not allowed in Windows file names.
    let name = if cfg!(windows) {
        "dir/sp ace [x].txt"
    } else {
        "dir/sp ace [x]*.txt"
    };
    t.commit_file(name, "a\nb\nc\nd\n", "init");
    let repo = t.repo();
    let buffer = "a\nd\n";
    let hunks = diff_against_index(&repo, p(name), buffer);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].kind, ChangeKind::Deleted);
    stage_hunk(&repo, p(name), &hunks[0], buffer).unwrap();
    assert_eq!(t.staged(name), "a\nd\n");
    unstage_file(&repo, p(name)).unwrap();
    assert_eq!(t.staged(name), "a\nb\nc\nd\n");
}

#[test]
fn untracked_file_hunk_stages_buffer_as_new_file() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("base.txt", "base\n", "init");
    t.write("new.txt", "on disk\n");
    let repo = t.repo();
    let buffer = "in buffer\nsecond\n";
    let hunks = diff_against_index(&repo, p("new.txt"), buffer);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].kind, ChangeKind::Added);
    assert_eq!(hunks[0].new_range(), 0..2);
    stage_hunk(&repo, p("new.txt"), &hunks[0], buffer).unwrap();
    assert_eq!(t.staged("new.txt"), buffer);

    // Unstaging the only hunk of a new file removes it from the index.
    let staged = staged_hunks(&repo, p("new.txt")).unwrap();
    unstage_hunk(&repo, p("new.txt"), &staged[0]).unwrap();
    let entries = status(&repo).unwrap();
    let entry = entries.iter().find(|e| e.path == p("new.txt")).unwrap();
    assert_eq!(entry.index, StatusCode::Untracked);
}

#[test]
fn gutter_sources() {
    let Some(t) = TestRepo::new() else { return };
    t.write(".gitignore", "*.log\n");
    t.commit_file("f.txt", "a\nb\n", "init");
    let repo = t.repo();
    // The index wins over HEAD.
    t.write("f.txt", "a\nB\n");
    t.git(&["add", "f.txt"]);
    assert!(diff_against_index(&repo, p("f.txt"), "a\nB\n").is_empty());
    // Ignored and out-of-repo files have no hunks.
    assert!(diff_against_index(&repo, p("x.log"), "x\n").is_empty());
    assert!(diff_against_index(&repo, p("../x.txt"), "x\n").is_empty());
    // Revert restores the index lines in the buffer.
    let buffer = "A\nB\n";
    let hunks = diff_against_index(&repo, p("f.txt"), buffer);
    assert_eq!(revert_hunk(&hunks[0], buffer).apply(buffer), "a\nB\n");
}

#[test]
fn crlf_buffer_is_normalized_when_git_converts() {
    let Some(t) = TestRepo::new() else { return };
    t.commit_file("w.txt", "a\nb\nc\n", "init");
    t.git(&["config", "core.autocrlf", "true"]);
    let repo = t.repo();
    let buffer = "a\r\nB\r\nc\r\n";
    let hunks = diff_against_index(&repo, p("w.txt"), buffer);
    assert_eq!(hunks.len(), 1);
    stage_hunk(&repo, p("w.txt"), &hunks[0], buffer).unwrap();
    assert_eq!(t.staged("w.txt"), "a\nB\nc\n");
}

#[test]
fn stage_and_unstage_files() {
    let Some(t) = TestRepo::new() else { return };
    let repo = t.repo();
    // Unborn repository: unstage uses `git rm --cached`.
    t.write("a.txt", "a\n");
    stage_file(&repo, p("a.txt")).unwrap();
    assert_eq!(status(&repo).unwrap()[0].index, StatusCode::Added);
    t.write("a.txt", "a\nchanged after staging\n");
    unstage_file(&repo, p("a.txt")).unwrap();
    assert_eq!(status(&repo).unwrap()[0].index, StatusCode::Untracked);

    // Born repository: `git restore --staged`, deletions included.
    stage_file(&repo, p("a.txt")).unwrap();
    commit(&repo, "first").unwrap();
    std::fs::remove_file(t.dir.join("a.txt")).unwrap();
    stage_file(&repo, p("a.txt")).unwrap();
    assert_eq!(status(&repo).unwrap()[0].index, StatusCode::Deleted);
    unstage_file(&repo, p("a.txt")).unwrap();
    let entry = &status(&repo).unwrap()[0];
    assert_eq!(
        (entry.index, entry.worktree),
        (StatusCode::Unmodified, StatusCode::Deleted)
    );
    assert!(matches!(
        stage_file(&repo, p("../outside")),
        Err(GitError::InvalidPath(_))
    ));
}

#[test]
fn commit_outcomes() {
    let Some(t) = TestRepo::new() else { return };
    let repo = t.repo();
    assert!(matches!(commit(&repo, "  \n"), Err(GitError::EmptyMessage)));
    assert!(matches!(commit(&repo, "x"), Err(GitError::NothingToCommit)));
    t.write("a.txt", "a\n");
    // Untracked files are not staged.
    assert!(matches!(commit(&repo, "x"), Err(GitError::NothingToCommit)));
    stage_file(&repo, p("a.txt")).unwrap();
    let message = "Subject line\n\nBody with -dashes and \"quotes\"\n";
    let sha = commit(&repo, message).unwrap();
    assert_eq!(sha, t.git(&["rev-parse", "--short", "HEAD"]).trim());
    assert_eq!(
        t.git(&["log", "-1", "--format=%B"]).trim_end(),
        message.trim_end()
    );
    assert_eq!(t.git(&["log", "-1", "--format=%an"]).trim(), "Arcode Test");
}
