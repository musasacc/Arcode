//! Security review: host file descriptors. Its own test binary because the
//! test deliberately exhausts the process's descriptor limit.
//!
//! Needs the fixtures: run `scripts/build-test-plugins.sh` first.

#[allow(dead_code)]
#[path = "sandbox/common.rs"]
mod common;

use std::sync::Arc;

use arcode_plugin_host::CallOutput;
use arcode_plugin_host::consent::{ConsentAnswer, ScriptedConsent};
use arcode_plugin_host::grants::GrantScope;
use serde_json::json;

use common::*;

/// Most descriptors a plugin should be able to hold at once.
const REASONABLE_CAP: u64 = 4096;

/// Finding: every file a guest opens through WASI is a real host
/// descriptor, and the instance's `ResourceTable` allows 1,000,000
/// entries, far above the process limit (often 1024-65536). A plugin with
/// an ordinary read grant opens `/project/x` in a loop, keeps the handles
/// in a static, and the *editor process* runs out of descriptors (EMFILE):
/// it can no longer open files, sockets or LSP pipes. The handles survive
/// across calls until the instance is dropped.
#[test]
fn a_plugin_cannot_exhaust_the_hosts_file_descriptors() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("x"), "x").unwrap();
    let app = App::start(None);
    let consent = ScriptedConsent::always(ConsentAnswer::Allow {
        scope: GrantScope::Once,
        write: false,
    });
    let svc = services(
        &app,
        Arc::new(consent),
        Arc::new(FakeEditor::default()),
        Arc::new(FakeHttp::default()),
    );
    let (rt, _cache) = runtime(|_| {});
    let h = handle(&rt, fixture("review-abuse"), svc, Some(tmp.path()));
    let args = json!({"op": "hold-files", "path": "/project/x", "max": 1_000_000});
    assert_eq!(command(&h, "run", args, true), Ok(CallOutput::Done));
    let held = last_with(&h, "op", "hold-files")["result"].clone();
    // While the plugin holds them, can the host still open a file?
    let host_open = std::fs::File::open(tmp.path().join("x")).map(drop);
    let release = json!({"op": "release-files"});
    assert_eq!(command(&h, "run", release, true), Ok(CallOutput::Done));
    assert!(
        host_open.is_ok(),
        "the host could not open a file while the plugin held {held}: {host_open:?}"
    );
    assert!(
        held["held"].as_u64().unwrap_or(u64::MAX) <= REASONABLE_CAP,
        "the plugin held {held} descriptors"
    );
}
