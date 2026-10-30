//! The per-plugin log shown in the Plugins view (§4, §6 `host.log`): a
//! line-oriented ring buffer capped at [`LOG_CAP_BYTES`].
//!
//! `host.log` adds whole lines; guest stdout/stderr are line-buffered here,
//! so a flood of output costs at most the cap plus one partial line.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::bindings::Level;
use crate::cancel::lock;

/// Total bytes of text kept per plugin.
pub const LOG_CAP_BYTES: usize = 1024 * 1024;

/// A partial stdout line longer than this is flushed as its own line.
const MAX_LINE_BYTES: usize = 16 * 1024;

/// Where a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSource {
    /// `host.log`.
    Host,
    /// Guest stdout.
    Stdout,
    /// Guest stderr.
    Stderr,
}

/// One log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Origin.
    pub source: LogSource,
    /// Severity (`Info` for stdout, `Warn` for stderr).
    pub level: Level,
    /// The text, without the newline.
    pub text: String,
}

#[derive(Debug, Default)]
struct Ring {
    lines: VecDeque<LogLine>,
    bytes: usize,
    dropped_lines: u64,
    pending_stdout: Vec<u8>,
    pending_stderr: Vec<u8>,
}

/// A shared, capped plugin log. Cloning shares the buffer.
#[derive(Debug, Clone, Default)]
pub struct PluginLog {
    ring: Arc<Mutex<Ring>>,
}

impl PluginLog {
    /// An empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a `host.log` line.
    pub fn push(&self, level: Level, text: &str) {
        let mut ring = lock(&self.ring);
        push_line(&mut ring, LogSource::Host, level, text);
    }

    /// Appends raw guest output, splitting it into lines.
    pub fn write_stream(&self, source: LogSource, bytes: &[u8]) {
        let mut guard = lock(&self.ring);
        let ring = &mut *guard;
        let pending = match source {
            LogSource::Stderr => &mut ring.pending_stderr,
            _ => &mut ring.pending_stdout,
        };
        let mut lines = Vec::new();
        for &b in bytes {
            if b == b'\n' {
                lines.push(std::mem::take(pending));
            } else {
                pending.push(b);
                if pending.len() >= MAX_LINE_BYTES {
                    lines.push(std::mem::take(pending));
                }
            }
        }
        let level = match source {
            LogSource::Stderr => Level::Warn,
            _ => Level::Info,
        };
        for line in lines {
            push_line(ring, source, level, &String::from_utf8_lossy(&line));
        }
    }

    /// A copy of the kept lines, oldest first.
    pub fn lines(&self) -> Vec<LogLine> {
        lock(&self.ring).lines.iter().cloned().collect()
    }

    /// Bytes of text currently kept.
    pub fn bytes(&self) -> usize {
        lock(&self.ring).bytes
    }

    /// Lines evicted to stay under the cap.
    pub fn dropped_lines(&self) -> u64 {
        lock(&self.ring).dropped_lines
    }
}

fn push_line(ring: &mut Ring, source: LogSource, level: Level, text: &str) {
    let mut text = text.to_owned();
    if text.len() > MAX_LINE_BYTES {
        let mut cut = MAX_LINE_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    ring.bytes += text.len();
    ring.lines.push_back(LogLine {
        source,
        level,
        text,
    });
    while ring.bytes > LOG_CAP_BYTES {
        let Some(old) = ring.lines.pop_front() else {
            break;
        };
        ring.bytes -= old.text.len();
        ring.dropped_lines += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lines_and_keeps_partial_ones() {
        let log = PluginLog::new();
        log.write_stream(LogSource::Stdout, b"one\ntw");
        log.write_stream(LogSource::Stderr, b"err\n");
        log.write_stream(LogSource::Stdout, b"o\n");
        log.push(Level::Debug, "host");
        let lines: Vec<_> = log
            .lines()
            .into_iter()
            .map(|l| (l.source, l.text))
            .collect();
        assert_eq!(
            lines,
            [
                (LogSource::Stdout, "one".to_owned()),
                (LogSource::Stderr, "err".to_owned()),
                (LogSource::Stdout, "two".to_owned()),
                (LogSource::Host, "host".to_owned()),
            ]
        );
    }

    #[test]
    fn ring_stays_under_the_cap() {
        let log = PluginLog::new();
        let chunk = format!("{}\n", "x".repeat(999));
        for _ in 0..3000 {
            log.write_stream(LogSource::Stdout, chunk.as_bytes());
        }
        assert!(log.bytes() <= LOG_CAP_BYTES);
        assert!(log.bytes() > LOG_CAP_BYTES - 1000);
        assert_eq!(log.dropped_lines() as usize + log.lines().len(), 3000);
        // A newline-free flood is split instead of growing without bound.
        log.write_stream(LogSource::Stdout, &vec![b'y'; 5 * MAX_LINE_BYTES]);
        assert!(log.lines().iter().all(|l| l.text.len() <= MAX_LINE_BYTES));
    }
}
