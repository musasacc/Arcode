//! Security review: files, the compiled-component cache and editor access.
//! Each test states the behaviour the sandbox should have; a failing test
//! is an open finding (see the review notes in the test docs).
//!
//! Needs the fixtures: run `scripts/build-test-plugins.sh` first.

#[allow(dead_code)]
#[path = "sandbox/common.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arcode_plugin_host::bridge::DocSnapshot;
use arcode_plugin_host::consent::{ConsentAnswer, ScriptedConsent};
use arcode_plugin_host::grants::GrantScope;
use arcode_plugin_host::manifest::UserLimits;
use arcode_plugin_host::registry::load;
use arcode_plugin_host::{CallOutput, PluginSource, RegisteredPlugin};
use serde_json::{Value, json};

use common::*;

const ALLOW_WRITE: ConsentAnswer = ConsentAnswer::Allow {
    scope: GrantScope::Once,
    write: true,
};

/// A plugin folder `dir/<id>` holding `manifest` and a copy of `wasm`.
fn plugin_folder(dir: &Path, id: &str, manifest: &str, wasm: &Path) -> PathBuf {
    let folder = dir.join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("plugin.toml"), manifest).unwrap();
    std::fs::copy(wasm, folder.join("plugin.wasm")).unwrap();
    folder
}

fn registered(dir: PathBuf, source: PluginSource) -> RegisteredPlugin {
    let manifest = load(&dir, source).expect("manifest");
    let limits = manifest.limits.clamp(&UserLimits::default());
    RegisteredPlugin {
        manifest,
        dir,
        source,
        limits,
        shadowed: Vec::new(),
    }
}

fn op_result(h: &arcode_plugin_host::InstanceHandle, op: &str) -> Value {
    last_with(h, "op", op)["result"].clone()
}

/// Finding: `editor.open-file` checks the path lexically only. A plugin
/// with a write grant creates `/project/leak.abuse -> ../secret.txt`
/// through WASI (cap-std refuses absolute targets but accepts `..`
/// targets; it only refuses *following* them) and asks the host to open
/// it; the host forwards it to the app, which opens (and, since `abuse` is
/// a language the plugin contributes, lets the plugin read through
/// `editor.text`) a file outside the project.
#[cfg(unix)]
#[test]
fn open_file_refuses_symlinks_that_leave_the_project() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    let secret = tmp.path().join("secret.txt");
    std::fs::write(&secret, "outside the project").unwrap();
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let editor = Arc::new(FakeEditor::default());
    let svc = services(
        &app,
        Arc::new(ScriptedConsent::always(ALLOW_WRITE)),
        editor.clone(),
        Arc::new(FakeHttp::default()),
    );
    let h = handle(&rt, fixture("review-abuse"), svc, Some(&root));
    let run = |args: Value| assert_eq!(command(&h, "run", args, true), Ok(CallOutput::Done));
    run(json!({"op": "symlink", "target": "../secret.txt", "link": "/project/leak.abuse"}));
    assert_eq!(
        op_result(&h, "symlink"),
        json!({"ok": true}),
        "WASI symlink"
    );
    assert!(root.join("leak.abuse").is_symlink());
    run(json!({"op": "open-file", "path": "leak.abuse"}));
    let opened = editor.opened.lock().unwrap().clone();
    assert!(
        opened.is_empty() && op_result(&h, "open-file")["err"].is_string(),
        "the host asked the app to open a symlink leaving the project: {opened:?}, {}",
        op_result(&h, "open-file")
    );
}

/// Finding: `editor = "context"` is never asked about, yet it silently
/// covers every open document of any language listed under
/// `contributes.code-lenses` (or `contributes.languages`), including
/// built-in ones like `rust`. §4: context is the lens/command origin
/// document or a language the plugin itself contributes; claiming `rust`
/// turns "context" into an unprompted `all-open` for Rust files.
#[test]
fn context_access_is_not_widened_by_claiming_builtin_languages() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = r#"[plugin]
id = "test.lens-claim"
name = "Lens Claim"
version = "0.1.0"
api = "0.1"
entry = "plugin.wasm"

[capabilities]
editor = "context"
network = { hosts = ["127.0.0.1"] }

[[contributes.commands]]
id = "echo"
title = "Echo"

