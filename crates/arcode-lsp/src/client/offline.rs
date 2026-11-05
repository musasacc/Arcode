//! Steps for particular servers that keep them from going online on their
//! own where their settings alone cannot (the settings are in
//! `arcode-config`'s built-in server table). Servers are named by their
//! `[lsp.<name>]` key.

use serde_json::{Value, json};

/// Whether `initialize` names the root as a workspace folder.
///
/// taplo loads the schema catalog for each workspace folder named in
/// `initialize` with its default settings, before it asks for the client's,
/// so no setting can stop that download. Without folders it serves every
/// file from its default workspace, which gets the client's settings
/// first. It still uses the project's `.taplo.toml`: it is started in the
/// project root and reads that file from there.
pub(crate) fn sends_workspace_folders(server: &str) -> bool {
    server != "taplo"
}

/// Notifications sent right after `initialized`.
///
/// yaml-language-server downloads the schema a file names in a
/// `# yaml-language-server: $schema=<url>` comment unless the client offers
/// to fetch schemas itself (`yaml/registerContentRequest`). Arcode offers
/// and then declines each `vscode/content` request, so such schemas stay
/// unloaded. Only while the schema store is off, as in the built-in
/// settings: settings that turn it on ask for downloads.
pub(crate) fn after_initialized(
    server: &str,
    options: Option<&Value>,
) -> Vec<(&'static str, Value)> {
    let store_off = options
        .and_then(|o| o.pointer("/yaml/schemaStore/enable"))
        .is_some_and(|enable| enable == &Value::Bool(false));
    if server == "yaml-language-server" && store_off {
        // An object, as the server declares a parameter (it is not used).
        vec![("yaml/registerContentRequest", json!({}))]
    } else {
        Vec::new()
    }
}

/// The error for `vscode/content`, a server asking the client for a remote
/// file (a schema): Arcode does not download them. Servers show it after
/// the URL ("Unable to load schema from '<url>': ...").
pub(crate) const CONTENT_REQUEST_ERROR: &str = "Arcode does not download schemas";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taplo_starts_without_workspace_folders() {
        assert!(!sends_workspace_folders("taplo"));
        assert!(sends_workspace_folders("rust-analyzer"));
    }

    #[test]
    fn yaml_routes_schema_downloads_through_the_client_while_the_store_is_off() {
        let off = json!({ "yaml": { "schemaStore": { "enable": false } } });
        let sent = after_initialized("yaml-language-server", Some(&off));
        assert_eq!(sent, [("yaml/registerContentRequest", json!({}))]);
        let on = json!({ "yaml": { "schemaStore": { "enable": true } } });
        assert!(after_initialized("yaml-language-server", Some(&on)).is_empty());
        assert!(after_initialized("yaml-language-server", None).is_empty());
        assert!(after_initialized("taplo", Some(&off)).is_empty());
    }
}
