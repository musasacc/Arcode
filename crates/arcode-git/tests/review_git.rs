//! Reviewer tests: each one demonstrates a defect found during review.

use std::fs;
use std::path::Path;
use std::process::Command;

use arcode_git::{ChangeKind, find_repo, line_diff};

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn git(dir: &Path, args: &[&str]) {
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
            "user.name=Review",
            "-c",
            "user.email=review@example.invalid",
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
}

fn pack_count(git_dir: &Path) -> usize {
    fs::read_dir(git_dir.join("objects").join("pack"))
        .map(|d| d.count())
        .unwrap_or(0)
}

/// In a partial clone (`--filter=blob:none`), `head_file_contents` runs
/// `git cat-file blob HEAD:<path>` which lazily fetches the missing blob from
/// the promisor remote. That is a network operation (here via a `file://`
/// remote, in real life https/ssh, possibly with credential helpers),
/// violating "NEVER performs network operations".
#[test]
fn head_file_contents_does_not_lazy_fetch_in_partial_clone() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q"]);
    git(&origin, &["config", "uploadpack.allowFilter", "true"]);
    fs::write(origin.join("a.txt"), "secret blob\n").unwrap();
    git(&origin, &["add", "a.txt"]);
    git(&origin, &["commit", "-q", "-m", "c"]);

    let url = format!("file://{}", origin.display());
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            "--no-checkout",
            "--filter=blob:none",
            &url,
            "clone",
        ],
    );
    let clone = tmp.path().join("clone");
    let repo = find_repo(&clone).unwrap();
    let packs_before = pack_count(&repo.git_dir);

    let contents = repo.head_file_contents(Path::new("a.txt"));

    assert_eq!(
        pack_count(&repo.git_dir),
        packs_before,
        "head_file_contents fetched objects from the remote"
    );
    assert_eq!(contents, None, "blob was only obtainable via a fetch");
}

/// A relative `gitdir:` must be resolved like git does (the OS follows
/// symlinks before `..`), not lexically. Opening a worktree through a
/// symlinked path makes `find_repo` resolve `..` against the symlink's
/// parent, find nothing there and return `None`, while git works fine.
#[cfg(unix)]
#[test]
fn relative_gitdir_through_symlinked_checkout() {
    let tmp = tempfile::tempdir().unwrap();
    let main_git = tmp.path().join("main").join(".git");
    fs::create_dir_all(main_git.join("refs").join("heads")).unwrap();
    fs::write(main_git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(
        main_git.join("refs/heads/feat"),
        "0123456789abcdef0123456789abcdef01234567\n",
    )
    .unwrap();
    let wt_git = main_git.join("worktrees").join("wt");
    fs::create_dir_all(&wt_git).unwrap();
    fs::write(wt_git.join("HEAD"), "ref: refs/heads/feat\n").unwrap();
    fs::write(wt_git.join("commondir"), "../..\n").unwrap();

    // Real checkout at tmp/real/wt; gitdir is relative to that location.
    let real_wt = tmp.path().join("real").join("wt");
    fs::create_dir_all(&real_wt).unwrap();
    fs::write(
        real_wt.join(".git"),
        "gitdir: ../../main/.git/worktrees/wt\n",
    )
    .unwrap();
    assert!(find_repo(&real_wt).is_some(), "sanity: real path works");

    // The user opens the same checkout through a symlink.
    // (At a different depth, so lexical `..` folding lands elsewhere.)
    let link = tmp.path().join("wt-link");
    std::os::unix::fs::symlink(&real_wt, &link).unwrap();
    // The OS (and git) resolve this path fine.
    assert!(link.join("../../main/.git/worktrees/wt/HEAD").is_file());

    let repo = find_repo(&link).expect("worktree reached via symlink not found");
    assert_eq!(repo.branch_display(), "feat");
}

/// Same defect with a real submodule (git always writes a relative
/// `gitdir: ../.git/modules/<name>`): `git -C <symlink> rev-parse` works,
/// `find_repo` returns `None`.
#[cfg(unix)]
#[test]
fn submodule_opened_through_symlink() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir_all(&lib).unwrap();
    git(&lib, &["init", "-q"]);
    fs::write(lib.join("x"), "x\n").unwrap();
    git(&lib, &["add", "x"]);
    git(&lib, &["commit", "-q", "-m", "x"]);

    let sup = tmp.path().join("deep").join("super");
    fs::create_dir_all(&sup).unwrap();
    git(&sup, &["init", "-q"]);
    git(
        &sup,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            "../../lib",
            "lib",
        ],
    );
    assert!(find_repo(&sup.join("lib")).is_some(), "sanity");

    let link = tmp.path().join("liblink");
    std::os::unix::fs::symlink(sup.join("lib"), &link).unwrap();
    let repo = find_repo(&link).expect("submodule reached via symlink not found");
    assert_eq!(repo.branch_display(), "main");
}

/// The buffer normalizes lone `\r` to `\n` (classic Mac line endings), but
/// `line_diff` only splits on `\n`, so diffing the committed text against an
/// unchanged buffer marks the whole file as modified.
#[test]
fn lone_cr_line_endings_are_line_breaks() {
    let committed = "a\rb\rc\r";
    let buffer = "a\nb\nc\n";
    assert_eq!(line_diff(committed, buffer), vec![]);
}

/// Line indices must match the editor's line model (ropey with default
/// `unicode_lines`, which also breaks on U+2028/U+2029/U+0085/VT/FF). With
/// `str::lines` a U+2028 inside a JS string shifts every later gutter mark.
#[test]
fn unicode_line_separator_shifts_gutter_lines() {
    let old = "let s = 'x\u{2028}y';\nfoo();\n";
    let new = "let s = 'x\u{2028}y';\nbar();\n";
    let changes = line_diff(old, new);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::Modified);
    // In the editor buffer `foo();` is line 2 (0-based).
    assert_eq!(changes[0].new_lines, 2..3);
}
