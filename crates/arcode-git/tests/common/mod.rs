//! Shared helpers: temporary repositories driven by the real `git` CLI,
//! isolated from the user's global and system configuration.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use arcode_git::{Repo, find_repo};

pub struct TestRepo {
    _tmp: tempfile::TempDir,
    pub dir: PathBuf,
    global: PathBuf,
}

impl TestRepo {
    /// A fresh repository on branch `main`, or `None` when git is missing.
    /// Identity and signing are configured in the repository itself, so the
    /// library under test (which reads normal config) can commit.
    pub fn new() -> Option<Self> {
        let ok = Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            return None;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("repo");
        fs::create_dir(&dir).unwrap();
        let global = tmp.path().join("gitconfig");
        fs::write(&global, "").unwrap();
        let repo = Self {
            _tmp: tmp,
            dir,
            global,
        };
        repo.git(&["init", "-q", "-b", "main"]);
        for (key, value) in [
            ("user.name", "Arcode Test"),
            ("user.email", "test@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
            ("merge.conflictStyle", "merge"),
        ] {
            repo.git(&["config", key, value]);
        }
        Some(repo)
    }

    pub fn repo(&self) -> Repo {
        find_repo(&self.dir).unwrap()
    }

    /// Runs git with isolated global/system config; panics on failure.
    pub fn git(&self, args: &[&str]) -> String {
        let out = self.git_raw(args, &[]);
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// Like [`TestRepo::git`] with extra environment, without asserting.
    pub fn git_raw(&self, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
        Command::new("git")
            .current_dir(&self.dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &self.global)
            .envs(env.iter().copied())
            .args(args)
            .output()
            .unwrap()
    }

    pub fn write(&self, rel: &str, text: &str) {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.dir.join(rel)).unwrap()
    }

    /// Writes, stages and commits one file.
    pub fn commit_file(&self, rel: &str, text: &str, message: &str) {
        self.write(rel, text);
        self.git(&["add", "--", rel]);
        self.git(&["commit", "-q", "-m", message]);
    }

    /// The staged content of `rel`.
    pub fn staged(&self, rel: &str) -> String {
        self.git(&["cat-file", "blob", &format!(":0:{rel}")])
    }
}

pub fn p(rel: &str) -> &Path {
    Path::new(rel)
}

/// `count` numbered lines: `line 1\n` ... `line N\n`.
pub fn numbered(count: usize) -> String {
    (1..=count).map(|i| format!("line {i}\n")).collect()
}
