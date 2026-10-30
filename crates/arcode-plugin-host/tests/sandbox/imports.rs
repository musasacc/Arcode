//! Step 6: `kv` persistence and quota, `find-files` against a real
//! `.gitignore`, and `editor` reads/writes through [`MemoryEditor`].

use std::path::Path;
use std::sync::Arc;

use arcode_plugin_host::bridge::{DocSnapshot, EditorBridge};
use arcode_plugin_host::consent::{ConsentAnswer, ScriptedConsent};
use arcode_plugin_host::grants::GrantScope;
use arcode_plugin_host::host_impl::kv::{KV_QUOTA_BYTES, KvFiles};
use arcode_plugin_host::index::WalkIndex;
use arcode_plugin_host::memory_editor::MemoryEditor;
use arcode_plugin_host::{Call, CallOutput, InstanceHandle};
use serde_json::{Value, json};

use crate::common::*;

const ALWAYS: ConsentAnswer = ConsentAnswer::Allow {
    scope: GrantScope::Global,
    write: true,
};

fn op(h: &InstanceHandle, args: Value) -> Value {
    op_about(h, args, None)
}

fn op_about(h: &InstanceHandle, args: Value, doc: Option<u64>) -> Value {
    let name = args["op"].as_str().unwrap_or_default().to_owned();
    let call = Call::Command {
        command: "echo".into(),
        args: args.to_string(),
    };
    let pending = match doc {
        Some(d) => h.dispatch_about(call, false, d),
        None => h.dispatch(call, false),
    };
    assert_eq!(
        pending.wait(),
        Ok(CallOutput::Done),
        "{:?}",
        h.log().lines()
    );
    last_with(h, "op", &name)["result"].clone()
}

/// An echo instance and what keeps it alive.
struct Echo {
    h: InstanceHandle,
    consent: Arc<ScriptedConsent>,
    _app: App,
    _cache: tempfile::TempDir,
}

/// An echo instance with kv in `kv`, a walking index and `editor`.
fn echo_with(kv: KvFiles, project: &Path, editor: Arc<MemoryEditor>) -> Echo {
    let app = App::start(None);
    let consent = Arc::new(ScriptedConsent::always(ALWAYS));
    let mut services = services(&app, consent.clone(), editor, Arc::new(FakeHttp::default()));
    services.index = Arc::new(WalkIndex::new(project));
    let (rt, cache) = runtime(|_| {});
    let mut spec = spec(fixture("echo"), services, Some(project));
    spec.kv = kv;
    Echo {
        h: InstanceHandle::new(rt, spec),
        consent,
        _app: app,
        _cache: cache,
    }
}

fn files(dir: &Path, project: &str) -> KvFiles {
    KvFiles {
        global: Some(dir.join("test.echo/global.kv")),
        project: Some(dir.join(format!("test.echo/{project}.kv"))),
    }
}

#[test]
fn kv_scopes_persist_across_instances_and_projects() {
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let editor = Arc::new(MemoryEditor::default());
    let e = echo_with(files(data.path(), "p1"), project.path(), editor.clone());
    let h = &e.h;
    op(h, json!({"op": "kv", "key": "g", "value": "1"}));
    op(
        h,
        json!({"op": "kv", "key": "p", "value": "2", "project": true}),
    );
    let r = op(
        h,
        json!({"op": "kv", "key": "big", "value": "x".repeat(KV_QUOTA_BYTES)}),
    );
    assert_eq!(r["set"], "KvError::QuotaExceeded");
    h.shutdown();
    assert!(h.wait_stopped(std::time::Duration::from_secs(5)));
    assert!(data.path().join("test.echo/global.kv").is_file());

    // A new instance (e.g. after restarting Arcode) reads both scopes back.
    let e = echo_with(files(data.path(), "p1"), project.path(), editor.clone());
    let h = &e.h;
    assert_eq!(
        op(h, json!({"op": "kv-get", "key": "g"})),
        json!({"get": "1", "keys": ["g"]})
    );
    let r = op(h, json!({"op": "kv-get", "key": "p", "project": true}));
    assert_eq!(r, json!({"get": "2", "keys": ["p"]}));
    h.shutdown();

    // Another project shares the global scope only.
    let e = echo_with(files(data.path(), "p2"), project.path(), editor);
    let h = &e.h;
    assert_eq!(op(h, json!({"op": "kv-get", "key": "g"}))["get"], "1");
    let r = op(h, json!({"op": "kv-get", "key": "p", "project": true}));
    assert_eq!(r, json!({"get": null, "keys": []}));
}

