//! What the Clone Repository dialog checks and says: the request built
//! from its fields, the command "Clone in Terminal" types, and the
//! messages for a clone that ended without a repository.

use std::path::PathBuf;

use arcode_git::{CloneFailure, folder_name_from_url};
use arcode_project::{Shell, quote_arg_in};

use super::job::{self, JobError};
use crate::path_prompt::expand_home;

/// A checked request: clone `url` into `parent/folder`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloneRequest {
    /// The repository address as typed (trimmed; a local path starting
    /// with `~` has it expanded).
    pub url: String,
    /// The folder the clone goes into.
    pub parent: PathBuf,
    /// The clone's folder name inside `parent`.
    pub folder: String,
}

impl CloneRequest {
    /// The folder the repository is cloned into.
    pub fn target(&self) -> PathBuf {
        self.parent.join(&self.folder)
    }
}

/// The folder name proposed for `url` (empty while there is no address).
pub fn proposed_folder(url: &str) -> String {
    if url.trim().is_empty() {
        String::new()
    } else {
        folder_name_from_url(url)
    }
}

/// Whether `url` names a repository on this machine: a path (`/srv/repo`,
/// `~/code/repo`, `./repo`, `C:\code\repo`, `\\server\share\repo`) or a
/// `file://` address. Such an address may contain spaces.
pub fn is_local_path(url: &str) -> bool {
    let bytes = url.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    drive
        || url.starts_with(['/', '\\', '~', '.'])
        || url
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("file://"))
}

/// Checks the dialog's fields (`location` may start with `~`). Looks at
/// the disk once: the target must not exist, or be an empty folder.
pub fn check_request(url: &str, location: &str, folder: &str) -> Result<CloneRequest, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("Enter the address of the repository.".into());
    }
    let local = is_local_path(url);
    if url.starts_with('-') || (!local && url.chars().any(char::is_whitespace)) {
        return Err("That is not a repository address.".into());
    }
    // git runs without a shell, which would have expanded it.
    let url = if url.starts_with('~') {
        expand_home(url).display().to_string()
    } else {
        url.to_owned()
    };
    let folder = folder.trim();
    if folder.is_empty() {
        return Err("Enter a name for the folder.".into());
    }
    if folder.contains(['/', '\\']) || folder == "." || folder == ".." {
        return Err("The folder name cannot contain / or \\.".into());
    }
    let location = location.trim();
    if location.is_empty() {
        return Err("Choose where to put the clone.".into());
    }
    let parent = expand_home(location);
    if !parent.is_absolute() {
        return Err("Enter the full path of the parent folder.".into());
    }
    let request = CloneRequest {
        url,
        parent,
        folder: folder.to_owned(),
    };
    let target = request.target();
    if job::Before::of(&target) == job::Before::Occupied {
        let home = std::env::home_dir();
        return Err(format!(
            "{} already exists and is not empty. Choose another folder name.",
            crate::welcome::tilde_path(&target, home.as_deref())
        ));
    }
    Ok(request)
}

/// The command line "Clone in Terminal" types, quoted for `shell`:
/// `git clone <url> <folder>` when the terminal starts in the parent
/// folder (`in_parent`), else with the full target path.
pub fn terminal_command(shell: Shell, request: &CloneRequest, in_parent: bool) -> String {
    let target = if in_parent {
        request.folder.clone()
    } else {
        request.target().display().to_string()
    };
    format!(
        "git clone {} {}",
        quote_arg_in(shell, &request.url),
        quote_arg_in(shell, &target)
    )
}

/// What the dialog says when a clone ended without a repository.
pub fn failure_message(error: &JobError) -> String {
    match error {
        JobError::GitMissing => "Git is not installed (or not on PATH). Install it with your \
             package manager or from git-scm.com, then try again."
            .into(),
        JobError::Spawn(err) => format!("Could not start git: {err}"),
        JobError::Cancelled => "Clone cancelled.".into(),
        JobError::Failed(failure) => match failure {
            CloneFailure::AuthRequired => "The server asks you to sign in, which Arcode cannot do \
                 here. Set up an SSH key in your ssh agent or a git credential helper, or use \
                 Clone in Terminal to type your credentials."
                .into(),
            CloneFailure::UnknownHostKey => "ssh does not know this server's host key yet. Use \
                 Clone in Terminal once to check and accept it."
                .into(),
            CloneFailure::NotFound => "Repository not found. Check the address; a private \
                 repository also needs you to sign in."
                .into(),
            CloneFailure::Network => "Could not reach the server. Check the address, your \
                 network connection and your proxy settings."
                .into(),
            CloneFailure::TargetNotEmpty => {
                "The folder already exists and is not empty. Choose another folder name.".into()
            }
            CloneFailure::Other(line) => format!("git clone failed: {line}"),
        },
    }
}

/// Whether `error` is something to fix (drawn as an error) rather than
/// what the user asked for (a cancelled clone).
pub fn is_problem(error: &JobError) -> bool {
    !matches!(error, JobError::Cancelled)
}

/// Whether "Clone in Terminal" is the way out of `error`.
pub fn needs_terminal(error: &JobError) -> bool {
    matches!(
        error,
        JobError::Failed(CloneFailure::AuthRequired | CloneFailure::UnknownHostKey)
    )
}
