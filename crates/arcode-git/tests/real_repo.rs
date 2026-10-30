//! End-to-end checks against repositories created by the real `git` CLI.
//! Each test is skipped (passes trivially) when git is not installed.

use std::fs;
use std::path::Path;
use std::process::Command;

use arcode_git::{ChangeKind, Head, find_repo, line_diff};

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Runs git in `dir` isolated from the user's global/system config.
fn git(dir: &Path, args: &[&str]) -> String {
    let global = dir.join(".test-gitconfig");
    if !global.exists() {
        fs::write(&global, "").unwrap();
    }
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &global)
        .args([
            "-c",
            "user.name=Arcode Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Creates a repo in `dir` with one commit containing `src/a.txt`.
fn init_with_commit(dir: &Path) {
    git(dir, &["init", "-q"]);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/a.txt"), "one\ntwo\nthree\n").unwrap();
    git(dir, &["add", "src/a.txt"]);
    git(dir, &["commit", "-q", "-m", "first"]);
}

#[test]
fn empty_repository_is_unborn() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q"]);
    let repo = find_repo(tmp.path()).unwrap();
    assert_eq!(repo.head(), Head::Unborn("main".into()));
    fs::write(tmp.path().join("x.txt"), "x").unwrap();
    assert_eq!(repo.head_file_contents(Path::new("x.txt")), None);
}

#[test]
fn committed_file_contents_and_branch() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    init_with_commit(tmp.path());
    let repo = find_repo(&tmp.path().join("src").join("a.txt")).unwrap();
    assert_eq!(repo.workdir, tmp.path());
    assert_eq!(repo.head(), Head::Branch("main".into()));
    assert_eq!(repo.branch_display(), "main");

    let rel = Path::new("src").join("a.txt");
    let committed = repo.head_file_contents(&rel).unwrap();
    assert_eq!(committed, "one\ntwo\nthree\n");
    // Absolute paths inside the workdir work too.
    let abs = tmp.path().join(&rel);
    assert_eq!(
        repo.head_file_contents(&abs).as_deref(),
        Some(committed.as_str())
    );

    // Untracked files, directories and escaping paths yield None.
    fs::write(tmp.path().join("new.txt"), "new").unwrap();
    assert_eq!(repo.head_file_contents(Path::new("new.txt")), None);
    assert_eq!(repo.head_file_contents(Path::new("src")), None);
    assert_eq!(repo.head_file_contents(Path::new("../a.txt")), None);

    // The gutter flow: edit the working copy, diff against HEAD.
    fs::write(&abs, "one\nTWO\nthree\nfour\n").unwrap();
    let current = fs::read_to_string(&abs).unwrap();
    let kinds: Vec<_> = line_diff(&committed, &current)
        .into_iter()
        .map(|c| (c.kind, c.new_lines))
        .collect();
    assert_eq!(
        kinds,
        vec![(ChangeKind::Modified, 1..2), (ChangeKind::Added, 3..4)]
    );
}

#[test]
fn unicode_path_is_not_quoted() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    init_with_commit(tmp.path());
    let name = "größe ü.txt";
    fs::write(tmp.path().join(name), "ä\n").unwrap();
    git(tmp.path(), &["add", "--", name]);
    git(tmp.path(), &["commit", "-q", "-m", "unicode"]);
    let repo = find_repo(tmp.path()).unwrap();
    assert_eq!(
        repo.head_file_contents(Path::new(name)).as_deref(),
        Some("ä\n")
    );
}

#[test]
fn detached_and_packed_refs() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    init_with_commit(tmp.path());
    let full = git(tmp.path(), &["rev-parse", "HEAD"]);

    git(tmp.path(), &["pack-refs", "--all"]);
    let repo = find_repo(tmp.path()).unwrap();
    assert!(!tmp.path().join(".git/refs/heads/main").exists());
    assert_eq!(repo.head(), Head::Branch("main".into()));

    git(tmp.path(), &["checkout", "-q", "--detach"]);
    assert_eq!(repo.head(), Head::Detached(full[..7].to_owned()));
    assert_eq!(repo.branch_display(), format!("detached@{}", &full[..7]));
}

#[test]
fn linked_worktree() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    fs::create_dir_all(&main).unwrap();
    init_with_commit(&main);
    git(&main, &["worktree", "add", "-q", "-b", "feat", "../wt"]);

    let wt = tmp.path().join("wt");
    let repo = find_repo(&wt.join("src")).unwrap();
    assert_eq!(repo.workdir, wt);
    assert!(repo.is_linked());
    assert_eq!(
        fs::canonicalize(&repo.common_dir).unwrap(),
        fs::canonicalize(main.join(".git")).unwrap()
    );
    assert_eq!(repo.head(), Head::Branch("feat".into()));
    assert_eq!(
        repo.head_file_contents(Path::new("src/a.txt")).as_deref(),
        Some("one\ntwo\nthree\n")
    );
}
