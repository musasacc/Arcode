//! Where the Discord app listens, and connecting to it.
//!
//! Linux and macOS: a Unix socket `discord-ipc-0` … `discord-ipc-9` in the
//! runtime or temporary folder, or in the folder a sandboxed Discord
//! (Flatpak, Snap) exposes there. Windows: the named pipe
//! `\\.\pipe\discord-ipc-0` … `9`. Only local endpoints: nothing here can
//! reach the network.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How many numbered endpoints Discord may use (one per running client).
pub const ENDPOINTS: usize = 10;

/// Environment variables naming the folders the socket may be in, in the
/// order Discord's own libraries try them.
pub const DIR_VARS: [&str; 4] = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"];

/// Sub-folders of a base folder where sandboxed Discord clients put
/// their socket.
pub const SANDBOX_SUBDIRS: [&str; 4] = [
    "app/com.discordapp.Discord",
    "snap.discord",
    ".flatpak/com.discordapp.Discord/xdg-run",
    ".flatpak/dev.vencord.Vesktop/xdg-run",
];

/// The folders to look in, from the environment `var` reads (`/tmp`
/// last), each followed by its sandbox sub-folders, without repeats.
pub fn socket_dirs(var: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let bases = DIR_VARS
        .iter()
        .filter_map(|name| var(name))
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .chain([PathBuf::from("/tmp")]);
    let mut dirs: Vec<PathBuf> = Vec::new();
    for base in bases {
        let candidates =
            std::iter::once(base.clone()).chain(SANDBOX_SUBDIRS.iter().map(|sub| base.join(sub)));
        for dir in candidates {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs
}

/// Every socket path to try: endpoint 0 in each folder, then 1, ….
pub fn socket_paths(dirs: &[PathBuf]) -> Vec<PathBuf> {
    (0..ENDPOINTS)
        .flat_map(|n| dirs.iter().map(move |dir| socket_path(dir, n)))
        .collect()
}

/// The socket of endpoint `n` in `dir`.
pub fn socket_path(dir: &Path, n: usize) -> PathBuf {
    dir.join(format!("discord-ipc-{n}"))
}

/// The Windows pipe names to try.
pub fn pipe_names() -> Vec<String> {
    (0..ENDPOINTS)
        .map(|n| format!(r"\\.\pipe\discord-ipc-{n}"))
        .collect()
}

/// A connection to the Discord app that can say, without blocking,
/// whether something arrived, and whose reads and writes end by a
/// deadline.
pub trait IpcStream: Read + Write + Send {
    /// `Ok(true)` when data is waiting, `Ok(false)` when none is, an error
    /// when the other end closed the connection.
    fn has_data(&mut self) -> io::Result<bool>;

    /// Reads and writes from now on fail with `TimedOut` once `deadline`
    /// has passed instead of waiting for a stuck Discord. `None`: each
    /// waits at most [`IO_TIMEOUT`].
    fn set_deadline(&mut self, deadline: Option<Instant>);
}

impl<T: IpcStream + ?Sized> IpcStream for Box<T> {
    fn has_data(&mut self) -> io::Result<bool> {
        (**self).has_data()
    }

    fn set_deadline(&mut self, deadline: Option<Instant>) {
        (**self).set_deadline(deadline);
    }
}

/// The longest a single read or write waits without a deadline.
pub const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// How often a read or write that cannot block (Windows) looks again.
const IO_POLL: Duration = Duration::from_millis(2);

/// The error of an operation that ran past its deadline.
pub fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "Discord did not answer in time")
}

/// How long the next read or write may wait at `now`: until `deadline`,
/// at most [`IO_TIMEOUT`]; an error once the deadline has passed.
pub fn time_left(deadline: Option<Instant>, now: Instant) -> io::Result<Duration> {
    let Some(deadline) = deadline else {
        return Ok(IO_TIMEOUT);
    };
    let left = deadline.saturating_duration_since(now);
    if left.is_zero() {
        return Err(timed_out());
    }
    Ok(left.min(IO_TIMEOUT))
}

/// A read that never blocks: waits until `available` reports bytes (or
/// the end of `wait`), then reads no more than are there.
pub fn read_available(
    buf: &mut [u8],
    wait: Duration,
    mut available: impl FnMut() -> io::Result<usize>,
    mut read: impl FnMut(&mut [u8]) -> io::Result<usize>,
) -> io::Result<usize> {
    if buf.is_empty() {
        return Ok(0);
    }
    let end = Instant::now() + wait;
    loop {
        let n = available()?;
        if n > 0 {
            let len = n.min(buf.len());
            return read(&mut buf[..len]);
        }
        if Instant::now() >= end {
            return Err(timed_out());
        }
        std::thread::sleep(IO_POLL);
    }
}

/// A write that never blocks: `write` takes what fits (0 while the
/// other end is not reading); tries again until the end of `wait`.
pub fn write_some(
    buf: &[u8],
    wait: Duration,
    mut write: impl FnMut(&[u8]) -> io::Result<usize>,
) -> io::Result<usize> {
    if buf.is_empty() {
        return Ok(0);
    }
    let end = Instant::now() + wait;
    loop {
        let n = write(buf)?;
        if n > 0 {
            return Ok(n);
        }
        if Instant::now() >= end {
            return Err(timed_out());
        }
        std::thread::sleep(IO_POLL);
    }
}

