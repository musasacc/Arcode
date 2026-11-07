//! The Discord socket on Linux and macOS.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Instant;

use super::time_left;

/// A Unix socket; one byte read ahead by [`super::IpcStream::has_data`]
/// is kept until the next read. Every read and write waits at most until
/// the deadline, so a stuck Discord never holds the presence thread.
pub struct UnixIpc {
    stream: UnixStream,
    ahead: Option<u8>,
    deadline: Option<Instant>,
}

impl UnixIpc {
    pub fn connect(path: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        Ok(Self {
            stream,
            ahead: None,
            deadline: None,
        })
    }
}

/// A socket timeout reads as `WouldBlock`; it is a timeout here.
fn timeout_as_timed_out(err: io::Error) -> io::Error {
    if err.kind() == io::ErrorKind::WouldBlock {
        super::timed_out()
    } else {
        err
    }
}

impl super::IpcStream for UnixIpc {
    fn has_data(&mut self) -> io::Result<bool> {
        if self.ahead.is_some() {
            return Ok(true);
        }
        self.stream.set_nonblocking(true)?;
        let mut byte = [0u8];
        let read = self.stream.read(&mut byte);
        self.stream.set_nonblocking(false)?;
        match read {
            Ok(0) => Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(_) => {
                self.ahead = Some(byte[0]);
                Ok(true)
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }
}

impl Read for UnixIpc {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if let Some(byte) = self.ahead.take() {
            buf[0] = byte;
            return Ok(1);
        }
        let wait = time_left(self.deadline, Instant::now())?;
        self.stream.set_read_timeout(Some(wait))?;
        self.stream.read(buf).map_err(timeout_as_timed_out)
    }
}

impl Write for UnixIpc {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let wait = time_left(self.deadline, Instant::now())?;
        self.stream.set_write_timeout(Some(wait))?;
        self.stream.write(buf).map_err(timeout_as_timed_out)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}
