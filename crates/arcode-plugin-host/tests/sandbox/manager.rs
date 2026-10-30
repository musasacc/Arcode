//! The `PluginManager` facade, as the app will drive it: discovery costs
//! nothing, commands/panels/openers/lenses dispatch to the right plugin,
//! grants and kv persist under the user's dirs, revoking the file grant
//! recycles the instance.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use arcode_plugin_host::bindings::ui::{Event, EventKind};
use arcode_plugin_host::bridge::DocSnapshot;
use arcode_plugin_host::consent::{ConsentAnswer, ConsentKind, ScriptedConsent};
use arcode_plugin_host::grants::{Capability, GrantScope};
use arcode_plugin_host::manager::event_is_gesture;
use arcode_plugin_host::memory_editor::MemoryEditor;
use arcode_plugin_host::paths::PluginPaths;
use arcode_plugin_host::{
    AppServices, CallOutput, ManagerConfig, ManagerError, PluginId, PluginManager, PluginOutput,
    ProjectContext, QualifiedId,
};
use serde_json::json;

use crate::common::*;

struct Setup {
    manager: PluginManager,
    outputs: async_channel::Receiver<PluginOutput>,
    consent: Arc<ScriptedConsent>,
    editor: Arc<MemoryEditor>,
    dirs: tempfile::TempDir,
}

fn setup() -> Setup {
    let dirs = tempfile::tempdir().unwrap();
    let base = dirs.path();
    let paths = PluginPaths::with_bases(
        None,
        &base.join("config"),
        &base.join("data"),
        &base.join("cache"),
    );
    let mut config = ManagerConfig::new(paths);
    config.dev_folders = ["echo", "ui-big"].iter().map(|f| fixture(f).dir).collect();
    let consent = Arc::new(ScriptedConsent::always(ConsentAnswer::Allow {
        scope: GrantScope::Global,
        write: false,
    }));
    let editor = Arc::new(MemoryEditor::default());
    let app = AppServices {
        editor: editor.clone(),
        index: None,
        http: Arc::new(FakeHttp::default()),
        consent: Some(consent.clone()),
    };
    let (manager, outputs) = PluginManager::new(config, app);
    Setup {
        manager,
        outputs,
        consent,
        editor,
        dirs,
    }
}

fn q(id: &str) -> QualifiedId {
    QualifiedId::parse(id).unwrap()
}

fn echo_id() -> PluginId {
    PluginId::parse("test.echo").unwrap()
}

fn project(root: &Path, name: &str) -> Option<ProjectContext> {
    Some(ProjectContext {
        root: root.to_owned(),
        name: name.into(),
    })
}

#[test]
fn discovery_starts_nothing() {
    let s = setup();
    assert_eq!(s.manager.registry().plugins().count(), 2);
    assert!(!s.manager.runtime().engine_created());
    let h = s.manager.instance(&echo_id()).unwrap();
    assert!(!h.thread_running(), "a handle alone costs nothing");
    assert!(!s.manager.runtime().engine_created());
}

#[test]
fn commands_panels_openers_and_lenses_reach_their_plugin() {
    let mut s = setup();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("hello.txt"), "hi").unwrap();
    s.manager.set_project(project(root.path(), "demo"));
    let run = |args: serde_json::Value| {
        s.manager
            .run_command(&q("test.echo/echo"), &args.to_string(), None)
    };
    assert_eq!(
        run(json!({"op": "root-name"})).unwrap().wait(),
        Ok(CallOutput::Done)
    );
    let h = s.manager.instance(&echo_id()).unwrap();
    assert_eq!(last_with(&h, "op", "root-name")["result"], "demo");
    // The default index walks the project.
    run(json!({"op": "find-files", "glob": "*.txt", "limit": 5}))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(
        last_with(&h, "op", "find-files")["result"],
        json!({"ok": ["hello.txt"]})
    );
    // Commands are user gestures.
    run(json!({"op": "begin-task"})).unwrap().wait().unwrap();
    assert_eq!(last_with(&h, "op", "begin-task")["result"], true);
    let task = std::iter::from_fn(|| s.outputs.recv_blocking().ok())
        .find(|o| matches!(o, PluginOutput::TaskStarted { .. }));
    assert!(
        matches!(task, Some(PluginOutput::TaskStarted { cancel, .. }) if !cancel.is_cancelled())
    );

    assert_eq!(
        s.manager
            .run_command(&q("test.echo/nope"), "{}", None)
            .unwrap_err(),
        ManagerError::UnknownContribution("test.echo/nope".into())
    );
    assert!(
        s.manager
            .run_command(&q("test.other/x"), "{}", None)
            .is_err()
    );

    let shown = s.manager.show_panel(&q("test.ui-big/big")).unwrap();
    assert_eq!(shown.pending.wait(), Ok(CallOutput::Done));
    let big = s.manager.instance(&shown.plugin).unwrap();
    assert!(
        big.panel_view(shown.panel)
            .is_some_and(|v| v.nodes.len() == 3)
    );
    let click = Event {
        node: 1,
        kind: EventKind::Click,
        values: vec![],
    };
    assert!(event_is_gesture(&click.kind));
    assert!(!event_is_gesture(&EventKind::TreeToggle("x".into())));
    let r = s
        .manager
        .ui_event(&shown.plugin, shown.panel, click)
        .unwrap()
        .wait();
    assert_eq!(r, Ok(CallOutput::Done));
    assert_eq!(
        s.manager
            .close_panel(&shown.plugin, shown.panel)
            .unwrap()
            .wait(),
        Ok(CallOutput::Done)
    );
    assert!(big.open_panels().is_empty());

    let opener = s.manager.opener_for(Path::new("a/b.echo-view")).unwrap();
    assert_eq!(opener.as_str(), "test.echo/echo-file");
    let opened = s.manager.open_with(&opener, "a/b.echo-view").unwrap();
    assert_eq!(opened.pending.wait(), Ok(CallOutput::Done));
    assert_eq!(
        last_with(&h, "export", "open-view")["args"],
        r#"{"path":"a/b.echo-view"}"#
    );

    s.editor.publish(DocSnapshot {
        id: 3,
        path: Some("x.echo".into()),
        language: "echo".into(),
        version: 2,
        rope: ropey::Rope::from_str("a\nb\n"),
        selections: vec![],
    });
    let lenses = s.manager.code_lenses(3);
    assert_eq!(lenses.len(), 1);
    assert_eq!(lenses[0].plugin, echo_id());
    assert!(matches!(lenses[0].pending.wait(), Ok(CallOutput::Lenses(l)) if l.len() == 1));
    assert!(s.manager.code_lenses(99).is_empty(), "unknown document");

    s.manager.set_enabled(&echo_id(), false);
    let refused = s.manager.run_command(&q("test.echo/echo"), "{}", None);
    assert_eq!(refused.unwrap_err(), ManagerError::Disabled(echo_id()));
}

