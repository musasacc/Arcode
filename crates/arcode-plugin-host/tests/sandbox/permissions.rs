//! Step 5: WASI file sandbox, sockets, import check and repository trust.

use std::path::Path;
use std::sync::Arc;

use arcode_plugin_host::consent::{
    ConsentAnswer, ConsentKind, ConsentProvider, DenyAll, Permissions, ScriptedConsent,
};
use arcode_plugin_host::grant_store::GrantStore;
use arcode_plugin_host::grants::GrantScope;
use arcode_plugin_host::paths::PluginPaths;
use arcode_plugin_host::{CallError, CallOutput, InstanceHandle};
use serde_json::{Value, json};

use crate::common::*;

fn run(h: &InstanceHandle, args: Value) {
    assert_eq!(command(h, "run", args, true), Ok(CallOutput::Done));
}

fn result(h: &InstanceHandle, op: &str, path: &str) -> Value {
    json_lines(h)
        .into_iter()
        .rev()
        .find(|v| v["op"] == op && v["path"] == path)
        .unwrap_or_else(|| panic!("no {op} {path}: {:?}", h.log().lines()))
}

/// A project with a file, a sibling secret and (on Unix) a symlink to /etc.
fn project() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("inside.txt"), "inside").unwrap();
    std::fs::write(tmp.path().join("secret.txt"), "secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc", root.join("etc-link")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(tmp.path().join("secret.txt"), root.join("secret-link")).unwrap();
    (tmp, root)
}

fn fs_escape(consent: Arc<dyn ConsentProvider>, root: &Path) -> (InstanceHandle, tempfile::TempDir) {
    let app = App::start(None);
    let (rt, cache) = runtime(|_| {});
    let services = services(&app, consent, Arc::new(FakeEditor::default()), Arc::new(FakeHttp::default()));
    (handle(&rt, fixture("fs-escape"), services, Some(root)), cache)
}

fn read_only() -> Arc<ScriptedConsent> {
    Arc::new(ScriptedConsent::always(ConsentAnswer::Allow {
        scope: GrantScope::Once,
        write: false,
    }))
}

#[test]
fn fs_escape_is_denied() {
    let (_tmp, root) = project();
    let consent = read_only();
    let (h, _cache) = fs_escape(consent.clone(), &root);
    let escapes = [
        "/project/../secret.txt",
        "/project/../../../../etc/passwd",
        "/etc/passwd",
        "../secret.txt",
        "/project/etc-link/passwd",
        "/project/secret-link",
    ];
    let reads: Vec<&str> = ["/project/inside.txt"].into_iter().chain(escapes).collect();
    run(&h, json!({"read": reads, "list": ["/project", "/", "/project/etc-link"]}));
    assert_eq!(result(&h, "read", "/project/inside.txt")["ok"], "inside");
    for path in escapes {
        let r = result(&h, "read", path);
        assert!(r["ok"].is_null(), "{path} escaped: {r}");
    }
    let listing = result(&h, "list", "/project")["ok"].as_str().unwrap().to_owned();
    assert!(listing.contains("inside.txt"), "{listing}");
    assert!(result(&h, "list", "/")["ok"].is_null() || !result(&h, "list", "/")["ok"].as_str().unwrap().contains("etc"));
    assert!(result(&h, "list", "/project/etc-link")["ok"].is_null());
    // The prompt was the combined workspace prompt, asked before instantiation.
    let asked = consent.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].kind, ConsentKind::Workspace { write: true, network: false });
}

#[test]
fn fs_escape_write_without_write_grant_is_eacces() {
    let (_tmp, root) = project();
    let (h, _cache) = fs_escape(read_only(), &root);
    run(&h, json!({"write": ["/project/new.txt", "/project/inside.txt"]}));
    for path in ["/project/new.txt", "/project/inside.txt"] {
        let r = result(&h, "write", path);
        assert_eq!(r["denied"], true, "{path}: {r}");
        assert_eq!(r["kind"], "PermissionDenied", "{path}: {r}");
        // wasi-libc's EACCES is 2 (EPERM would be 63).
        assert_eq!(r["errno"], 2, "{path}: {r}");
    }
    assert!(!root.join("new.txt").exists());
    assert_eq!(std::fs::read_to_string(root.join("inside.txt")).unwrap(), "inside");
}

