//! A running `git clone`: started on a thread of its own, reporting
//! progress lines over a channel (the dialog redraws only when the phase
//! or the percentage changes), cancellable at any time.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use arcode_git::{CloneFailure, CloneProgress};
use arcode_project::proc_group::{kill_group, own_group};

/// Most of git's error output kept for the error message.
const MAX_STDERR: usize = 16 * 1024;

/// What a clone reports.
#[derive(Debug, Clone, PartialEq)]
pub enum JobEvent {
    /// A new phase or percentage.
    Progress(CloneProgress),
    /// The clone ended.
    Finished(Result<(), JobError>),
}

/// Why a clone ended without a repository.
#[derive(Debug, Clone, PartialEq)]
pub enum JobError {
    /// The `git` program was not found.
    GitMissing,
    /// git could not be started for another reason.
    Spawn(String),
    /// The user cancelled.
    Cancelled,
    /// git failed.
    Failed(CloneFailure),
}

/// The running clone. Dropping it stops git.
pub struct CloneJob {
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
}

impl CloneJob {
    /// Starts cloning `url` into `target` (absolute; it must not exist or
    /// be empty). Events arrive on the returned channel, `Finished` last.
    /// If the clone fails or is cancelled, what it left is removed again
    /// (git leaves it behind when killed): the `target` folder if it did
    /// not exist before, its contents if it was empty. A folder that had
    /// anything in it is never touched.
    pub fn start(url: String, target: PathBuf) -> (Self, async_channel::Receiver<JobEvent>) {
        let (tx, rx) = async_channel::unbounded();
        let child = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));
        let job = Self {
            child: child.clone(),
            cancelled: cancelled.clone(),
        };
        let before = Before::of(&target);
        std::thread::Builder::new()
            .name("git-clone".into())
            .spawn(move || {
                let result = run(&url, &target, &child, &cancelled, &tx);
                if result.is_err() {
                    clean_up(&target, before);
                }
                let _ = tx.send_blocking(JobEvent::Finished(result));
            })
            .ok();
        (job, rx)
    }

    /// Stops git and everything it started (ssh, remote helpers).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock()
            && let Some(child) = child.as_mut()
        {
            kill_group(child);
        }
    }
}

impl Drop for CloneJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn run(
    url: &str,
    target: &Path,
    slot: &Mutex<Option<Child>>,
    cancelled: &AtomicBool,
    tx: &async_channel::Sender<JobEvent>,
) -> Result<(), JobError> {
    let batch_ssh = !arcode_git::ssh_is_customized();
    let mut command = arcode_git::clone_command(url, target, batch_ssh);
    own_group(&mut command);
    let stderr = {
        let Ok(mut slot) = slot.lock() else {
            return Err(JobError::Spawn("internal error".into()));
        };
        // Cancelled while `git config` ran.
        if cancelled.load(Ordering::SeqCst) {
            return Err(JobError::Cancelled);
        }
        let mut child = command.spawn().map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => JobError::GitMissing,
            _ => JobError::Spawn(err.to_string()),
        })?;
        let stderr = child.stderr.take();
        *slot = Some(child);
        stderr
    };
    let mut output = String::new();
    if let Some(mut stderr) = stderr {
        let mut last: Option<CloneProgress> = None;
        let mut line = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            for &byte in &buf[..n] {
                if byte != b'\r' && byte != b'\n' {
                    line.push(byte);
                    continue;
                }
                let text = String::from_utf8_lossy(&line).into_owned();
                line.clear();
                if let Some(progress) = arcode_git::parse_progress(&text) {
                    if last.as_ref() != Some(&progress) {
                        last = Some(progress.clone());
                        let _ = tx.send_blocking(JobEvent::Progress(progress));
                    }
                } else if !text.trim().is_empty() {
                    push_capped(&mut output, &text);
                }
            }
        }
        push_capped(&mut output, &String::from_utf8_lossy(&line));
    }
    let status = slot
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
        .map(|mut child| child.wait());
    if cancelled.load(Ordering::SeqCst) {
        return Err(JobError::Cancelled);
    }
    match status {
        Some(Ok(status)) if status.success() => Ok(()),
        Some(Ok(_)) => Err(JobError::Failed(arcode_git::clone_failure(&output))),
        Some(Err(err)) => Err(JobError::Spawn(err.to_string())),
        None => Err(JobError::Cancelled),
    }
}

/// Appends a line, keeping the last [`MAX_STDERR`] bytes or so.
fn push_capped(output: &mut String, line: &str) {
    output.push_str(line);
    output.push('\n');
    if output.len() > MAX_STDERR {
        let mut cut = output.len() - MAX_STDERR;
        while !output.is_char_boundary(cut) {
            cut += 1;
        }
        output.drain(..cut);
    }
}

/// The target folder before the clone started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Before {
    /// It did not exist.
    Missing,
    /// It was an empty folder.
    Empty,
    /// Anything else: a file, a folder with entries, unreadable.
    Occupied,
}

impl Before {
    pub(super) fn of(target: &Path) -> Self {
        if !target.exists() {
            return Self::Missing;
        }
        match std::fs::read_dir(target).map(|mut entries| entries.next().is_none()) {
            Ok(true) => Self::Empty,
            _ => Self::Occupied,
        }
    }
}

/// Removes what a failed clone left in `target` (see [`CloneJob::start`]).
pub(super) fn clean_up(target: &Path, before: Before) {
    match before {
        Before::Occupied => {}
        Before::Missing => {
            if target.is_dir() && !target.is_symlink() {
                let _ = std::fs::remove_dir_all(target);
            }
        }
        Before::Empty => {
            if let Ok(entries) = std::fs::read_dir(target) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let _ = if path.is_dir() && !path.is_symlink() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                }
            }
        }
    }
}
