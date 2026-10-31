//! A scripted language server run as a child process by the manager tests.
//!
//! Behaviour (all reported back through diagnostics so tests can observe it):
//! - `didOpen` / `didChange` / `didSave` publish one diagnostic describing
//!   what was received (`"<name>: changed v2 incremental"`).
//! - `hover` answers `"<name> v<version> <last change kind>"`.
//! - `completion` answers one item labelled with the server name.
//! - `ARCODE_FAKE_HOVER=null` / `error` makes hover answer `null` / fail.
//! - `ARCODE_FAKE_DEFINITION=empty` / `here` offers `definition` and answers
//!   `[]` / a location at line 0, character 7.
//! - `ARCODE_FAKE_INFO=<name>` answers `initialize` with that `serverInfo`.
//! - `workspace/didChangeWatchedFiles` publishes `"watched <type>"` on each
//!   changed file; `rust-analyzer/rebuildProcMacros` publishes `"rebuilt"`
//!   on every open document.

use std::collections::HashMap;

use arcode_lsp::transport::{read_message, write_message};
use serde_json::{Value, json};
use tokio::io::{AsyncWrite, BufReader};

struct Doc {
    version: i64,
    last: String,
}

/// Entry point when the test binary is started as a server.
pub fn run() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(serve());
}

async fn send<W: AsyncWrite + Unpin>(out: &mut W, msg: Value) {
    if write_message(out, &msg.to_string()).await.is_err() {
        std::process::exit(1);
    }
}

async fn publish<W: AsyncWrite + Unpin>(
    out: &mut W,
    name: &str,
    uri: &Value,
    version: i64,
    what: &str,
) {
    let diag = json!({
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
        "severity": 1,
        "message": format!("{name}: {what}"),
    });
    let params = json!({ "uri": uri, "version": version, "diagnostics": [diag] });
    send(
        out,
        json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": params }),
    )
    .await;
}

async fn serve() {
    let name = std::env::var("ARCODE_FAKE_NAME").unwrap_or_else(|_| "fake".into());
    let encoding = std::env::var("ARCODE_FAKE_ENCODING").unwrap_or_else(|_| "utf-16".into());
    let hover_mode = std::env::var("ARCODE_FAKE_HOVER").unwrap_or_else(|_| "normal".into());
    let definition = std::env::var("ARCODE_FAKE_DEFINITION").ok();
    let info = std::env::var("ARCODE_FAKE_INFO").ok();
    eprintln!("{name} starting");
    let mut input = BufReader::new(tokio::io::stdin());
    let mut out = tokio::io::stdout();
    let mut docs: HashMap<String, Doc> = HashMap::new();
    while let Ok(Some(text)) = read_message(&mut input).await {
        let Ok(msg) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let method = msg["method"].as_str().unwrap_or_default().to_string();
        let params = &msg["params"];
        let id = msg.get("id").cloned();
        let uri = params["textDocument"]["uri"].clone();
        let key = uri.as_str().unwrap_or_default().to_string();
        match method.as_str() {
            "initialize" => {
                let mut caps = json!({
                    "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": true } },
                    "hoverProvider": true,
                    "completionProvider": {},
                    "positionEncoding": encoding,
                });
                if definition.is_some() {
                    caps["definitionProvider"] = json!(true);
                }
                let mut result = json!({ "capabilities": caps });
                if let Some(info) = &info {
                    result["serverInfo"] = json!({ "name": info });
                }
                send(
                    &mut out,
                    json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                )
                .await;
            }
            "workspace/didChangeWatchedFiles" => {
                for change in params["changes"].as_array().into_iter().flatten() {
                    let what = format!("watched {}", change["type"]);
                    publish(&mut out, &name, &change["uri"], 0, &what).await;
                }
            }
            "rust-analyzer/rebuildProcMacros" => {
                send(
                    &mut out,
                    json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                )
                .await;
                let uris: Vec<String> = docs.keys().cloned().collect();
                for uri in uris {
                    publish(&mut out, &name, &json!(uri), 0, "rebuilt").await;
                }
            }
            "textDocument/didOpen" => {
                let version = params["textDocument"]["version"].as_i64().unwrap_or(0);
                docs.insert(
                    key,
                    Doc {
                        version,
                        last: "open".into(),
                    },
                );
                publish(
                    &mut out,
                    &name,
                    &uri,
                    version,
                    &format!("opened v{version}"),
                )
                .await;
            }
            "textDocument/didChange" => {
                let version = params["textDocument"]["version"].as_i64().unwrap_or(0);
                let kind = if params["contentChanges"][0].get("range").is_some() {
                    "incremental"
                } else {
                    "full"
                };
                docs.insert(
                    key,
                    Doc {
                        version,
                        last: kind.into(),
                    },
                );
                publish(
                    &mut out,
                    &name,
                    &uri,
                    version,
                    &format!("changed v{version} {kind}"),
                )
                .await;
            }
            "textDocument/didSave" => {
                let version = docs.get(&key).map_or(0, |d| d.version);
                let text = params["text"].as_str().unwrap_or("<none>");
                publish(&mut out, &name, &uri, version, &format!("saved {text}")).await;
            }
            "textDocument/didClose" => {
                docs.remove(&key);
                let params = json!({ "uri": uri, "diagnostics": [] });
                send(&mut out, json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": params })).await;
            }
            "textDocument/hover" if hover_mode != "normal" => {
                let reply = if hover_mode == "null" {
                    json!({ "jsonrpc": "2.0", "id": id, "result": null })
                } else {
                    let error = json!({ "code": -32603, "message": "hover failed" });
                    json!({ "jsonrpc": "2.0", "id": id, "error": error })
                };
                send(&mut out, reply).await;
            }
            "textDocument/hover" => {
                let doc = docs.get(&key);
                let value = match doc {
                    Some(d) => format!("{name} v{} {}", d.version, d.last),
                    None => format!("{name} unknown document"),
                };
                let result = json!({ "contents": { "kind": "plaintext", "value": value } });
                send(
                    &mut out,
                    json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                )
                .await;
            }
            "textDocument/definition" => {
                let result = if definition.as_deref() == Some("empty") {
                    json!([])
                } else {
                    let pos = json!({ "line": 0, "character": 7 });
                    json!([{ "uri": uri, "range": { "start": pos, "end": pos } }])
                };
                send(
                    &mut out,
                    json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                )
                .await;
            }
            "textDocument/completion" => {
                send(
                    &mut out,
                    json!({ "jsonrpc": "2.0", "id": id, "result": [ { "label": name } ] }),
                )
                .await;
            }
            "shutdown" => {
                send(
                    &mut out,
                    json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                )
                .await
            }
            "exit" => std::process::exit(0),
            _ => {
                if let Some(id) = id {
                    let error = json!({ "code": -32601, "message": "unknown" });
                    send(
                        &mut out,
                        json!({ "jsonrpc": "2.0", "id": id, "error": error }),
                    )
                    .await;
                }
            }
        }
    }
}
