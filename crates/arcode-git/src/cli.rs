//! Thin wrapper around the `git` command-line tool for read-only, local
//! queries. Never used for network operations.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::{Head, Repo, paths};

/// Environment variables that would redirect git away from the repository
/// we resolved ourselves (e.g. when Arcode itself runs inside a git hook).
const SCRUBBED_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
];

/// Builds a `git` command running in `workdir` with interactive prompts
/// disabled, unquoted paths, no optional locks and no fsmonitor hook.
pub(crate) fn git_command(workdir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(workdir)
        .args(["-c", "core.quotepath=off", "-c", "core.fsmonitor=false"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for var in SCRUBBED_ENV {
        cmd.env_remove(var);
    }
    hide_console_window(&mut cmd);
    cmd
}

#[cfg(windows)]
fn hide_console_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`: don't flash a console for a GUI app's child.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_cmd: &mut Command) {}

/// Runs git with `args` in `workdir` and returns stdout on success.
/// Returns `None` if git is missing, fails to start, or exits non-zero.
pub(crate) fn run<I, S>(workdir: &Path, args: I) -> Option<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_command(workdir).args(args).output().ok()?;
    output.status.success().then_some(output.stdout)
}

/// Runs git and returns trimmed UTF-8 stdout.
fn run_line(workdir: &Path, args: &[&str]) -> Option<String> {
    let out = run(workdir, args)?;
    let text = String::from_utf8(out).ok()?;
    Some(text.trim().to_owned())
}

/// Resolves `HEAD` through the CLI (used only for the reftable backend).
pub(crate) fn head_via_cli(repo: &Repo) -> Option<Head> {
    let dir = &repo.workdir;
    if let Some(refname) = run_line(dir, &["symbolic-ref", "-q", "HEAD"]) {
        let name = refname
            .strip_prefix("refs/heads/")
            .unwrap_or(&refname)
            .to_owned();
        let born = run(dir, ["rev-parse", "-q", "--verify", "HEAD"]).is_some();
        return Some(if born {
            Head::Branch(name)
        } else {
            Head::Unborn(name)
        });
    }
    let sha = run_line(dir, &["rev-parse", "--short=7", "HEAD"])?;
    Some(Head::Detached(sha))
}

impl Repo {
    /// Returns the content of `rel_path` as committed in `HEAD`.
    ///
    /// `rel_path` is relative to [`Repo::workdir`] (an absolute path inside
    /// the working tree is accepted too). Runs `git cat-file blob
    /// HEAD:<path>`, which like `git show` prints the raw blob but also
    /// rejects directories and submodules.
    ///
    /// Returns `None` when git is not installed, the file is not tracked in
    /// `HEAD`, the repository has no commits, or the blob is not UTF-8.
    pub fn head_file_contents(&self, rel_path: &Path) -> Option<String> {
        let rel = self.relative_path(rel_path)?;
        let git_path = paths::to_git_path(&rel)?;
        let spec = format!("HEAD:{git_path}");
        let out = run(&self.workdir, ["cat-file", "blob", spec.as_str()])?;
        String::from_utf8(out).ok()
    }
}
