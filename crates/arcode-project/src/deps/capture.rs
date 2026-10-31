//! Runs a check command in the background and captures its output, with
//! a flag to cancel it (the process and what it started are killed).

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::commands::PackageCommand;
use crate::proc_group;

/// Most output kept per stream.
const MAX_OUTPUT: usize = 8 * 1024 * 1024;
/// How often the process is polled for exit and cancellation.
const POLL: Duration = Duration::from_millis(50);

/// Shared cancellation flag.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    /// A flag that is not set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the running command to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether [`CancelFlag::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// What a captured command printed and how it ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapturedOutput {
    /// Exited with status 0.
    pub success: bool,
    /// The exit code (`None` when killed).
    pub code: Option<i32>,
    /// Standard output (lossy UTF-8, capped).
    pub stdout: String,
    /// Standard error (lossy UTF-8, capped).
    pub stderr: String,
    /// Stopped by the cancel flag or the timeout.
    pub cancelled: bool,
}

/// Runs `cmd` without a shell and waits for it, killing it (and what it
/// started) when `cancel` is set or `timeout` passes. Colors and update
/// notices are turned off. Blocks: call it off the UI thread.
pub fn capture(
    cmd: &PackageCommand,
    cancel: &CancelFlag,
    timeout: Duration,
) -> std::io::Result<CapturedOutput> {
    let mut command = Command::new(&cmd.program);
    command
        .args(&cmd.args)
        .current_dir(&cmd.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1")
        .env("CARGO_TERM_COLOR", "never")
        .env("NPM_CONFIG_UPDATE_NOTIFIER", "false")
        .env("NPM_CONFIG_FUND", "false")
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    proc_group::own_group(&mut command);
    let mut child = command.spawn()?;
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(pipe) = pipe {
                let _ = pipe.take(MAX_OUTPUT as u64).read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let start = Instant::now();
    let mut cancelled = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if cancel.is_cancelled() || start.elapsed() > timeout {
            cancelled = true;
            proc_group::kill_group(&mut child);
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(POLL);
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    Ok(CapturedOutput {
        success: status.is_some_and(|s| s.success()),
        code: status.and_then(|s| s.code()),
        stdout,
        stderr,
        cancelled,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> PackageCommand {
        PackageCommand::new(
            "sh".into(),
            vec!["-c".into(), script.into()],
            &std::env::temp_dir(),
            None,
        )
    }

    #[test]
    fn captures_output_and_status() {
        let out = capture(
            &sh("echo out; echo err >&2; exit 3"),
            &CancelFlag::new(),
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(out.stdout, "out\n");
        assert_eq!(out.stderr, "err\n");
        assert_eq!(out.code, Some(3));
        assert!(!out.success && !out.cancelled);
    }

    #[test]
    fn cancel_kills_the_command() {
        let flag = CancelFlag::new();
        let f = flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            f.cancel();
        });
        let start = Instant::now();
        let out = capture(&sh("sleep 30"), &flag, Duration::from_secs(60)).unwrap();
        assert!(out.cancelled);
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn missing_program_is_an_error() {
        let mut cmd = sh("");
        cmd.program = "arcode-no-such-program-xyz".into();
        assert!(capture(&cmd, &CancelFlag::new(), Duration::from_secs(5)).is_err());
    }
}
