//! Review tests for the Source Control view's group actions.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use arcode_config::{Config, ThemeRegistry};
use gpui::{AppContext, Entity, TestAppContext};

use super::{GitPanel, Group};
use crate::git::store::GitStore;

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// A repository with a committed `tracked.txt`, an unstaged change to it,
/// an untracked `new.txt`, and (when `conflict`) an unresolved merge
/// conflict in `c.txt`.
fn setup(conflict: bool) -> Option<(tempfile::TempDir, PathBuf)> {
    Command::new("git").arg("--version").output().ok()?;
    let dir = tempfile::tempdir().ok()?;
    let root = fs::canonicalize(dir.path()).ok()?.join("repo");
    fs::create_dir(&root).ok()?;
    git(&root, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "T"),
        ("user.email", "t@example.invalid"),
        ("commit.gpgsign", "false"),
    ] {
        git(&root, &["config", k, v]);
    }
    fs::write(root.join("tracked.txt"), "one\n").unwrap();
    fs::write(root.join("c.txt"), "base\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    if conflict {
        git(&root, &["switch", "-q", "-c", "other"]);
        fs::write(root.join("c.txt"), "theirs\n").unwrap();
        git(&root, &["commit", "-q", "-am", "theirs"]);
        git(&root, &["switch", "-q", "main"]);
        fs::write(root.join("c.txt"), "ours\n").unwrap();
        git(&root, &["commit", "-q", "-am", "ours"]);
        // Fails with a conflict; that is the point.
        Command::new("git")
            .current_dir(&root)
            .args(["merge", "-q", "other"])
            .output()
            .unwrap();
        assert!(
            !git(&root, &["ls-files", "-u"]).is_empty(),
            "conflict set up"
        );
    }
    fs::write(root.join("tracked.txt"), "one changed\n").unwrap();
    fs::write(root.join("new.txt"), "new\n").unwrap();
    Some((dir, root))
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(Config::default(), cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
    });
}

fn panel_for(root: &Path, cx: &mut TestAppContext) -> Entity<GitPanel> {
    init(cx);
    let store = cx.new(GitStore::new);
    store.update(cx, |s, cx| s.set_project_root(Some(root.to_path_buf()), cx));
    settle(cx);
    cx.new(|cx| GitPanel::new(store, cx))
}

fn settle(cx: &mut TestAppContext) {
    for _ in 0..3 {
        cx.executor().advance_clock(Duration::from_millis(700));
        cx.run_until_parked();
    }
}

/// "Stage all" on the UNTRACKED header runs `git add -A`, which also
/// stages every tracked modification (the Changes group). Expected: only
/// the untracked files are staged.
#[gpui::test]
fn review_stage_all_untracked_stages_only_untracked_files(cx: &mut TestAppContext) {
    let Some((_dir, root)) = setup(false) else {
        return;
    };
    let panel = panel_for(&root, cx);
    panel.update(cx, |p, cx| p.stage_group(Group::Untracked, cx));
    settle(cx);
    let staged = git(&root, &["diff", "--cached", "--name-only"]);
    assert_eq!(
        staged.trim(),
        "new.txt",
        "only the untracked file should be staged"
    );
}

/// "Stage all" on the CHANGES header runs `git add -u`, which also stages
/// unresolved merge conflicts (marking them resolved with the conflict
/// markers still in the file). Expected: files in the Merge Conflicts
/// group stay unmerged; they are only marked resolved one by one.
#[gpui::test]
fn review_stage_all_changes_does_not_resolve_conflicts(cx: &mut TestAppContext) {
    let Some((_dir, root)) = setup(true) else {
        return;
    };
    let panel = panel_for(&root, cx);
    panel.update(cx, |p, cx| p.stage_group(Group::Changes, cx));
    settle(cx);
    assert!(
        !git(&root, &["ls-files", "-u"]).is_empty(),
        "c.txt (still full of conflict markers) was marked resolved by Stage all"
    );
    assert!(
        fs::read_to_string(root.join("c.txt"))
            .unwrap()
            .contains("<<<<<<<")
    );
}

/// A second commit request while the first runs (double Ctrl+Enter) is
/// ignored: one commit, and the message box is cleared afterwards.
#[gpui::test]
fn double_commit_request_commits_once(cx: &mut TestAppContext) {
    let Some((_dir, root)) = setup(false) else {
        return;
    };
    git(&root, &["add", "tracked.txt"]);
    let panel = panel_for(&root, cx);
    panel.update(cx, |p, cx| {
        p.message().update(cx, |m, cx| m.set_text("one commit", cx));
        p.commit_staged(cx);
        p.commit_staged(cx);
    });
    settle(cx);
    let log = git(&root, &["log", "--format=%s"]);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["one commit", "init"]);
    let text = panel.update(cx, |p, cx| p.message().read(cx).text().to_string());
    assert_eq!(text, "");
    // The guard is released: the next commit works.
    panel.update(cx, |p, cx| {
        p.message()
            .update(cx, |m, cx| m.set_text("nothing staged", cx));
        p.commit_staged(cx);
    });
    settle(cx);
    let text = panel.update(cx, |p, cx| p.message().read(cx).text().to_string());
    assert_eq!(text, "nothing staged", "a failed commit keeps the message");
    assert!(!panel.read_with(cx, |p, _| p.committing));
}
