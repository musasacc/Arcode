//! Review tests (plugins part 2: Plugins view). Each failing test
//! reproduces a defect found in review.

use std::fs;
use std::path::{Path, PathBuf};

use arcode_plugin_host::PluginId;
use arcode_plugin_host::grants::Capability;
use arcode_plugin_host::paths::PluginPaths;

use super::client::PluginClient;

fn manifest(id: &str) -> String {
    format!(
        "[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\napi = \"0.1\"\n\
         entry = \"plugin.wasm\"\n\n[capabilities]\nworkspace = \"read\"\n\
         network = {{ hosts = [\"*\"] }}\n"
    )
}

fn plugin_dir(root: &Path, id: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join("plugin.toml"), manifest(id)).expect("write");
    fs::write(dir.join("plugin.wasm"), b"not wasm").expect("write");
}

fn paths(base: &Path) -> PluginPaths {
    let mut paths = super::paths::plugin_paths(
        None,
        &base.join("config"),
        &base.join("data"),
        &base.join("cache"),
        None,
    );
    let bundled = base.join("bundled");
    plugin_dir(&bundled, "acme.tools");
    plugin_dir(&bundled, "acme.other");
    paths.bundled = vec![bundled];
    paths
}

/// One window's plugin client (every workspace window has its own
/// manager, grant store and `plugins.toml` state).
fn window(paths: &PluginPaths, project: Option<&Path>) -> PluginClient {
    let (client, _requests) = PluginClient::new();
    assert!(client.ensure_with(paths.clone(), Vec::new(), true));
    if let Some(root) = project {
        client.set_project(Some(root.to_path_buf()), Some("p".into()));
    }
    client
}

fn id(s: &str) -> PluginId {
    PluginId::parse(s).expect("id")
}

fn net(host: &str) -> Capability {
    Capability::Network { host: host.into() }
}

fn grant_lines(entries: &[(&str, &str, Option<&str>)]) -> String {
    let mut text = String::new();
    for (plugin, capability, host) in entries {
        text.push_str(&format!(
            "[[grant]]\nplugin = \"{plugin}\"\ncapability = \"{capability}\"\n"
        ));
        if let Some(host) = host {
            text.push_str(&format!("host = \"{host}\"\n"));
        }
        text.push_str("state = \"allow\"\n");
    }
    text
}

/// Two windows are open. Revoking an "Always" network grant in window A's
/// Plugins view leaves window B allowing that host, and B's next grant
/// change writes the revoked grant back to `plugin-grants.toml`, so it is
/// allowed again in every later session.
#[test]
fn review_revoke_in_one_window_is_undone_by_another() {
    let base = tempfile::tempdir().expect("tempdir");
    let paths = paths(base.path());
    fs::create_dir_all(base.path().join("config")).expect("mkdir");
    fs::write(
        &paths.global_grants,
        grant_lines(&[
            ("acme.tools", "network", Some("a.test")),
            ("acme.tools", "network", Some("b.test")),
        ]),
    )
    .expect("write");
    let a = window(&paths, None);
    let b = window(&paths, None);
    let tools = id("acme.tools");
    assert!(a.manager().expect("loaded").revoke(&tools, &net("a.test")));
    let b_still_allows = b
        .manager()
        .expect("loaded")
        .grants(&tools)
        .iter()
        .any(|(c, _, _)| *c == net("a.test"));
    // Window B changes another grant (here: revokes b.test).
    assert!(b.manager().expect("loaded").revoke(&tools, &net("b.test")));
    let later = window(&paths, None);
    let resurrected = later
        .manager()
        .expect("loaded")
        .grants(&tools)
        .iter()
        .any(|(c, _, _)| *c == net("a.test"));
    eprintln!("b_still_allows={b_still_allows} resurrected={resurrected}");
    assert!(!b_still_allows, "window B still allows the revoked host");
    assert!(!resurrected, "the revoked grant is back on disk");
}

/// Two windows are open. Disabling a plugin in window A is lost as soon as
/// window B enables or disables any plugin: B rewrites `plugins.toml` from
/// its own copy, and the next start runs the plugin again.
#[test]
fn review_disable_in_one_window_is_undone_by_another() {
    let base = tempfile::tempdir().expect("tempdir");
    let paths = paths(base.path());
    let a = window(&paths, None);
    let b = window(&paths, None);
    a.set_enabled(&id("acme.tools"), false);
    let saved = window(&paths, None);
    assert!(
        !saved
            .manager()
            .expect("loaded")
            .is_enabled(&id("acme.tools")),
        "precondition: the choice is saved"
    );
    drop(saved);
    b.set_enabled(&id("acme.other"), false);
    let later = window(&paths, None);
    assert!(
        !later
            .manager()
            .expect("loaded")
            .is_enabled(&id("acme.tools")),
        "the plugin disabled in window A is enabled again"
    );
}

/// "Uninstall … Its folder and the permissions you gave it are removed."
/// Only the open project's grants are removed: "Always for this project"
/// grants in other projects stay, and any later plugin installed with the
/// same id (another author's) gets project-file and network access there
/// without being asked.
#[test]
fn review_uninstall_keeps_grants_of_other_projects() {
    let base = tempfile::tempdir().expect("tempdir");
    let paths = paths(base.path());
    let installed: PathBuf = paths.installed.clone();
    plugin_dir(&installed, "acme.notes");
    let other_project = base.path().join("other-project");
    let open_project = base.path().join("open-project");
    fs::create_dir_all(&other_project).expect("mkdir");
    fs::create_dir_all(&open_project).expect("mkdir");
    let other_grants = paths.project_grants(&other_project);
    fs::create_dir_all(other_grants.parent().expect("parent")).expect("mkdir");
    fs::write(
        &other_grants,
        grant_lines(&[
            ("acme.notes", "workspace-read", None),
            ("acme.notes", "network", Some("exfil.test")),
        ]),
    )
    .expect("write");
    let notes = id("acme.notes");
    let ws = window(&paths, Some(&open_project));
    ws.uninstall(&notes).expect("uninstalled");
    assert!(!installed.join("acme.notes").exists(), "folder removed");
    drop(ws);

    // A different plugin with the same id is installed later.
    plugin_dir(&installed, "acme.notes");
    let later = window(&paths, Some(&other_project));
    let grants = later.manager().expect("loaded").grants(&notes);
    assert!(
        grants.is_empty(),
        "the uninstalled plugin's grants are still there: {grants:?}"
    );
}