/// Opens the first endpoint that answers.
#[cfg(unix)]
pub fn connect(dirs: &[PathBuf]) -> io::Result<Box<dyn IpcStream>> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "Discord is not running");
    for path in socket_paths(dirs) {
        match unix::UnixIpc::connect(&path) {
            Ok(stream) => return Ok(Box::new(stream)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => last = err,
        }
    }
    Err(last)
}

/// Opens the first endpoint that answers.
#[cfg(windows)]
pub fn connect(_dirs: &[PathBuf]) -> io::Result<Box<dyn IpcStream>> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "Discord is not running");
    for name in pipe_names() {
        match windows::PipeIpc::connect(&name) {
            Ok(stream) => return Ok(Box::new(stream)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => last = err,
        }
    }
    Err(last)
}

/// Platforms without Unix sockets or named pipes have no Discord app to
/// talk to.
#[cfg(not(any(unix, windows)))]
pub fn connect(_dirs: &[PathBuf]) -> io::Result<Box<dyn IpcStream>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no Discord IPC on this platform",
    ))
}

#[cfg(unix)]
#[path = "socket_unix.rs"]
mod unix;

#[cfg(windows)]
#[path = "socket_windows.rs"]
mod windows;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_come_from_the_environment_then_tmp() {
        let env = |name: &str| match name {
            "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_owned()),
            "TMP" => Some("/tmp".to_owned()),
            "TEMP" => Some("  ".to_owned()),
            _ => None,
        };
        let dirs = socket_dirs(env);
        let shown: Vec<_> = dirs.iter().map(|d| d.to_string_lossy()).collect();
        assert_eq!(
            shown,
            [
                "/run/user/1000",
                "/run/user/1000/app/com.discordapp.Discord",
                "/run/user/1000/snap.discord",
                "/run/user/1000/.flatpak/com.discordapp.Discord/xdg-run",
                "/run/user/1000/.flatpak/dev.vencord.Vesktop/xdg-run",
                "/tmp",
                "/tmp/app/com.discordapp.Discord",
                "/tmp/snap.discord",
                "/tmp/.flatpak/com.discordapp.Discord/xdg-run",
                "/tmp/.flatpak/dev.vencord.Vesktop/xdg-run",
            ]
        );
        assert_eq!(socket_dirs(|_| None).len(), 1 + SANDBOX_SUBDIRS.len());
    }

    #[test]
    fn every_endpoint_of_every_folder_is_tried_lowest_first() {
        let dirs = [PathBuf::from("/a"), PathBuf::from("/b")];
        let paths = socket_paths(&dirs);
        assert_eq!(paths.len(), 2 * ENDPOINTS);
        assert_eq!(paths[0], Path::new("/a/discord-ipc-0"));
        assert_eq!(paths[1], Path::new("/b/discord-ipc-0"));
        assert_eq!(paths[19], Path::new("/b/discord-ipc-9"));
    }

    #[test]
    fn pipe_names_are_numbered() {
        let names = pipe_names();
        assert_eq!(names.len(), ENDPOINTS);
        assert_eq!(names[0], r"\\.\pipe\discord-ipc-0");
        assert_eq!(names[9], r"\\.\pipe\discord-ipc-9");
    }

    #[test]
    fn time_left_is_bounded_by_the_deadline_and_the_io_timeout() {
        let now = Instant::now();
        assert_eq!(time_left(None, now).unwrap(), IO_TIMEOUT);
        let soon = now + Duration::from_millis(30);
        assert_eq!(
            time_left(Some(soon), now).unwrap(),
            Duration::from_millis(30)
        );
        let late = now + IO_TIMEOUT * 3;
        assert_eq!(time_left(Some(late), now).unwrap(), IO_TIMEOUT);
        let err = time_left(Some(now), now).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn pipe_reads_take_only_what_is_there_and_give_up_in_time() {
        // Three bytes arrive after two empty looks.
        let mut looks = [0, 0, 3].into_iter();
        let mut asked = 0;
        let mut buf = [0u8; 8];
        let n = read_available(
            &mut buf,
            Duration::from_secs(1),
            || Ok(looks.next().unwrap_or(0)),
            |b| {
                asked = b.len();
                b.fill(7);
                Ok(b.len())
            },
        )
        .unwrap();
        assert_eq!((n, asked), (3, 3), "never asks for more than is there");
        // Nothing arrives: a timeout, without a blocking read.
        let started = Instant::now();
        let err = read_available(
            &mut buf,
            Duration::from_millis(30),
            || Ok(0),
            |_| panic!("read without data"),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
        // A broken pipe is reported at once.
        let err = read_available(
            &mut buf,
            Duration::from_secs(5),
            || Err(io::ErrorKind::BrokenPipe.into()),
            |_| Ok(0),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn pipe_writes_try_again_while_full_and_give_up_in_time() {
        let mut tries = 0;
        let n = write_some(b"hello", Duration::from_secs(1), |b| {
            tries += 1;
            Ok(if tries < 3 { 0 } else { b.len() })
        })
        .unwrap();
        assert_eq!((n, tries), (5, 3));
        let started = Instant::now();
        let err = write_some(b"hello", Duration::from_millis(30), |_| Ok(0)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn nothing_listening_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = connect(&[dir.path().to_path_buf()]).err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
