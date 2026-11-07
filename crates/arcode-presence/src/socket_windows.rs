//! The Discord named pipe on Windows.
//!
//! The pipe is opened as a file, whose reads and writes would wait for
//! as long as Discord takes. So reads only take the bytes
//! `PeekNamedPipe` reports, and the handle is switched to non-blocking
//! writes (`PIPE_NOWAIT`), which take what fits; both look again until
//! the deadline ([`super::read_available`], [`super::write_some`]).

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::io::AsRawHandle;
use std::time::Instant;

use super::{read_available, time_left, write_some};

/// `PIPE_READMODE_BYTE | PIPE_NOWAIT`.
const BYTES_NO_WAIT: u32 = 0x0000_0001;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn PeekNamedPipe(
        pipe: *mut c_void,
        buffer: *mut c_void,
        buffer_size: u32,
        bytes_read: *mut u32,
        total_available: *mut u32,
        bytes_left: *mut u32,
    ) -> i32;

    fn SetNamedPipeHandleState(
        pipe: *mut c_void,
        mode: *mut u32,
        max_collection_count: *mut u32,
        collect_data_timeout: *mut u32,
    ) -> i32;
}

/// A named pipe opened as a file.
pub struct PipeIpc {
    file: File,
    deadline: Option<Instant>,
}

impl PipeIpc {
    pub fn connect(name: &str) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(name)?;
        let mut mode = BYTES_NO_WAIT;
        // SAFETY: the handle is the pipe just opened and owned by `file`;
        // `mode` is a valid u32 for the call, the other pointers are null
        // (not changed).
        let ok = unsafe {
            SetNamedPipeHandleState(
                file.as_raw_handle(),
                &mut mode,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            log::debug!(
                "discord: pipe stays blocking: {}",
                io::Error::last_os_error()
            );
        }
        Ok(Self {
            file,
            deadline: None,
        })
    }

    /// Bytes waiting in the pipe; an error once Discord went away.
    fn available(&self) -> io::Result<usize> {
        let mut available = 0u32;
        // SAFETY: the handle is the open pipe owned by `self.file`; only
        // the count is written, through a valid pointer, and no buffer is
        // passed.
        let ok = unsafe {
            PeekNamedPipe(
                self.file.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            // A broken pipe: Discord went away.
            return Err(io::Error::last_os_error());
        }
        Ok(available as usize)
    }
}

impl super::IpcStream for PipeIpc {
    fn has_data(&mut self) -> io::Result<bool> {
        Ok(self.available()? > 0)
    }

    fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }
}

impl Read for PipeIpc {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let wait = time_left(self.deadline, Instant::now())?;
        let this = &*self;
        read_available(buf, wait, || this.available(), |b| (&this.file).read(b))
    }
}

impl Write for PipeIpc {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let wait = time_left(self.deadline, Instant::now())?;
        let file = &self.file;
        write_some(buf, wait, |b| (&*file).write(b))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
