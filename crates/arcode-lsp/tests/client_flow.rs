//! End-to-end client behaviour against an in-process fake server.

mod common;

use arcode_lsp::lsp_types::request::HoverRequest;
use arcode_lsp::lsp_types::{
    HoverContents, HoverParams, MarkupContent, MarkupKind, MessageType, Position, Range,
    TextDocumentIdentifier, TextDocumentPositionParams,
};
use arcode_lsp::{LspEvent, PositionEncoding, ServerStatus, TextChange, path_to_uri};
use common::{connect, next_event, root};
use serde_json::{Value, json};

fn incremental_caps() -> Value {
    json!({ "textDocumentSync": 2, "hoverProvider": true, "positionEncoding": "utf-16" })
}

#[tokio::test]
async fn full_session_with_fake_server() {
    let options = json!({ "cargo": { "features": "all" }, "check": { "command": "clippy" } });
    let (client, mut peer, events, init) = connect(incremental_caps(), Some(options.clone())).await;

    // initialize carried our identity, capabilities and options.
    let params = &init["params"];
    assert_eq!(params["clientInfo"]["name"], "Arcode");
    assert_eq!(
        params["capabilities"]["general"]["positionEncodings"],
        json!(["utf-8", "utf-16"])
    );
    assert_eq!(params["capabilities"]["window"]["workDoneProgress"], true);
    assert_eq!(params["initializationOptions"], options);
    assert_eq!(params["rootUri"], json!(path_to_uri(&root())));
    assert_eq!(params["workspaceFolders"][0]["uri"], params["rootUri"]);
    assert!(params["processId"].is_u64());

    assert_eq!(client.position_encoding(), PositionEncoding::Utf16);
    assert!(client.capabilities().hover_provider.is_some());
    assert_eq!(client.server_info().map(|i| i.name.as_str()), Some("fake"));
    next_event(&events, |e| {
        matches!(
            e,
            LspEvent::ServerStatus {
                status: ServerStatus::Running,
                ..
            }
        )
    })
    .await;

    // didOpen is forwarded; the server answers with diagnostics.
    let file = root().join("src/main file.rs");
    let uri = path_to_uri(&file);
    client
        .did_open(uri.clone(), "rust", 1, "fn main() {}".into())
        .unwrap();
    let open = peer.expect("textDocument/didOpen").await;
    assert_eq!(open["params"]["textDocument"]["languageId"], "rust");
    assert_eq!(open["params"]["textDocument"]["text"], "fn main() {}");
    peer.notify(
        "textDocument/publishDiagnostics",
        json!({
            "uri": uri, "version": 1,
            "diagnostics": [{ "range": { "start": {"line":0,"character":3}, "end": {"line":0,"character":7} },
                              "severity": 1, "message": "boom" }]
        }),
    )
    .await;
    match next_event(&events, |e| matches!(e, LspEvent::Diagnostics { .. })).await {
        LspEvent::Diagnostics {
            server,
            path,
            version,
            diagnostics,
        } => {
            assert_eq!(server, "fake");
            assert_eq!(path, file);
            assert_eq!(version, Some(1));
            assert_eq!(diagnostics[0].message, "boom");
        }
        _ => unreachable!(),
    }

    // Incremental sync sends ranges, not the full text.
    let range = Range::new(Position::new(0, 3), Position::new(0, 7));
    client
        .did_change(
            uri.clone(),
            2,
            vec![TextChange {
                range: Some(range),
                text: "start".into(),
            }],
            || panic!("full text must not be built for incremental servers"),
        )
        .unwrap();
    let change = peer.expect("textDocument/didChange").await;
    assert_eq!(change["params"]["textDocument"]["version"], 2);
    assert_eq!(
        change["params"]["contentChanges"][0]["range"]["start"]["character"],
        3
    );
    assert_eq!(change["params"]["contentChanges"][0]["text"], "start");

    // Server -> client workspace/configuration is answered from the options.
    peer.send(json!({
        "jsonrpc": "2.0", "id": "cfg-1", "method": "workspace/configuration",
        "params": { "items": [ { "section": "rust-analyzer.cargo" }, { "section": "unknown.key" }, {} ] }
    }))
    .await;
    let reply = peer.recv().await;
    assert_eq!(reply["id"], "cfg-1");
    assert_eq!(
        reply["result"],
        json!([{ "features": "all" }, null, options])
    );

    // A hover request round-trips.
    let hover = client.request::<HoverRequest>(HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            position: Position::new(0, 4),
        },
        work_done_progress_params: Default::default(),
    });
    let server = async {
        let req = peer.expect("textDocument/hover").await;
        assert_eq!(req["params"]["position"]["character"], 4);
        peer.respond(
            &req["id"],
            json!({ "contents": { "kind": "markdown", "value": "**main**" } }),
        )
        .await;
    };
    let (hover, ()) = tokio::join!(hover, server);
    let hover = hover.unwrap().expect("hover result");
    assert_eq!(
        hover.contents,
        HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: "**main**".into()
        })
    );

    // Unknown server requests get MethodNotFound; known ones get null.
    peer.send(json!({ "jsonrpc": "2.0", "id": 7, "method": "custom/thing", "params": {} }))
        .await;
    let reply = peer.recv().await;
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["error"]["code"], -32601);
    for (id, method) in [
        (8, "window/workDoneProgress/create"),
        (9, "client/registerCapability"),
        (10, "client/unregisterCapability"),
        (11, "workspace/inlayHint/refresh"),
    ] {
        peer.send(
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": { "token": "t" } }),
        )
        .await;
        let reply = peer.recv().await;
        assert_eq!(reply["id"], id);
        assert_eq!(reply["result"], Value::Null, "{method}: {reply}");
    }

    // Progress becomes Progress events and Busy/Running status.
    let progress = |kind: Value| json!({ "token": "idx", "value": kind });
    peer.notify(
        "$/progress",
        progress(json!({ "kind": "begin", "title": "Indexing", "percentage": 0 })),
    )
    .await;
    peer.notify(
        "$/progress",
        progress(json!({ "kind": "report", "message": "3/10", "percentage": 30 })),
    )
    .await;
    peer.notify("$/progress", progress(json!({ "kind": "end" })))
        .await;
    let busy = next_event(&events, |e| {
        matches!(
            e,
            LspEvent::ServerStatus {
                status: ServerStatus::Busy(_),
                ..
            }
        )
    })
    .await;
    assert!(
        matches!(busy, LspEvent::ServerStatus { status: ServerStatus::Busy(ref t), .. } if t == "Indexing")
    );
    let report = next_event(&events, |e| {
        matches!(
            e,
            LspEvent::Progress {
                percentage: Some(30),
                ..
            }
        )
    })
    .await;
    assert!(
        matches!(report, LspEvent::Progress { title: Some(ref t), done: false, .. } if t == "Indexing")
    );
    next_event(&events, |e| {
        matches!(e, LspEvent::Progress { done: true, .. })
    })
    .await;
    next_event(&events, |e| {
        matches!(
            e,
            LspEvent::ServerStatus {
                status: ServerStatus::Running,
                ..
            }
        )
    })
    .await;

    // Messages and edits are forwarded.
    peer.notify(
        "window/logMessage",
        json!({ "type": 3, "message": "hello log" }),
    )
    .await;
    peer.notify(
        "window/showMessage",
        json!({ "type": 1, "message": "hello user" }),
    )
    .await;
    let log = next_event(&events, |e| matches!(e, LspEvent::Log { .. })).await;
    assert!(
        matches!(log, LspEvent::Log { level: MessageType::INFO, ref message, .. } if message == "hello log")
    );
    let show = next_event(&events, |e| matches!(e, LspEvent::ShowMessage { .. })).await;
    assert!(matches!(
        show,
        LspEvent::ShowMessage {
            typ: MessageType::ERROR,
            ..
        }
    ));
    peer.send(json!({
        "jsonrpc": "2.0", "id": 12, "method": "workspace/applyEdit",
        "params": { "edit": { "changes": { uri.as_str(): [ { "range": range, "newText": "x" } ] } } }
    }))
    .await;
    let reply = peer.recv().await;
    assert_eq!(reply["result"]["applied"], false);
    let edit = next_event(&events, |e| matches!(e, LspEvent::ApplyEdit { .. })).await;
    match edit {
        LspEvent::ApplyEdit { edit, .. } => assert!(edit.changes.unwrap().contains_key(&uri)),
        _ => unreachable!(),
    }

    // didSave / didClose.
    client
        .did_save(uri.clone(), || {
            unreachable!("bare kind sync saves without text")
        })
        .unwrap();
    let save = peer.expect("textDocument/didSave").await;
    assert!(save["params"].get("text").is_none());
    client.did_close(uri.clone()).unwrap();
    peer.expect("textDocument/didClose").await;

    // Graceful shutdown.
    let shutdown = client.shutdown();
    let server = async {
        let req = peer.expect("shutdown").await;
        assert!(req.get("params").is_none());
        peer.respond(&req["id"], Value::Null).await;
        peer.expect("exit").await;
        drop(peer);
    };
    let (result, ()) = tokio::join!(shutdown, server);
    result.unwrap();
    next_event(&events, |e| {
        matches!(
            e,
            LspEvent::ServerStatus {
                status: ServerStatus::Stopped,
                ..
            }
        )
    })
    .await;
    assert!(!client.is_alive());
}

#[tokio::test]
async fn full_sync_server_gets_whole_text_and_utf8_is_negotiated() {
    let caps = json!({ "textDocumentSync": { "openClose": true, "change": 1, "save": { "includeText": true } },
                       "positionEncoding": "utf-8" });
    let (client, mut peer, _events, _) = connect(caps, None).await;
    assert_eq!(client.position_encoding(), PositionEncoding::Utf8);
    let uri = path_to_uri(&root().join("a.txt"));
    client
        .did_change(
            uri.clone(),
            3,
            vec![TextChange {
                range: None,
                text: "ignored".into(),
            }],
            || "whole".into(),
        )
        .unwrap();
    let change = peer.expect("textDocument/didChange").await;
    assert_eq!(
        change["params"]["contentChanges"],
        json!([{ "text": "whole" }])
    );
    client.did_save(uri, || "saved text".into()).unwrap();
    let save = peer.expect("textDocument/didSave").await;
    assert_eq!(save["params"]["text"], "saved text");
}
