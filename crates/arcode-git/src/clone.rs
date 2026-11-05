//! Cloning a repository the user asked for (`git clone --progress`).
//!
//! This is the only part of the crate that reaches the network, and only
//! for a clone the user started. The command never waits on a prompt
//! nobody can see: `GIT_TERMINAL_PROMPT=0` makes HTTPS fail instead of
//! asking for a user name, and unless the user configured their own ssh
//! command, ssh runs with `BatchMode=yes` (keys from the agent or without
//! a passphrase still work; a passphrase or an unknown host key fails).
//! [`clone_failure`] turns git's message into something a dialog can
//! explain.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::cli::{SCRUBBED_ENV, hide_console_window};

/// The ssh command used when the user has none of their own: it fails
/// instead of asking for a passphrase or a host key confirmation.
pub const BATCH_SSH_COMMAND: &str = "ssh -o BatchMode=yes";

/// The folder name `git clone` would pick for `url`: the last path part
/// without a trailing `.git` or `/` (`https://host/owner/repo.git`,
/// `git@host:owner/repo.git` and `/srv/repo/` all give `repo`). Falls back
/// to `"repository"` when nothing usable is left.
pub fn folder_name_from_url(url: &str) -> String {
    let url = url.trim();
    let url = url.split(['?', '#']).next().unwrap_or(url);
    let trimmed = url.trim_end_matches(['/', '\\']);
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let trimmed = trimmed.trim_end_matches(['/', '\\']);
    // `host:repo` (scp-like ssh) has no slash before the name.
    let last = trimmed
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or(trimmed)
        .trim();
    if last.is_empty() || last == "." || last == ".." || last.contains('@') {
        "repository".to_owned()
    } else {
        last.to_owned()
    }
}

/// Builds `git clone --progress -- <url> <target>`.
///
/// `target` should be absolute. Progress and errors go to stderr (piped),
/// stdout and stdin are closed. With `batch_ssh`, ssh runs with
/// [`BATCH_SSH_COMMAND`]; pass `false` when the user set `GIT_SSH`,
/// `GIT_SSH_COMMAND` or `core.sshCommand` (see [`ssh_is_customized`]).
pub fn clone_command(url: &str, target: &Path, batch_ssh: bool) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(["clone", "--progress", "--", url])
        .arg(target)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LANGUAGE", "en")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if batch_ssh {
        cmd.env("GIT_SSH_COMMAND", BATCH_SSH_COMMAND);
    }
    for var in SCRUBBED_ENV {
        cmd.env_remove(var);
    }
    hide_console_window(&mut cmd);
    cmd
}

/// Whether the user chose their own ssh command (`GIT_SSH`,
/// `GIT_SSH_COMMAND` or `core.sshCommand`), which a clone must then use
/// as it is. Runs `git config`: call it off the UI thread.
pub fn ssh_is_customized() -> bool {
    let env_set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    if env_set("GIT_SSH") || env_set("GIT_SSH_COMMAND") {
        return true;
    }
    let mut cmd = Command::new("git");
    cmd.args(["config", "--get", "core.sshCommand"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for var in SCRUBBED_ENV {
        cmd.env_remove(var);
    }
    hide_console_window(&mut cmd);
    cmd.output()
        .is_ok_and(|out| out.status.success() && !out.stdout.trim_ascii().is_empty())
}

/// One progress line of `git clone --progress`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneProgress {
    /// What git is doing ("Receiving objects", "Resolving deltas").
    pub phase: String,
    /// How far along that is, when git says.
    pub percent: Option<u8>,
}

/// Parses a progress line (`Receiving objects:  45% (450/1000), 1.2 MiB`,
/// `remote: Counting objects: 100% (5/5), done.`). Other lines (errors,
/// hints, "Cloning into...") give `None`.
pub fn parse_progress(line: &str) -> Option<CloneProgress> {
    let line = line.trim();
    let line = line.strip_prefix("remote:").unwrap_or(line).trim_start();
    let (phase, rest) = line.split_once(':')?;
    let words = phase.trim();
    let lower = words.to_ascii_lowercase();
    if words.is_empty()
        || !words.chars().all(|c| c.is_ascii_alphabetic() || c == ' ')
        || ["fatal", "error", "warning", "hint"].contains(&lower.as_str())
    {
        return None;
    }
    let percent = rest.split_once('%').and_then(|(before, _)| {
        let digits = before.trim();
        digits.parse::<u8>().ok().filter(|p| *p <= 100)
    });
    Some(CloneProgress {
        phase: words.to_owned(),
        percent,
    })
}

/// Why a clone failed, from git's error output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneFailure {
    /// The server wants a user name, password, token or ssh key that git
    /// could not get without asking.
    AuthRequired,
    /// ssh does not know the server's host key yet.
    UnknownHostKey,
    /// The repository does not exist (or is private and needs a login).
    NotFound,
    /// The host could not be reached.
    Network,
    /// The target folder exists and is not empty.
    TargetNotEmpty,
    /// Anything else: git's last error line.
    Other(String),
}

/// Classifies git's stderr (English messages, see [`clone_command`]).
pub fn clone_failure(stderr: &str) -> CloneFailure {
    let has = |needle: &str| stderr.contains(needle);
    if has("Host key verification failed") {
        CloneFailure::UnknownHostKey
    } else if has("terminal prompts disabled")
        || has("could not read Username")
        || has("could not read Password")
        || has("Authentication failed")
        || has("Permission denied (")
        || has("Permission denied, please try again")
        || has("Invalid username or password")
    {
        CloneFailure::AuthRequired
    } else if has("already exists and is not an empty directory") {
        CloneFailure::TargetNotEmpty
    } else if has("Could not resolve host")
        || has("Could not resolve hostname")
        || has("Failed to connect")
        || has("Connection timed out")
        || has("Network is unreachable")
        || has("Connection refused")
        || has("CONNECT tunnel failed")
        || has("Proxy CONNECT aborted")
        || has("Could not resolve proxy")
        || has("from proxy after CONNECT")
        || has("SOCKS")
        || has("Operation timed out")
    {
        CloneFailure::Network
    } else if has("Repository not found")
        || has("does not appear to be a git repository")
        || (has("repository '") && has("' not found"))
        || has("does not exist")
    {
        CloneFailure::NotFound
    } else {
        CloneFailure::Other(last_error_line(stderr))
    }
}

/// The last `fatal:` / `error:` line of `stderr` without its prefix, else
/// its last non-empty line.
fn last_error_line(stderr: &str) -> String {
    let lines = || stderr.split(['\n', '\r']).map(str::trim);
    lines()
        .filter_map(|l| {
            l.strip_prefix("fatal:")
                .or_else(|| l.strip_prefix("error:"))
                .map(str::trim)
        })
        .next_back()
        .or_else(|| lines().rfind(|l| !l.is_empty()))
        .unwrap_or("git clone failed")
        .to_owned()
}

#[cfg(test)]
#[path = "clone_tests.rs"]
mod tests;
