//! Sending breakpoints, and the bookkeeping that keeps a handle call made
//! before configuration from sending the same set a second time.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use super::core::Core;
use super::{REQUEST_TIMEOUT, parse_body};
use crate::connection::Connection;
use crate::error::Result;
use crate::event::DebugEvent;
use crate::types::{
    Breakpoint, FunctionBreakpoint, SetBreakpointsArguments, SetBreakpointsResponseBody,
    SetFunctionBreakpointsArguments, Source, SourceBreakpoint,
};

/// A file's source breakpoints, or `None` for the function breakpoints.
pub(crate) type Key = Option<PathBuf>;

/// Versions of the breakpoint sets the user stored, and which version the
/// configuration step sent (with the adapter's answer).
#[derive(Default)]
pub(crate) struct Book {
    counter: u64,
    latest: HashMap<Key, u64>,
    sent: HashMap<Key, (u64, Result<Vec<Breakpoint>>)>,
}

impl Book {
    /// Records that the set for `key` changed; returns its new version.
    pub(crate) fn bump(&mut self, key: Key) -> u64 {
        self.counter += 1;
        self.latest.insert(key, self.counter);
        self.counter
    }

    /// Version of the stored set for `key` (0: from the initial config).
    pub(crate) fn latest(&self, key: &Key) -> u64 {
        self.latest.get(key).copied().unwrap_or(0)
    }

    /// Records the answer to the configuration step's request for `key`.
    pub(crate) fn record(&mut self, key: Key, version: u64, result: Result<Vec<Breakpoint>>) {
        self.sent.insert(key, (version, result));
    }

    /// The answer, if configuration already sent `version` (or newer).
    pub(crate) fn sent_since(&self, key: &Key, version: u64) -> Option<Result<Vec<Breakpoint>>> {
        self.sent
            .get(key)
            .filter(|(sent, _)| *sent >= version)
            .map(|(_, result)| result.clone())
    }

    /// Forgets what was sent (a new adapter starts from nothing).
    pub(crate) fn reset_sent(&mut self) {
        self.sent.clear();
    }
}

/// Enqueues `setBreakpoints` for one file now; the future reports the
/// result as [`DebugEvent::BreakpointsSet`] and returns it.
pub(crate) fn send(
    core: Arc<Core>,
    generation: u64,
    conn: &Arc<Connection>,
    path: PathBuf,
    breakpoints: Vec<SourceBreakpoint>,
) -> impl Future<Output = Result<Vec<Breakpoint>>> + Send + use<> {
    let args = SetBreakpointsArguments {
        source: Source {
            name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            path: Some(path.to_string_lossy().into_owned()),
            source_reference: None,
        },
        breakpoints,
    };
    let request = serde_json::to_value(args)
        .map(|a| conn.request("setBreakpoints", Some(a), Some(REQUEST_TIMEOUT)));
    async move {
        let body: SetBreakpointsResponseBody = parse_body(request?.await?)?;
        if core.is_current(generation) {
            core.events.send(DebugEvent::BreakpointsSet {
                path,
                breakpoints: body.breakpoints.clone(),
            });
        }
        Ok(body.breakpoints)
    }
}

/// Enqueues `setFunctionBreakpoints` now.
pub(crate) fn send_functions(
    conn: &Arc<Connection>,
    breakpoints: Vec<FunctionBreakpoint>,
) -> impl Future<Output = Result<Vec<Breakpoint>>> + Send + use<> {
    let request = serde_json::to_value(SetFunctionBreakpointsArguments { breakpoints })
        .map(|a| conn.request("setFunctionBreakpoints", Some(a), Some(REQUEST_TIMEOUT)));
    async move { parse_body::<SetBreakpointsResponseBody>(request?.await?).map(|b| b.breakpoints) }
}

/// Sends the stored breakpoints during configuration (the newest set of
/// each file at the moment it is sent) and records the answers.
pub(crate) async fn configure(core: &Arc<Core>, generation: u64, conn: &Arc<Connection>) {
    let paths: Vec<PathBuf> = core.lock().config.breakpoints.keys().cloned().collect();
    for path in paths {
        let key = Some(path.clone());
        let next = {
            let data = core.lock();
            let bps = data.config.breakpoints.get(&path).cloned();
            bps.map(|b| (b, data.book.latest(&key)))
        };
        // Removed meanwhile: the handle call that removed it sends that.
        let Some((bps, version)) = next else { continue };
        let result = send(core.clone(), generation, conn, path.clone(), bps).await;
        if let Err(e) = &result {
            log::warn!("setBreakpoints for {} failed: {e}", path.display());
        }
        core.lock().book.record(key, version, result);
    }
    let (functions, version, supported) = {
        let data = core.lock();
        let caps = data.capabilities.as_ref();
        (
            data.config.function_breakpoints.clone(),
            data.book.latest(&None),
            caps.is_some_and(|c| c.supports_function_breakpoints),
        )
    };
    if supported && !functions.is_empty() {
        let result = send_functions(conn, functions).await;
        if let Err(e) = &result {
            log::warn!("setFunctionBreakpoints failed: {e}");
        }
        core.lock().book.record(None, version, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DapError;

    #[test]
    fn book_answers_only_for_versions_already_sent() {
        let mut book = Book::default();
        let key = Some(PathBuf::from("/a.rs"));
        assert_eq!(book.latest(&key), 0);
        let v1 = book.bump(key.clone());
        let v2 = book.bump(key.clone());
        assert!(v2 > v1);
        assert_eq!(book.latest(&key), v2);
        assert!(book.sent_since(&key, v1).is_none());
        book.record(key.clone(), v1, Ok(vec![]));
        assert_eq!(book.sent_since(&key, v1), Some(Ok(vec![])));
        assert!(book.sent_since(&key, v2).is_none());
        let failed = Err(DapError::Io("x".into()));
        book.record(key.clone(), v2, failed.clone());
        assert_eq!(book.sent_since(&key, v1), Some(failed));
        assert!(book.sent_since(&None, 1).is_none());
        book.reset_sent();
        assert!(book.sent_since(&key, v1).is_none());
        assert_eq!(book.latest(&key), v2);
    }
}
