//! `status` and merge conflicts against real repositories.

mod common;

use std::path::PathBuf;

use arcode_git::{
    ConflictKind, GitError, Resolution, StatusCode, commit, conflicted_files, parse_conflicts,
    resolve, stage_file, status, status_summary, status_with_ignored,
};
use common::TestRepo;

fn find<'a>(entries: &'a [arcode_git::FileStatus], path: &str) -> &'a arcode_git::FileStatus {
    let path: PathBuf = path.split('/').collect();
    entries
        .iter()
        .find(|e| e.path == path)
        .unwrap_or_else(|| panic!("{path:?} missing in {entries:#?}"))
}

#[test]
fn status_codes() {
    let Some(t) = TestRepo::new() else { return };
    t.write("keep.txt", "keep\n");
    t.write("modify.txt", "a\n");
    t.write("delete.txt", "d\n");
    t.write("old name.txt", "rename me, long enough to be detected\n");
    t.write(".gitignore", "*.log\n");
    t.git(&["add", "."]);
    t.git(&["commit", "-q", "-m", "init"]);

    t.write("modify.txt", "a\nb\n");
    std::fs::remove_file(t.dir.join("delete.txt")).unwrap();
    t.git(&["mv", "old name.txt", "new name.txt"]);
    t.write("staged.txt", "s\n");
    t.git(&["add", "staged.txt"]);
    t.write("staged.txt", "s\nmore\n");
    t.write("dir/sub/untracked.txt", "u\n");
    t.write("debug.log", "ignored\n");

    let repo = t.repo();
    let entries = status(&repo).unwrap();
    let modify = find(&entries, "modify.txt");
    assert_eq!(
        (modify.index, modify.worktree),
        (StatusCode::Unmodified, StatusCode::Modified)
    );
    let delete = find(&entries, "delete.txt");
    assert_eq!(delete.worktree, StatusCode::Deleted);
    let renamed = find(&entries, "new name.txt");
    assert_eq!(renamed.index, StatusCode::Renamed);
    assert_eq!(renamed.orig_path, Some(PathBuf::from("old name.txt")));
    let staged = find(&entries, "staged.txt");
    assert_eq!(
        (staged.index, staged.worktree),
        (StatusCode::Added, StatusCode::Modified)
    );
    // Untracked directories are expanded to files.
    assert_eq!(
        find(&entries, "dir/sub/untracked.txt").index,
        StatusCode::Untracked
    );
    assert!(entries.iter().all(|e| e.index != StatusCode::Ignored));
    assert!(
        !entries
            .iter()
            .any(|e| e.path == std::path::Path::new("keep.txt"))
    );

    let with_ignored = status_with_ignored(&repo).unwrap();
    assert_eq!(find(&with_ignored, "debug.log").index, StatusCode::Ignored);

    let sum = status_summary(&repo).unwrap();
    assert_eq!((sum.staged, sum.unstaged, sum.untracked), (2, 3, 1));
    assert_eq!(sum.conflicted, 0);
}

#[test]
fn clean_and_unborn_repositories() {
    let Some(t) = TestRepo::new() else { return };
    let repo = t.repo();
    assert!(status(&repo).unwrap().is_empty());
    t.write("a.txt", "a\n");
    let entries = status(&repo).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].index, StatusCode::Untracked);
}

/// Creates a both-modified conflict in `c.txt` on `main`.
fn make_conflict(t: &TestRepo) {
    t.commit_file("c.txt", "top\nmiddle\nbottom\n", "base");
    t.git(&["checkout", "-q", "-b", "feature"]);
    t.commit_file("c.txt", "top\nfeature side\nbottom\n", "feature");
    t.git(&["checkout", "-q", "main"]);
    t.commit_file("c.txt", "top\nmain side\nbottom\n", "main");
    let out = t.git_raw(&["merge", "-q", "feature"], &[]);
    assert!(!out.status.success(), "merge should conflict");
}

#[test]
fn real_merge_conflict() {
    let Some(t) = TestRepo::new() else { return };
    make_conflict(&t);
    let repo = t.repo();

    let entries = status(&repo).unwrap();
    let c = find(&entries, "c.txt");
    assert_eq!(c.conflict, Some(ConflictKind::BothModified));
    assert_eq!(c.index, StatusCode::Unmerged);
    assert_eq!(
        conflicted_files(&repo).unwrap(),
        vec![PathBuf::from("c.txt")]
    );
    assert_eq!(status_summary(&repo).unwrap().conflicted, 1);
    assert!(matches!(
        commit(&repo, "merge"),
        Err(GitError::UnmergedPaths(paths)) if paths == [PathBuf::from("c.txt")]
    ));

    let text = t.read("c.txt");
    let conflicts = parse_conflicts(&text);
    assert_eq!(conflicts.len(), 1);
    let conflict = &conflicts[0];
    assert_eq!(conflict.ours_label, "HEAD");
    assert_eq!(conflict.theirs_label, "feature");
    assert_eq!(conflict.base, None);
    let resolved = resolve(&text, conflict, Resolution::Theirs);
    assert_eq!(resolved, "top\nfeature side\nbottom\n");

    // Resolve, stage and conclude the merge.
    t.write("c.txt", &resolved);
    stage_file(&repo, std::path::Path::new("c.txt")).unwrap();
    assert!(conflicted_files(&repo).unwrap().is_empty());
    let sha = commit(&repo, "Merge feature").unwrap();
    assert_eq!(sha, t.git(&["rev-parse", "--short", "HEAD"]).trim());
    let parents = t.git(&["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(parents.split_whitespace().count(), 3, "merge commit");
}

#[test]
fn diff3_conflict_from_git() {
    let Some(t) = TestRepo::new() else { return };
    t.git(&["config", "merge.conflictStyle", "diff3"]);
    make_conflict(&t);
    let text = t.read("c.txt");
    let conflicts = parse_conflicts(&text);
    assert_eq!(conflicts.len(), 1, "{text}");
    let conflict = &conflicts[0];
    assert!(conflict.base.is_some());
    assert_eq!(
        resolve(&text, conflict, Resolution::Base),
        "top\nmiddle\nbottom\n"
    );
    assert_eq!(
        resolve(&text, conflict, Resolution::Both),
        "top\nmain side\nfeature side\nbottom\n"
    );
}