#[test]
fn find_files_honours_gitignore() {
    let project = tempfile::tempdir().unwrap();
    for f in [
        "notes.txt",
        "docs/a.txt",
        "build/out.txt",
        "docs/tmp.txt",
        "src/lib.rs",
    ] {
        let path = project.path().join(f);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x").unwrap();
    }
    std::fs::write(project.path().join(".gitignore"), "build/\ntmp.txt\n").unwrap();
    let e = echo_with(
        KvFiles::none(),
        project.path(),
        Arc::new(MemoryEditor::default()),
    );
    let h = &e.h;
    let r = op(
        h,
        json!({"op": "find-files", "glob": "**/*.txt", "limit": 100}),
    );
    assert_eq!(r, json!({"ok": ["docs/a.txt", "notes.txt"]}));
    let r = op(h, json!({"op": "find-files", "glob": "**/*", "limit": 1}));
    assert_eq!(r["ok"].as_array().map(Vec::len), Some(1));
    let r = op(
        h,
        json!({"op": "find-files", "glob": "*.txt", "limit": 100}),
    );
    assert_eq!(
        r,
        json!({"ok": ["notes.txt"]}),
        "`*` does not cross directories"
    );
}

fn snapshot(id: u64, language: &str, version: u64, text: &str) -> DocSnapshot {
    DocSnapshot {
        id,
        path: Some(format!("doc{id}.{language}")),
        language: language.into(),
        version,
        rope: ropey::Rope::from_str(text),
        selections: vec![],
    }
}

#[test]
fn editor_reads_snapshots_and_checks_versions() {
    let project = tempfile::tempdir().unwrap();
    let editor = Arc::new(MemoryEditor::default());
    editor.publish(snapshot(7, "echo", 5, "hello\nwörld\n"));
    let e = echo_with(KvFiles::none(), project.path(), editor.clone());
    let (h, consent) = (&e.h, &e.consent);
    let range = json!({"start": {"line": 1, "column": 1}, "end": {"line": 1, "column": 4}});
    let read = |extra: Value| {
        let mut args = json!({"op": "editor", "doc": 7});
        args.as_object_mut()
            .unwrap()
            .extend(range.as_object().unwrap().clone());
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        op(h, args)
    };
    assert_eq!(read(json!({}))["text"], json!({"Ok": "örl"}));
    // The app publishes a new snapshot after an edit; reads see it at once.
    editor.publish(snapshot(7, "echo", 6, "hello\nWORLD\n"));
    assert_eq!(read(json!({}))["text"], json!({"Ok": "ORL"}));
    let asked = consent.asked().len();
    // A stale version is refused before any prompt.
    let r = read(json!({"replace": "x", "version": 5}));
    assert_eq!(r["edit"], json!({"Err": "EditorError::VersionMismatch(6)"}));
    assert_eq!(consent.asked().len(), asked);
    let r = read(json!({"replace": "orl", "version": 6}));
    assert_eq!(r["edit"], json!({"Ok": 7}));
    assert_eq!(editor.text(7).as_deref(), Some("hello\nWorlD\n"));
    assert_eq!(consent.asked().len(), asked + 1, "editor-write asked once");
    let r = read(json!({"replace": "!", "version": 7}));
    assert_eq!(r["edit"], json!({"Ok": 8}));
    assert_eq!(consent.asked().len(), asked + 1, "then remembered");
    // An out-of-range edit is refused and changes nothing.
    let r = op(
        h,
        json!({"op": "editor", "doc": 7, "replace": "x", "version": 8,
        "start": {"line": 9, "column": 0}, "end": {"line": 9, "column": 0}}),
    );
    assert_eq!(r["edit"], json!({"Err": "EditorError::InvalidRange"}));
    assert_eq!(editor.snapshot(7).map(|s| s.version), Some(8));
    let scratch = op(
        h,
        json!({"op": "editor", "doc": 7, "scratch": "{\"a\": 1}"}),
    );
    let id = scratch["scratch"]["Ok"].as_u64().unwrap();
    assert_eq!(editor.text(id).as_deref(), Some("{\"a\": 1}"));
}

#[test]
fn context_access_covers_only_the_call_document() {
    let project = tempfile::tempdir().unwrap();
    let editor = Arc::new(MemoryEditor::default());
    editor.publish(snapshot(8, "rust", 1, "fn main() {}\n"));
    editor.focus(Some(8));
    let e = echo_with(KvFiles::none(), project.path(), editor);
    let h = &e.h;
    let r = op(h, json!({"op": "editor", "doc": 8}));
    assert_eq!(r["text"], json!({"Err": "EditorError::Denied"}));
    assert_eq!(
        r["current"],
        Value::Null,
        "the focused rust file is not visible"
    );
    // A command about doc 8 (lens click, editor command) may read it.
    let r = op_about(h, json!({"op": "editor", "doc": 8}), Some(8));
    assert_eq!(r["text"], json!({"Ok": "fn main() {}\n"}));
    assert_eq!(r["current"]["id"], 8);
}
