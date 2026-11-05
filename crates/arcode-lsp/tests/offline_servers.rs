//! Servers that would download on their own are started so they do not.

mod common;

use common::connect_named;
use serde_json::json;

#[tokio::test]
async fn taplo_starts_without_workspace_folders() {
    let options = json!({ "evenBetterToml": { "schema": { "enabled": false } } });
    let (_client, mut peer, _events, init, _) =
        connect_named("taplo", json!({}), Some(options)).await;
    // taplo would load its schema catalog for each folder named here,
    // before it asks for the settings.
    assert!(init["params"]["workspaceFolders"].is_null());
    assert!(init["params"]["rootUri"].is_string());
    // It gets its settings from workspace/configuration instead.
    peer.send(json!({
        "jsonrpc": "2.0", "id": 1, "method": "workspace/configuration",
        "params": { "items": [ { "section": "evenBetterToml" } ] }
    }))
    .await;
    let reply = peer.recv().await;
    assert_eq!(reply["result"], json!([{ "schema": { "enabled": false } }]));
}

#[tokio::test]
async fn yaml_schema_downloads_are_declined() {
    let options = json!({ "yaml": { "schemaStore": { "enable": false } } });
    let (_client, mut peer, _events, init, _) =
        connect_named("yaml-language-server", json!({}), Some(options)).await;
    assert!(init["params"]["workspaceFolders"].is_array());
    // Right after `initialized`: schemas go through the client from now on.
    let register = peer.expect("yaml/registerContentRequest").await;
    assert_eq!(register["params"], json!({}));
    let url = "https://json.schemastore.org/github-workflow.json";
    peer.send(json!({
        "jsonrpc": "2.0", "id": 7, "method": "vscode/content", "params": [url]
    }))
    .await;
    let reply = peer.recv().await;
    assert_eq!(reply["id"], 7);
    assert!(reply["result"].is_null());
    assert_eq!(reply["error"]["code"], arcode_lsp::REQUEST_FAILED);
    assert_eq!(
        reply["error"]["message"],
        "Arcode does not download schemas"
    );
}

#[tokio::test]
async fn other_servers_keep_workspace_folders_and_get_no_extra_messages() {
    let (_client, mut peer, _events, init, _) =
        connect_named("yaml-language-server", json!({}), None).await;
    assert!(init["params"]["workspaceFolders"].is_array());
    // With the schema store on (no settings), nothing else follows: the
    // next message is the answer to this request.
    peer.send(json!({
        "jsonrpc": "2.0", "id": 2, "method": "workspace/workspaceFolders", "params": null
    }))
    .await;
    let reply = peer.recv().await;
    assert_eq!(reply["id"], 2);
}
