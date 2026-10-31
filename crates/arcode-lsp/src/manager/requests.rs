//! Request routing: pick the server(s) for a document and forward a
//! request through its actor so it is ordered after earlier edits.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;

use lsp_types::ServerCapabilities;
use lsp_types::request::Request;
use tokio::sync::oneshot;

use super::LspManager;
use super::actor::{Command, ServerSlot};
use super::answer::Answer;
use crate::client::LspClient;
use crate::error::{LspError, Result};
use crate::event::PositionEncoding;

impl LspManager {
    /// Sends a request to the running servers for the document whose
    /// capabilities pass `supports`, in priority order, until one answers
    /// with something other than `null`; see [`LspManager::request_with`].
    pub fn request<R>(
        &self,
        path: &Path,
        params: R::Params,
        supports: impl Fn(&ServerCapabilities) -> bool,
    ) -> impl Future<Output = Result<(String, R::Result)>> + Send + 'static
    where
        R: Request,
        R::Params: Clone,
        R::Result: Answer,
    {
        self.request_with::<R>(path, supports, move |_| params.clone())
    }

    /// A single-answer request (hover, definition, rename, formatting,
    /// signature help, ...): goes to the highest-priority running server
    /// that passes `supports`; when it answers "nothing" (`null`, or an
    /// empty list where that means not found; see [`Answer`]) or fails, the
    /// next one is asked, and so on. Resolves to `(server name, result)`:
    /// the first real answer, else a "nothing" answer if any server gave one,
    /// else the first error ([`LspError::NoServer`] when no server
    /// qualifies).
    ///
    /// `make_params` runs now, once per qualifying server, with that
    /// server's position encoding. A fallback server is only sent its
    /// request after the previous one answered.
    pub fn request_with<R>(
        &self,
        path: &Path,
        supports: impl Fn(&ServerCapabilities) -> bool,
        make_params: impl Fn(PositionEncoding) -> R::Params,
    ) -> impl Future<Output = Result<(String, R::Result)>> + Send + 'static
    where
        R: Request,
        R::Result: Answer,
    {
        let candidates: Vec<(Arc<ServerSlot>, R::Params)> = self
            .running(path)
            .into_iter()
            .filter(|(_, c)| supports(c.capabilities()))
            .map(|(slot, client)| {
                let params = make_params(client.position_encoding());
                (slot, params)
            })
            .collect();
        async move {
            let mut null: Option<(String, R::Result)> = None;
            let mut error: Option<LspError> = None;
            for (slot, params) in candidates {
                let answer = dispatch::<R>(&slot, params)
                    .await
                    .unwrap_or_else(|_| Err(LspError::ServerStopped("request dropped".into())));
                match answer {
                    Ok(result) if !result.is_null() => return Ok((slot.name.clone(), result)),
                    Ok(result) => {
                        null.get_or_insert((slot.name.clone(), result));
                    }
                    Err(e) => {
                        log::debug!("[{}] {} failed: {e}", slot.name, R::METHOD);
                        error.get_or_insert(e);
                    }
                }
            }
            match (null, error) {
                (Some(answer), _) => Ok(answer),
                (None, Some(e)) => Err(e),
                (None, None) => Err(LspError::NoServer),
            }
        }
    }

    /// Sends a request to the running server named `server` for the
    /// document, e.g. `completionItem/resolve` for an item that server
    /// produced. Resolves to [`LspError::NoServer`] when it is not running.
    pub fn request_to<R>(
        &self,
        path: &Path,
        server: &str,
        make_params: impl FnOnce(PositionEncoding) -> R::Params,
    ) -> impl Future<Output = Result<R::Result>> + Send + 'static
    where
        R: Request,
    {
        let pending = self
            .running(path)
            .into_iter()
            .find(|(slot, _)| slot.name == server)
            .map(|(slot, client)| dispatch::<R>(&slot, make_params(client.position_encoding())))
            .ok_or(LspError::NoServer);
        async move {
            pending?
                .await
                .unwrap_or_else(|_| Err(LspError::ServerStopped("request dropped".into())))
        }
    }

    /// Sends a request to every running server for the document that passes
    /// `supports` (e.g. completion, code actions). Failed answers are logged
    /// and left out.
    pub fn request_all<R>(
        &self,
        path: &Path,
        params: R::Params,
        supports: impl Fn(&ServerCapabilities) -> bool,
    ) -> impl Future<Output = Vec<(String, R::Result)>> + Send + 'static
    where
        R: Request,
        R::Params: Clone,
    {
        self.request_all_with::<R>(path, supports, move |_| params.clone())
    }

    /// Like [`LspManager::request_all`], building params per server encoding.
    pub fn request_all_with<R>(
        &self,
        path: &Path,
        supports: impl Fn(&ServerCapabilities) -> bool,
        make_params: impl Fn(PositionEncoding) -> R::Params,
    ) -> impl Future<Output = Vec<(String, R::Result)>> + Send + 'static
    where
        R: Request,
    {
        let pending: Vec<_> = self
            .running(path)
            .into_iter()
            .filter(|(_, c)| supports(c.capabilities()))
            .map(|(slot, client)| {
                let params = make_params(client.position_encoding());
                (slot.name.clone(), dispatch::<R>(&slot, params))
            })
            .collect();
        async move {
            let mut out = Vec::with_capacity(pending.len());
            for (name, rx) in pending {
                match rx.await {
                    Ok(Ok(result)) => out.push((name, result)),
                    Ok(Err(e)) if e.is_cancellation() => {}
                    Ok(Err(e)) => log::debug!("[{name}] {} failed: {e}", R::METHOD),
                    Err(_) => log::debug!("[{name}] {} dropped", R::METHOD),
                }
            }
            out
        }
    }
}

/// Queues a request on a server actor; the answer arrives on the receiver.
/// Dropping the receiver cancels the request on the server.
fn dispatch<R: Request>(
    slot: &ServerSlot,
    params: R::Params,
) -> oneshot::Receiver<Result<R::Result>> {
    let (tx, rx) = oneshot::channel();
    slot.send(Command::Run(Box::new(move |client: &LspClient| {
        let request = client.request::<R>(params);
        tokio::spawn(async move {
            let mut tx = tx;
            let result = tokio::select! {
                r = request => Some(r),
                () = tx.closed() => None,
            };
            if let Some(r) = result {
                let _ = tx.send(r);
            }
        });
    })));
    rx
}