#[test]
fn grants_and_kv_persist_and_revoking_files_recycles() {
    let mut s = setup();
    let root = tempfile::tempdir().unwrap();
    s.manager.set_project(project(root.path(), "demo"));
    let run = |m: &PluginManager, args: serde_json::Value| {
        m.run_command(&q("test.echo/echo"), &args.to_string(), None)
            .unwrap()
            .wait()
    };
    run(
        &s.manager,
        json!({"op": "kv", "key": "k", "value": "v", "project": true}),
    )
    .unwrap();
    let workspace_prompts = || {
        s.consent
            .asked()
            .iter()
            .filter(|r| matches!(r.kind, ConsentKind::Workspace { .. }))
            .count()
    };
    assert_eq!(workspace_prompts(), 1);
    let grants = s.manager.grants(&echo_id());
    assert!(
        grants
            .iter()
            .any(|(c, _, scope)| *c == Capability::WorkspaceRead && *scope == GrantScope::Global)
    );
    assert!(
        s.dirs
            .path()
            .join("config/arcode/plugin-grants.toml")
            .is_file()
    );
    let kv = s.dirs.path().join("data/arcode/plugin-data/test.echo");
    assert_eq!(
        std::fs::read_dir(&kv).unwrap().count(),
        1,
        "project kv written"
    );

    // Revoking the file grant recycles: the next call asks again first.
    assert!(s.manager.revoke(&echo_id(), &Capability::WorkspaceRead));
    run(&s.manager, json!({"op": "api-version"})).unwrap();
    assert_eq!(workspace_prompts(), 2);
    let h = s.manager.instance(&echo_id()).unwrap();
    let activations = json_lines(&h)
        .iter()
        .filter(|v| v["export"] == "activate")
        .count();
    assert_eq!(activations, 2);
    assert_eq!(
        last_with(&h, "export", "activate")["reason"],
        "Activation::Restore"
    );

    // Switching projects stops instances; project kv follows the project.
    let other = tempfile::tempdir().unwrap();
    s.manager.set_project(project(other.path(), "other"));
    assert!(h.wait_stopped(Duration::from_secs(5)));
    run(
        &s.manager,
        json!({"op": "kv-get", "key": "k", "project": true}),
    )
    .unwrap();
    let h = s.manager.instance(&echo_id()).unwrap();
    assert_eq!(
        last_with(&h, "op", "kv-get")["result"]["get"],
        serde_json::Value::Null
    );
    s.manager.set_project(project(root.path(), "demo"));
    run(
        &s.manager,
        json!({"op": "kv-get", "key": "k", "project": true}),
    )
    .unwrap();
    let h = s.manager.instance(&echo_id()).unwrap();
    assert_eq!(last_with(&h, "op", "kv-get")["result"]["get"], "v");
    s.manager.shutdown();
    assert!(h.wait_stopped(Duration::from_secs(5)));
}

#[test]
fn consent_prompts_arrive_as_events_by_default() {
    let dirs = tempfile::tempdir().unwrap();
    let base = dirs.path();
    let paths = PluginPaths::with_bases(None, &base.join("c"), &base.join("d"), &base.join("k"));
    let mut config = ManagerConfig::new(paths);
    config.dev_folders = vec![fixture("echo").dir];
    config.persist = false;
    let app = AppServices {
        editor: Arc::new(MemoryEditor::default()),
        index: None,
        http: Arc::new(FakeHttp::default()),
        consent: None,
    };
    let (mut manager, outputs) = PluginManager::new(config, app);
    let root = tempfile::tempdir().unwrap();
    manager.set_project(project(root.path(), "demo"));
    // The app's side: answer the first prompt, record its wording.
    let answerer = std::thread::spawn(move || {
        while let Ok(out) = outputs.recv_blocking() {
            if let PluginOutput::Consent { request, reply } = out {
                reply.send(ConsentAnswer::Allow {
                    scope: GrantScope::Project,
                    write: false,
                });
                return request.message();
            }
        }
        String::new()
    });
    let args = json!({"op": "read-file", "path": "missing.txt"}).to_string();
    let pending = manager
        .run_command(&q("test.echo/echo"), &args, None)
        .unwrap();
    assert_eq!(pending.wait(), Ok(CallOutput::Done));
    let message = answerer.join().unwrap();
    assert!(
        message.contains("\u{201c}Echo\u{201d} can read"),
        "{message}"
    );
    let grants = manager.grants(&echo_id());
    assert_eq!(grants.len(), 1);
    assert!(
        !dirs.path().join("c").exists(),
        "persist = false writes nothing"
    );
}
