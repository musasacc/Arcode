//! Cloning a local repository by its path (no network involved).

mod common;

use arcode_git::{CloneFailure, clone_command, clone_failure};
use common::TestRepo;

#[test]
fn clones_a_local_repository() {
    let Some(source) = TestRepo::new() else {
        return;
    };
    source.commit_file("hello.txt", "hi\n", "first");
    let url = source.dir.display().to_string();
    let dest = tempfile::tempdir().unwrap();
    let target = dest.path().join(arcode_git::folder_name_from_url(&url));
    let out = clone_command(&url, &target, true).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(target.join("hello.txt")).unwrap(),
        "hi\n"
    );
    assert!(target.join(".git").is_dir());
}

#[test]
fn a_missing_repository_is_reported() {
    let Some(_) = TestRepo::new() else { return };
    let dest = tempfile::tempdir().unwrap();
    let missing = dest.path().join("no-such-repo");
    let url = missing.display().to_string();
    let out = clone_command(&url, &dest.path().join("x"), true)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(clone_failure(&stderr), CloneFailure::NotFound, "{stderr}");
    assert!(!dest.path().join("x").exists(), "git removes what it made");
}
