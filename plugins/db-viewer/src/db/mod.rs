//! One open SQLite database (§10 "Opening files").
//!
//! - Opened as `file:/project/<rel>?mode=ro&nolock=1`: WASI has no `fcntl`
//!   locks, and SQLite reads pages with `pread`, so the file is never copied.
//! - A WAL database (a `-wal` file next to it, or WAL in the header) is
//!   opened with `immutable=1` instead: WAL needs shared memory, which WASI
//!   lacks. That is a snapshot; uncheckpointed WAL changes are not shown.
//! - `mode=rw` only after the user turned writes on. When the workspace
//!   grant is read-only, SQLite falls back to read-only by itself and
//!   [`Database::writable`] says so.
//!
//! Every statement runs under a progress handler ([`Limit`]) that polls
//! `host.cancelled()` every 10k VM steps and, for interactive calls, stops
//! at a deadline so the plugin never runs into the host's CPU budget.

mod schema;

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use arcode_plugin::host;
use rusqlite::{Connection, ErrorCode, OpenFlags};

pub use schema::{ColumnInfo, Kind, Object};

use crate::sql::{OpenMode, file_uri};

/// VM steps between two progress-handler calls (§10).
const PROGRESS_STEPS: i32 = 10_000;

/// Why a statement stopped early.
const STOP_NONE: u8 = 0;
const STOP_CANCELLED: u8 = 1;
const STOP_DEADLINE: u8 = 2;

/// How long a statement may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// Until the user cancels (task calls).
    UntilCancelled,
    /// Until cancelled or `Duration` has passed (interactive calls).
    Deadline(Duration),
}

/// A failed database operation, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbError {
    /// The user cancelled.
    Cancelled,
    /// Stopped at an interactive deadline.
    TooSlow(Duration),
    /// SQLite's (or the file system's) own message.
    Message(String),
}

impl DbError {
    /// Plain text for the panel.
    pub fn message(&self) -> String {
        match self {
            Self::Cancelled => "Query cancelled".to_owned(),
            Self::TooSlow(d) => format!(
                "Stopped after {} ms: too slow for an interactive view. Run it from the Query tab instead.",
                d.as_millis()
            ),
            Self::Message(m) => m.clone(),
        }
    }
}

/// An open database and how it was opened.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
    mode: OpenMode,
    writable: bool,
    stop: Arc<AtomicU8>,
    limit: Option<Limit>,
}

/// `root/rel`, refusing absolute paths and `..` (the WASI preopen refuses
/// them too; this gives a clear message first).
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, DbError> {
    let rel_path = Path::new(rel);
    let plain = rel_path
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if rel.is_empty() || !plain {
        return Err(DbError::Message(format!("not a project path: {rel}")));
    }
    Ok(root.join(rel_path))
}

/// True when the database must be opened as a snapshot: a `-wal` file
/// exists, or the header says WAL (bytes 18-19 are 2).
pub fn needs_snapshot(path: &Path) -> bool {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    if Path::new(&wal).exists() {
        return true;
    }
    let mut header = [0u8; 20];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut header))
        .is_ok_and(|()| header[18] == 2 || header[19] == 2)
}

impl Database {
    /// Opens `rel` under `root` (`/project` in the sandbox). With
    /// `want_writes`, opens read-write unless it must be a snapshot.
    pub fn open(root: &Path, rel: &str, want_writes: bool) -> Result<Self, DbError> {
        let path = resolve(root, rel)?;
        if !path.is_file() {
            return Err(DbError::Message(format!("{rel}: file not found")));
        }
        let mode = if needs_snapshot(&path) {
            OpenMode::Snapshot
        } else if want_writes {
            OpenMode::ReadWrite
        } else {
            OpenMode::ReadOnly
        };
        let access = match mode {
            OpenMode::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE,
            _ => OpenFlags::SQLITE_OPEN_READ_ONLY,
        };
        let flags = access | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let uri = file_uri(&path.to_string_lossy(), mode);
        let conn = Connection::open_with_flags(&uri, flags).map_err(|e| sqlite_error(&e))?;
        let writable = mode == OpenMode::ReadWrite
            && !conn
                .is_readonly(rusqlite::MAIN_DB)
                .map_err(|e| sqlite_error(&e))?;
        // Memory, not temp files: the sandbox has no writable temp dir.
        let pragmas = format!(
            "PRAGMA cache_size = -4096; PRAGMA temp_store = MEMORY; PRAGMA query_only = {};",
            if writable { "OFF" } else { "ON" }
        );
        conn.execute_batch(&pragmas).map_err(|e| sqlite_error(&e))?;
        let mut db = Self {
            conn,
            mode,
            writable,
            stop: Arc::new(AtomicU8::new(STOP_NONE)),
            limit: None,
        };
        // Reading the schema is what reports "file is not a database".
        db.limit(Limit::Deadline(Duration::from_millis(500)));
        db.conn
            .query_row("SELECT count(*) FROM sqlite_schema", [], |_| Ok(()))
            .map_err(|e| db.error(&e))?;
        Ok(db)
    }

    /// How the file was opened.
    pub fn mode(&self) -> OpenMode {
        self.mode
    }

    /// True when statements may write (writes on *and* a write grant).
    pub fn writable(&self) -> bool {
        self.writable
    }

    /// The connection, for preparing statements.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Installs the progress handler for the next statements.
    pub fn limit(&mut self, limit: Limit) {
        self.stop.store(STOP_NONE, Ordering::Relaxed);
        if self.limit == Some(limit) && limit == Limit::UntilCancelled {
            return;
        }
        self.limit = Some(limit);
        let stop = self.stop.clone();
        let deadline = match limit {
            Limit::UntilCancelled => None,
            Limit::Deadline(d) => Some((Instant::now() + d, d)),
        };
        let handler = move || {
            if host::cancelled() {
                stop.store(STOP_CANCELLED, Ordering::Relaxed);
                return true;
            }
            if deadline.is_some_and(|(at, _)| Instant::now() >= at) {
                stop.store(STOP_DEADLINE, Ordering::Relaxed);
                return true;
            }
            false
        };
        // Only fails if the connection is gone, which `&mut self` rules out.
        let _ = self.conn.progress_handler(PROGRESS_STEPS, Some(handler));
    }

    /// Turns a rusqlite error into a [`DbError`], telling cancellation and
    /// deadlines apart from SQLite's own `interrupted`.
    pub fn error(&self, e: &rusqlite::Error) -> DbError {
        if e.sqlite_error_code() == Some(ErrorCode::OperationInterrupted) {
            match (self.stop.load(Ordering::Relaxed), self.limit) {
                (STOP_CANCELLED, _) => return DbError::Cancelled,
                (STOP_DEADLINE, Some(Limit::Deadline(d))) => return DbError::TooSlow(d),
                _ => {}
            }
        }
        sqlite_error(e)
    }

    /// Lists tables and views.
    pub fn objects(&mut self) -> Result<Vec<Object>, DbError> {
        self.limit(Limit::Deadline(Duration::from_millis(500)));
        schema::objects(&self.conn).map_err(|e| self.error(&e))
    }

    /// Columns of a table or view.
    pub fn columns(&mut self, object: &str) -> Result<Vec<ColumnInfo>, DbError> {
        self.limit(Limit::Deadline(Duration::from_millis(500)));
        schema::columns(&self.conn, object).map_err(|e| self.error(&e))
    }
}

fn sqlite_error(e: &rusqlite::Error) -> DbError {
    DbError::Message(e.to_string())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
