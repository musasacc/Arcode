//! Reader and writer tasks: route responses to pending requests, answer
//! server-to-client requests and turn notifications into events.

use std::collections::HashMap;
use std::sync::Arc;

use lsp_types::{
    ApplyWorkspaceEditParams, ConfigurationParams, LogMessageParams, MessageType, NumberOrString,
    ProgressParams, ProgressParamsValue, PublishDiagnosticsParams, ShowMessageParams,
    WorkDoneProgress,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncWrite};
use tokio::sync::mpsc;

use super::shared::Shared;
use crate::error::{INVALID_PARAMS, METHOD_NOT_FOUND};
use crate::event::{LspEvent, ServerStatus};
use crate::rpc::{self, Incoming};
use crate::transport;
use crate::uri::{path_to_uri, uri_to_path};

/// Reads messages until EOF, an error, or termination.
pub(crate) async fn read_loop<R>(shared: Arc<Shared>, mut reader: R)
where
    R: AsyncBufRead + Unpin + Send,
{
    let mut progress = ProgressTracker::default();
    let terminated = shared.terminated();
    tokio::pin!(terminated);
    loop {
        let next = tokio::select! {
            next = transport::read_message(&mut reader) => next,
            () = &mut terminated => return,
        };
        match next {
            Ok(Some(text)) => handle_message(&shared, &mut progress, &text),
            Ok(None) => {
                shared.terminate("server closed the connection");
                return;
            }
            Err(e) => {
                shared.terminate(format!("read error: {e}"));
                return;
            }
        }
    }
}

/// Writes queued messages until the queue closes or the connection ends.
pub(crate) async fn write_loop<W>(
    shared: Arc<Shared>,
    mut rx: mpsc::UnboundedReceiver<String>,
    mut writer: W,
) where
    W: AsyncWrite + Unpin + Send,
{
    let terminated = shared.terminated();
    tokio::pin!(terminated);
    loop {
        let message = tokio::select! {
            biased;
            m = rx.recv() => m,
            () = &mut terminated => None,
        };
        let Some(message) = message else { return };
        if let Err(e) = transport::write_message(&mut writer, &message).await {
            shared.terminate(format!("write error: {e}"));
            return;
        }
    }
}

fn handle_message(shared: &Shared, progress: &mut ProgressTracker, text: &str) {
    match rpc::parse_incoming(text) {
        Ok(Incoming::Response { id, result }) => match rpc::numeric_id(&id) {
            Some(id) => shared.complete(id, result),
            None => log::debug!("[{}] response with foreign id {id}", shared.name),
        },
        Ok(Incoming::Request { id, method, params }) => {
            let reply = match handle_request(shared, &method, params) {
                Ok(result) => rpc::response_ok(&id, result),
                Err((code, message)) => rpc::response_err(&id, code, &message),
            };
            if let Err(e) = shared.send_raw(reply) {
                log::debug!("[{}] could not answer {method}: {e}", shared.name);
            }
        }
        Ok(Incoming::Notification { method, params }) => {
            handle_notification(shared, progress, &method, params)
        }
        Err(e) => shared.log(
            MessageType::WARNING,
            format!("ignoring invalid message: {e}"),
        ),
    }
}

type ReplyResult = Result<Value, (i64, String)>;

fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, (i64, String)> {
    serde_json::from_value(params).map_err(|e| (INVALID_PARAMS, e.to_string()))
}

/// Answers a request sent by the server.
fn handle_request(shared: &Shared, method: &str, params: Value) -> ReplyResult {
    match method {
        "workspace/configuration" => {
            let params: ConfigurationParams = parse(params)?;
            let options = shared.initialization_options.as_ref();
            let items = params
                .items
                .iter()
                .map(|item| config_value(options, item.section.as_deref()))
                .collect();
            Ok(Value::Array(items))
        }
        "window/workDoneProgress/create"
        | "client/registerCapability"
        | "client/unregisterCapability" => Ok(Value::Null),
        m if m.starts_with("workspace/") && m.ends_with("/refresh") => Ok(Value::Null),
        "workspace/workspaceFolders" => {
            let name = shared
                .root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok(json!([{ "uri": path_to_uri(&shared.root), "name": name }]))
        }
        "workspace/applyEdit" => {
            let params: ApplyWorkspaceEditParams = parse(params)?;
            shared.events.send(LspEvent::ApplyEdit {
                server: shared.name.clone(),
                edit: params.edit,
            });
            Ok(
                json!({ "applied": false, "failureReason": "Arcode does not apply server edits yet" }),
            )
        }
        "window/showMessageRequest" => {
            let params: ShowMessageParams = parse(params)?;
            shared.events.send(LspEvent::ShowMessage {
                server: shared.name.clone(),
                typ: params.typ,
                message: params.message,
            });
            Ok(Value::Null)
        }
        _ => Err((METHOD_NOT_FOUND, format!("unhandled method {method}"))),
    }
}

