//! Request routing: pick the server(s) for a document and forward a
//! request through its actor so it is ordered after earlier edits.

use std::future::Future;
use std::path::Path;

use lsp_types::ServerCapabilities;
use lsp_types::request::Request;
use tokio::sync::oneshot;

use super::LspManager;
use super::actor::{Command, ServerSlot};
use crate::client::LspClient;
use crate::error::{LspError, Result};
use crate::event::PositionEncoding;

impl LspManager {
    /// Sends a request to the first running server (in priority order) for
    /// the document whose capabilities pass `supports`. Resolves to
    /// `(server name, result)`, or [`LspError::NoServer`].
    pub fn request<R>(
        &self,
        path: &Path,
        params: R::Params,
        supports: impl Fn(&ServerCapabilities) -> bool,
    ) -> impl Future<Output = Result<(String, R::Result)>> + Send + 'static
    where
        R: Request,
    {
        self.request_with::<R>(path, supports, move |_| params)
    }

    /// Like [`LspManager::request`], but builds the params once the server
    /// (and so its position encoding) is known.
    pub fn request_with<R>(
        &self,
        path: &Path,
        supports: impl Fn(&ServerCapabilities) -> bool,
        make_params: impl FnOnce(PositionEncoding) -> R::Params,
    ) -> impl Future<Output = Result<(String, R::Result)>> + Send + 'static
    where
        R: Request,
    {
        let target = self
            .running(path)
            .into_iter()
            .find(|(_, c)| supports(c.capabilities()));
        let pending = target
            .map(|(slot, client)| {
                let params = make_params(client.position_encoding());
                (slot.name.clone(), dispatch::<R>(&slot, params))
            })
            .ok_or(LspError::NoServer);
        async move {
            let (name, rx) = pending?;
            let result = rx
                .await
                .unwrap_or_else(|_| Err(LspError::ServerStopped("request dropped".into())))?;
            Ok((name, result))
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
