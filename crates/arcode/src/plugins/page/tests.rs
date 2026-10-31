//! The Plugins page model: rows from manifest-only plugins (no engine),
//! states, capability wording and the enable summary.

use std::collections::HashSet;
use std::path::Path;

use arcode_plugin_host::grants::{Capability, GrantScope, GrantState};
use arcode_plugin_host::manifest::{Capabilities, NetworkCapability};
use arcode_plugin_host::{PluginId, PluginSource};

use super::rows::{GrantRow, Status, capability_summary, enable_summary, rows};
use crate::plugins::PluginClient;

fn manifest(id: &str, name: &str, caps: &str) -> String {
    format!(
        "[plugin]\nid = \"{id}\"\nname = \"{name}\"\nversion = \"1.2.3\"\napi = \"0.1\"\n\
         entry = \"plugin.wasm\"\ndescription = \"Does {name} things\"\n\n[capabilities]\n{caps}\n"
    )
}

fn write(dir: &Path, text: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("plugin.toml"), text).unwrap();
    std::fs::write(dir.join("plugin.wasm"), b"never read").unwrap();
}

/// A bundled and an installed plugin, manifests only.
fn client(root: &Path) -> PluginClient {
    let bundled = root.join("bundled");
    write(
        &bundled.join("arcode.web"),
        &manifest("arcode.web", "Web", "network = { hosts = [\"*\"] }"),
    );
    let mut paths = crate::plugins::paths::plugin_paths(
        None,
        &root.join("config"),
        &root.join("data"),
        &root.join("cache"),
        None,
    );
    write(
        &paths.installed.join("acme.notes"),
        &manifest("acme.notes", "acme Notes", "workspace = \"read-write\""),
    );
    paths.bundled = vec![bundled];
    let (client, _requests) = PluginClient::new();
    assert!(client.ensure_with(paths, Vec::new(), false));
    client
}

#[test]
fn rows_list_every_plugin_with_its_state() {
    let dir = tempfile::tempdir().unwrap();
    let client = client(dir.path());
    let web = PluginId::parse("arcode.web").unwrap();
    let notes = PluginId::parse("acme.notes").unwrap();
    let list = rows(&client.manager().unwrap(), &HashSet::new());
    let names: Vec<&str> = list.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["acme Notes", "Web"], "sorted by name, any case");
    let (n, w) = (&list[0], &list[1]);
    assert_eq!(n.source, PluginSource::Installed);
    assert_eq!(n.status, Status::Disabled, "installed plugins start off");
    assert!(!n.enabled && n.can_uninstall());
    assert_eq!(w.status, Status::Inactive);
    assert!(w.enabled && !w.can_uninstall());
    assert_eq!(w.version, "1.2.3");
    assert_eq!(w.description.as_deref(), Some("Does Web things"));
    assert!(w.grants.is_empty());
    assert!(!client.engine_created(), "reading rows starts nothing");

    let crashed = HashSet::from([web.clone()]);
    let list = rows(&client.manager().unwrap(), &crashed);
    assert_eq!(list[1].status, Status::Crashed);

    client.set_enabled(&web, false);
    client.set_enabled(&notes, true);
    let list = rows(&client.manager().unwrap(), &HashSet::new());
    assert_eq!(list[1].status, Status::Disabled);
    // Enabling approves the current plugin.wasm.
    assert_eq!(list[0].status, Status::Inactive);
    assert!(client.contributions().commands.is_empty());
}

#[test]
fn capabilities_are_described_in_host_words() {
    let none = capability_summary(&Capabilities::default());
    assert_eq!(none, ["No access to files, documents or the network"]);
    let caps = Capabilities {
        network: Some(NetworkCapability {
            hosts: vec!["api.example.com".into(), "*.example.org".into()],
            background: true,
        }),
        editor_write: true,
        ..Capabilities::default()
    };
    let lines = capability_summary(&caps);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[1].contains("api.example.com, *.example.org"));
    assert!(lines[2].contains("background"));
    let any = Capabilities {
        network: Some(NetworkCapability {
            hosts: vec!["*".into()],
            background: false,
        }),
        ..Capabilities::default()
    };
    assert!(capability_summary(&any)[0].contains("any host"));
}

#[test]
fn grants_and_the_enable_summary_read_plainly() {
    let grant = GrantRow {
        capability: Capability::Network {
            host: "example.com".into(),
        },
        state: GrantState::Allow,
        scope: GrantScope::Global,
    };
    assert_eq!(grant.label(), "Allowed: connect to example.com (always)");
    let deny = GrantRow {
        capability: Capability::WorkspaceRead,
        state: GrantState::Deny,
        scope: GrantScope::Project,
    };
    assert_eq!(deny.label(), "Denied: read project files (this project)");

    let dir = tempfile::tempdir().unwrap();
    let client = client(dir.path());
    let list = rows(&client.manager().unwrap(), &HashSet::new());
    let text = enable_summary(&list[0]);
    assert!(
        text.starts_with("\u{201c}acme Notes\u{201d} 1.2.3 (acme.notes)"),
        "{text}"
    );
    assert!(text.contains("Read and change project files"), "{text}");
}