/// Looks up a configuration section in the initialization options.
///
/// `None` returns the whole object. A dotted section is looked up as a path;
/// if that fails, the first segment is assumed to name the options object
/// itself (e.g. `rust-analyzer.cargo` finds `cargo` in rust-analyzer's
/// options). Anything not found is `null`.
pub(crate) fn config_value(options: Option<&Value>, section: Option<&str>) -> Value {
    let Some(options) = options else {
        return Value::Null;
    };
    let Some(section) = section.filter(|s| !s.is_empty()) else {
        return options.clone();
    };
    let lookup = |path: &str| {
        if path.is_empty() {
            return Some(options);
        }
        options.get(path).or_else(|| {
            path.split('.')
                .try_fold(options, |value, key| value.get(key))
        })
    };
    if let Some(v) = lookup(section) {
        return v.clone();
    }
    let rest = section.split_once('.').map_or("", |(_, rest)| rest);
    lookup(rest).cloned().unwrap_or(Value::Null)
}

fn handle_notification(
    shared: &Shared,
    progress: &mut ProgressTracker,
    method: &str,
    params: Value,
) {
    let server = shared.name.clone();
    match method {
        "textDocument/publishDiagnostics" => {
            let Ok(p) = serde_json::from_value::<PublishDiagnosticsParams>(params) else {
                return shared.log(MessageType::WARNING, "invalid publishDiagnostics".into());
            };
            match uri_to_path(&p.uri) {
                Some(path) => shared.events.send(LspEvent::Diagnostics {
                    server,
                    path,
                    version: p.version,
                    diagnostics: p.diagnostics,
                }),
                None => log::debug!("[{server}] diagnostics for non-file URI {}", p.uri.as_str()),
            }
        }
        "window/logMessage" => {
            if let Ok(p) = serde_json::from_value::<LogMessageParams>(params) {
                shared.log(p.typ, p.message);
            }
        }
        "window/showMessage" => {
            if let Ok(p) = serde_json::from_value::<ShowMessageParams>(params) {
                shared.events.send(LspEvent::ShowMessage {
                    server,
                    typ: p.typ,
                    message: p.message,
                });
            }
        }
        "$/progress" => {
            if let Ok(p) = serde_json::from_value::<ProgressParams>(params) {
                progress.handle(shared, p);
            }
        }
        _ => log::trace!("[{server}] ignoring notification {method}"),
    }
}

/// Tracks active work-done progress tokens to derive `Busy`/`Running`.
#[derive(Default)]
struct ProgressTracker {
    titles: HashMap<String, String>,
}

impl ProgressTracker {
    fn handle(&mut self, shared: &Shared, params: ProgressParams) {
        let token = match params.token {
            NumberOrString::Number(n) => n.to_string(),
            NumberOrString::String(s) => s,
        };
        let ProgressParamsValue::WorkDone(work) = params.value;
        let was_idle = self.titles.is_empty();
        let (title, message, percentage, done) = match work {
            WorkDoneProgress::Begin(b) => {
                self.titles.insert(token.clone(), b.title.clone());
                (Some(b.title), b.message, b.percentage, false)
            }
            WorkDoneProgress::Report(r) => (
                self.titles.get(&token).cloned(),
                r.message,
                r.percentage,
                false,
            ),
            WorkDoneProgress::End(e) => (self.titles.remove(&token), e.message, None, true),
        };
        shared.events.send(LspEvent::Progress {
            server: shared.name.clone(),
            token,
            title: title.clone(),
            message,
            percentage,
            done,
        });
        if was_idle && !done {
            if let Some(title) = title {
                shared.emit_status(ServerStatus::Busy(title));
            }
        } else if done && !was_idle {
            match self.titles.values().next() {
                None => shared.emit_status(ServerStatus::Running),
                Some(other) => shared.emit_status(ServerStatus::Busy(other.clone())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_lookup() {
        let opts = json!({ "cargo": { "features": "all" }, "a.b": 1, "x": { "y": { "z": 2 } } });
        assert_eq!(config_value(None, Some("x")), Value::Null);
        assert_eq!(config_value(Some(&opts), None), opts);
        assert_eq!(config_value(Some(&opts), Some("a.b")), json!(1));
        assert_eq!(config_value(Some(&opts), Some("x.y.z")), json!(2));
        assert_eq!(config_value(Some(&opts), Some("rust-analyzer")), opts);
        assert_eq!(
            config_value(Some(&opts), Some("rust-analyzer.cargo")),
            json!({ "features": "all" })
        );
        assert_eq!(config_value(Some(&opts), Some("nope.missing")), Value::Null);
    }
}