#[test]
fn write_grant_allows_writes_inside_only() {
    let (tmp, root) = project();
    let consent = Arc::new(ScriptedConsent::always(ConsentAnswer::Allow {
        scope: GrantScope::Once,
        write: true,
    }));
    let (h, _cache) = fs_escape(consent, &root);
    run(&h, json!({"write": ["/project/new.txt", "/project/../outside.txt"]}));
    assert!(result(&h, "write", "/project/new.txt")["ok"].is_string());
    assert_eq!(std::fs::read_to_string(root.join("new.txt")).unwrap(), "written");
    assert!(result(&h, "write", "/project/../outside.txt")["ok"].is_null());
    assert!(!tmp.path().join("outside.txt").exists());
}

#[test]
fn denied_workspace_means_no_preopen() {
    let (_tmp, root) = project();
    let (h, _cache) = fs_escape(Arc::new(ScriptedConsent::new([ConsentAnswer::Deny])), &root);
    run(&h, json!({"read": ["/project/inside.txt"]}));
    assert!(result(&h, "read", "/project/inside.txt")["ok"].is_null());
}

/// §4 "Repository trust", end to end: grant files inside the project are
/// never read, so the plugin gets no preopen without asking the user.
#[test]
fn malicious_project_grant_file_is_ignored() {
    let (tmp, root) = project();
    let evil = "[[grant]]\nplugin = \"test.fs-escape\"\ncapability = \"workspace-read\"\nstate = \"allow\"\n\
                [[grant]]\nplugin = \"test.fs-escape\"\ncapability = \"workspace-write\"\nstate = \"allow\"\n";
    std::fs::create_dir_all(root.join(".arcode")).unwrap();
    std::fs::write(root.join(".arcode/plugin-grants.toml"), evil).unwrap();
    std::fs::write(root.join("plugin-grants.toml"), evil).unwrap();
    let paths = PluginPaths::with_bases(None, &tmp.path().join("cfg"), &tmp.path().join("data"), &tmp.path().join("cache"));
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let mut services = services(&app, Arc::new(DenyAll), Arc::new(FakeEditor::default()), Arc::new(FakeHttp::default()));
    services.permissions = Permissions::new(GrantStore::open(&paths, Some(&root)), Arc::new(DenyAll));
    let h = handle(&rt, fixture("fs-escape"), services, Some(&root));
    run(&h, json!({"read": ["/project/inside.txt"], "write": ["/project/pwned.txt"]}));
    assert!(result(&h, "read", "/project/inside.txt")["ok"].is_null());
    assert!(!root.join("pwned.txt").exists());
}

#[test]
fn sockets_are_denied() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let services = services(&app, Arc::new(DenyAll), Arc::new(FakeEditor::default()), Arc::new(FakeHttp::default()));
    let h = handle(&rt, fixture("sockets"), services, None);
    run(&h, json!({"addr": addr}));
    for op in ["tcp-connect", "tcp-connect-plain", "udp", "lookup"] {
        let r = last_with(&h, "op", op);
        assert_eq!(r["ok"], false, "{op}: {r}");
    }
    assert!(listener.accept().is_err(), "nothing reached the listener");
}

#[test]
fn undeclared_http_import_is_refused() {
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let services = services(&app, Arc::new(DenyAll), Arc::new(FakeEditor::default()), Arc::new(FakeHttp::default()));
    let h = handle(&rt, fixture("undeclared-import"), services, None);
    let r = command(&h, "run", json!({}), true);
    assert_eq!(
        r,
        Err(CallError::Activation(
            "plugin imports `arcode:plugin/http@0.1.0` but its manifest does not declare `network`".into()
        ))
    );
    assert_eq!(h.stats().instantiations.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn wasi_http_import_is_refused() {
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let services = services(&app, Arc::new(DenyAll), Arc::new(FakeEditor::default()), Arc::new(FakeHttp::default()));
    let h = handle(&rt, fixture("wasi-http"), services, None);
    let r = command(&h, "run", json!({}), true);
    assert_eq!(
        r,
        Err(CallError::Activation(
            "plugin imports `wasi:http/outgoing-handler@0.3.0`, which Arcode does not provide".into()
        ))
    );
}