[[contributes.code-lenses]]
languages = ["rust", "python", "plaintext"]
"#;
    let wasm = fixture("echo").dir.join("plugin.wasm");
    let dir = plugin_folder(tmp.path(), "test.lens-claim", manifest, &wasm);
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let editor = FakeEditor {
        docs: vec![DocSnapshot {
            id: 7,
            path: Some("src/secrets.rs".into()),
            language: "rust".into(),
            version: 1,
            rope: ropey::Rope::from_str("const API_KEY: &str = \"hunter2\";\n"),
            selections: vec![],
        }],
        focused: None,
        ..FakeEditor::default()
    };
    let svc = services(
        &app,
        Arc::new(ScriptedConsent::default()),
        Arc::new(editor),
        Arc::new(FakeHttp::default()),
    );
    let h = handle(&rt, registered(dir, PluginSource::Dev), svc, None);
    // A palette command: no origin document, document 7 is not focused.
    let r = command(&h, "echo", json!({"op": "editor", "doc": 7}), true);
    assert_eq!(r, Ok(CallOutput::Done));
    let text = op_result(&h, "editor")["text"].clone();
    assert_eq!(
        text,
        json!({"Err": "Denied"}),
        "an unrelated Rust document was readable without any prompt"
    );
}

/// Finding: the entry `plugin.wasm` is never checked to be a regular file
/// (only `plugin.toml` is). An installed plugin whose `plugin.wasm` is a
/// FIFO hangs its activation forever; one whose `plugin.wasm` is a
/// symlink to `/dev/zero` (length 0, so the 32 MiB check passes) makes
/// `fs::read` allocate until the editor process dies.
#[cfg(unix)]
#[test]
fn entry_that_is_not_a_regular_file_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = std::fs::read_to_string(fixture("spin").dir.join("plugin.toml")).unwrap();
    let id = load(&fixture("spin").dir, PluginSource::Dev)
        .unwrap()
        .id()
        .to_string();
    let folder = tmp.path().join(&id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("plugin.toml"), manifest).unwrap();
    let fifo = folder.join("plugin.wasm");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    assert!(made.is_ok_and(|s| s.success()), "mkfifo");
    let plugin = registered(folder, PluginSource::Installed);
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let svc = services(
        &app,
        Arc::new(ScriptedConsent::default()),
        Arc::new(FakeEditor::default()),
        Arc::new(FakeHttp::default()),
    );
    let h = handle(&rt, plugin, svc, None);
    let command_id = h.plugin().manifest.contributes.commands[0].id.clone();
    let pending = h.dispatch(
        arcode_plugin_host::Call::Command {
            command: command_id,
            args: "{}".into(),
        },
        true,
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut answered = None;
    while Instant::now() < deadline {
        if let Ok(r) = pending.result.try_recv() {
            answered = Some(r);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if answered.is_none() {
        // Unblock the plugin thread (it waits for a FIFO writer).
        drop(std::fs::OpenOptions::new().write(true).open(&fifo));
    }
    let answered = answered.expect("activation hung reading a FIFO plugin.wasm");
    assert!(answered.is_err(), "{answered:?}");
}

/// Finding: the `.cwasm` cache trusts any file in its directory that has
/// the right name and never makes an existing directory private, so a
/// cache dir that already existed with loose permissions lets another
/// local user plant native code (`Component::deserialize_file` is
/// `unsafe` for exactly this reason). §2: "We only do this for files we
/// wrote".
#[cfg(unix)]
#[test]
fn a_planted_cwasm_is_never_deserialized() {
    use arcode_plugin_host::imports_check::component_imports;
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let shared = tmp.path().join("cwasm");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777)).unwrap();
    let (rt, _keep) = runtime(|c| c.cache_dir = shared.clone());
    let engine = rt.engine().unwrap();
    let spin = fixture("spin").dir.join("plugin.wasm");
    let echo = fixture("echo").dir.join("plugin.wasm");
    let genuine = component_imports(engine, &rt.cache().load(engine, &spin).unwrap());
    let entry = std::fs::read_dir(&shared)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "cwasm"))
        .expect("cache entry");
    // "Another user" swaps in a different (here: valid) native artifact.
    let planted = rt
        .cache()
        .prepare(engine, &echo)
        .map(|p| std::fs::read(p).unwrap())
        .unwrap();
    std::fs::write(&entry, planted).unwrap();
    let loaded = component_imports(engine, &rt.cache().load(engine, &spin).unwrap());
    let mode = std::fs::metadata(&shared).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        loaded, genuine,
        "spin.wasm was loaded from a planted artifact (cache dir mode {mode:o})"
    );
}
