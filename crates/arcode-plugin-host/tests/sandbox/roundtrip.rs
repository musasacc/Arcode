//! Step 3: `echo` round-trips every WIT type through the fake bridge.

use std::sync::Arc;

use arcode_plugin_host::bindings::editor::DocInfo;
use arcode_plugin_host::bindings::ui::{Cell, Event, EventKind, Widget};
use arcode_plugin_host::bindings::Level;
use arcode_plugin_host::bridge::DocSnapshot;
use arcode_plugin_host::consent::{ConsentAnswer, ScriptedConsent};
use arcode_plugin_host::grants::GrantScope;
use arcode_plugin_host::{Call, CallError, CallOutput, InstanceHandle, PluginOutput};
use serde_json::{Value, json};

use crate::common::*;

fn doc(id: u64, language: &str, text: &str) -> DocSnapshot {
    DocSnapshot {
        id,
        path: Some(format!("doc{id}.{language}")),
        language: language.into(),
        version: 5,
        rope: ropey::Rope::from_str(text),
        selections: vec![range(0, 1, 0, 3)],
    }
}

fn op(h: &InstanceHandle, args: Value, gesture: bool) -> Value {
    let name = args["op"].as_str().unwrap_or_default().to_owned();
    let r = command(h, "echo", args, gesture);
    assert_eq!(r, Ok(CallOutput::Done), "op {name}: {:?}", h.log().lines());
    last_with(h, "op", &name)["result"].clone()
}

struct Echo {
    h: InstanceHandle,
    app: App,
    editor: Arc<FakeEditor>,
    http: Arc<FakeHttp>,
    consent: Arc<ScriptedConsent>,
    _project: tempfile::TempDir,
    _cache: tempfile::TempDir,
}

fn echo() -> Echo {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("hello.txt"), "hello from the project").unwrap();
    // `editor.open-file` only opens regular files inside the project.
    std::fs::create_dir_all(project.path().join("src")).unwrap();
    std::fs::write(project.path().join("src/lib.rs"), "").unwrap();
    let app = App::start(Some(2));
    let editor = Arc::new(FakeEditor {
        docs: vec![
            doc(7, "echo", "hello\nwörld\n"),
            doc(8, "rust", "fn main() {}"),
        ],
        focused: Some(7),
        ..FakeEditor::default()
    });
    let http = Arc::new(FakeHttp::default());
    let consent = Arc::new(ScriptedConsent::always(ConsentAnswer::Allow {
        scope: GrantScope::Global,
        write: false,
    }));
    let services = services(&app, consent.clone(), editor.clone(), http.clone());
    let (rt, cache) = runtime(|_| {});
    let h = handle(&rt, fixture("echo"), services, Some(project.path()));
    Echo {
        h,
        app,
        editor,
        http,
        consent,
        _project: project,
        _cache: cache,
    }
}

#[test]
fn host_kv_workspace_imports_round_trip() {
    let e = echo();
    let h = &e.h;
    assert_eq!(op(h, json!({"op": "api-version"}), false), "0.1.0");
    // Activation reason from the first call, and workspace consent asked once.
    assert_eq!(
        json_lines(h)[0],
        json!({"export": "activate", "reason": "Activation::Command(\"echo\")"})
    );
    assert_eq!(e.consent.asked().len(), 1);

    op(h, json!({"op": "log", "message": "careful"}), false);
    let warn = h
        .log()
        .lines()
        .into_iter()
        .find(|l| l.text == "careful")
        .unwrap();
    assert_eq!(warn.level, Level::Warn);

    op(h, json!({"op": "notify", "message": "boom"}), false);
    op(
        h,
        json!({"op": "status", "text": "env: dev", "command": "echo"}),
        false,
    );
    assert!(e.app.wait_for(|o| matches!(o, PluginOutput::Notify { level: Level::Error, message, .. } if message == "boom")));
    assert!(e.app.wait_for(
        |o| matches!(o, PluginOutput::Status { text: Some(t), command: Some(c), .. }
        if t == "env: dev" && c.as_str() == "test.echo/echo")
    ));

    assert_eq!(op(h, json!({"op": "quick-pick"}), true), 2);
    assert_eq!(e.app.picks.lock().unwrap()[0].1, ["one", "two", "three"]);

    let settings: Value =
        serde_json::from_str(op(h, json!({"op": "settings"}), false).as_str().unwrap()).unwrap();
    assert_eq!(
        settings,
        json!({"greeting": "hi"}),
        "user values merged, undeclared keys dropped"
    );
    assert_eq!(op(h, json!({"op": "cancelled"}), false), false);
    assert_eq!(
        op(h, json!({"op": "begin-task"}), false),
        false,
        "no task outside a gesture"
    );
    assert_eq!(op(h, json!({"op": "begin-task"}), true), true);
    assert!(e.app.wait_for(
        |o| matches!(o, PluginOutput::TaskStarted { title, .. } if title == "Echo task")
    ));
    assert!(
        e.app
            .wait_for(|o| matches!(o, PluginOutput::TaskFinished { .. }))
    );

    let kv = op(h, json!({"op": "kv", "key": "k", "value": "v"}), false);
    assert_eq!(kv, json!({"set": null, "get": "v", "keys": ["k"]}));
    let kv = op(
        h,
        json!({"op": "kv", "key": "bad\u{7}", "value": "v", "project": true}),
        false,
    );
    assert_eq!(
        kv,
        json!({"set": "KvError::InvalidKey", "get": null, "keys": []})
    );

    assert_eq!(op(h, json!({"op": "root-name"}), false), "proj");
    let found = op(
        h,
        json!({"op": "find-files", "glob": "**/*.txt", "limit": 5}),
        false,
    );
    assert_eq!(found, json!({"ok": ["hello.txt"]}));
    let bad = op(
        h,
        json!({"op": "find-files", "glob": "a[", "limit": 5}),
        false,
    );
    assert!(
        bad["err"]
            .as_str()
            .unwrap()
            .starts_with("FsError::InvalidGlob("),
        "{bad}"
    );
    let read = op(h, json!({"op": "read-file", "path": "hello.txt"}), false);
    assert_eq!(read, json!({"ok": "hello from the project"}));
}

#[test]
fn editor_and_http_imports_round_trip() {
    let e = echo();
    let h = &e.h;
    let r = op(
        h,
        json!({"op": "editor", "doc": 7, "start": {"line": 1, "column": 1}, "end": {"line": 1, "column": 4},
               "replace": "ÖRL", "version": 5, "scratch": "{}", "open": "src/lib.rs"}),
        false,
    );
    assert_eq!(
        r["current"],
        json!({"id": 7, "path": "doc7.echo", "language": "echo", "version": 5, "lines": 3})
    );
    assert_eq!(r["info"], json!({"Ok": 5}));
    assert_eq!(r["text"], json!({"Ok": "örl"}));
    assert_eq!(r["selections"], json!({"Ok": [[0, 1, 0, 3]]}));
    assert_eq!(r["edit"], json!({"Ok": 6}));
    assert_eq!(r["scratch"], json!({"Ok": 99}));
    assert_eq!(r["open"], json!({"Ok": null}));
    let edits = e.editor.edits.lock().unwrap();
    assert_eq!(edits[0].2[0].text, "ÖRL");
    assert_eq!(edits[0].2[0].range, range(1, 1, 1, 4));
    assert_eq!(e.editor.opened.lock().unwrap()[0].0, "src/lib.rs");
    drop(edits);

    // `editor = "context"`: a rust document is not this plugin's business.
    let r = op(h, json!({"op": "editor", "doc": 8}), false);
    assert_eq!(r["text"], json!({"Err": "EditorError::Denied"}));
    assert_eq!(r["info"], json!({"Err": "EditorError::Denied"}));
    let r = op(
        h,
        json!({"op": "editor", "doc": 7, "start": {"line": 9, "column": 0}, "end": {"line": 9, "column": 1}}),
        false,
    );
    assert_eq!(r["text"], json!({"Err": "EditorError::InvalidRange"}));

    // http: no gesture, no request.
    let r = op(
        h,
        json!({"op": "http", "method": "POST", "url": "http://127.0.0.1:8080/x", "body": "ping"}),
        false,
    );
    assert_eq!(r, json!({"err": "HttpError::NoUserGesture"}));
    let r = op(
        h,
        json!({"op": "http", "method": "POST", "url": "http://127.0.0.1:8080/x", "body": "ping"}),
        true,
    );
    assert_eq!(
        r,
        json!({"status": 201, "reason": "Created", "version": "HTTP/1.1", "final-url": "http://127.0.0.1:8080/x",
               "remote-addr": "127.0.0.1:8080", "headers": [["content-type", "text/plain"]], "body": "ping",
               "truncated": true, "headers-ms": 3, "total-ms": 7})
    );
    let sent = &e.http.requests.lock().unwrap()[0];
    assert_eq!(
        (sent.method.as_str(), sent.timeout_ms, sent.follow_redirects),
        ("POST", 5000, true)
    );
    assert_eq!(sent.headers, [("x-echo".to_owned(), "1".to_owned())]);
    assert_eq!(
        sent.max_response_bytes,
        50 * 1024 * 1024,
        "0 means the host default"
    );
    // The redirect to a host outside the manifest's list is refused.
    let redirects = e.http.redirects.lock().unwrap().clone();
    assert_eq!(
        redirects,
        [("127.0.0.1".into(), true), ("evil.test".into(), false)]
    );
    let r = op(
        h,
        json!({"op": "http", "method": "GET", "url": "https://elsewhere.test/"}),
        true,
    );
    assert_eq!(
        r,
        json!({"err": "HttpError::Denied"}),
        "host not in network.hosts"
    );
    let r = op(
        h,
        json!({"op": "http", "method": "GET", "url": "nope"}),
        true,
    );
    assert!(
        r["err"]
            .as_str()
            .unwrap()
            .starts_with("HttpError::InvalidRequest")
    );
}

#[test]
fn ui_and_guest_exports_round_trip() {
    let e = echo();
    let h = &e.h;
    let panel = op(h, json!({"op": "render", "title": "T"}), false)
        .as_u64()
        .unwrap() as u32;
    assert!(e.app.wait_for(|o| matches!(o, PluginOutput::PanelOpened { contribution, .. } if contribution.as_str() == "test.echo/main")));
    // The host validated the view and keeps it; the app got it in full.
    assert!(e.app.wait_for(
        |o| matches!(o, PluginOutput::PanelView { changes, panel: p, .. } if *p == panel && changes.full)
    ));
    let view = h.panel_view(panel).expect("rendered").to_view();
    let kinds: std::collections::HashSet<_> = view
        .nodes
        .iter()
        .map(|n| std::mem::discriminant(&n.widget))
        .collect();
    assert_eq!(kinds.len(), 19, "every widget variant");
    assert!(
        view.nodes
            .iter()
            .any(|n| matches!(&n.widget, Widget::Split(s) if (s.ratio - 0.25).abs() < 1e-6))
    );
    // A second render with a new title is a single patch: one dirty node.
    op(h, json!({"op": "render", "title": "U"}), false);
    assert!(e.app.wait_for(
        |o| matches!(o, PluginOutput::PanelView { snapshot, changes, .. }
        if !changes.full && changes.dirty.len() == 1
            && matches!(snapshot.node(changes.dirty[0]).map(|n| &n.widget),
                Some(Widget::Text(t)) if t.text == "U"))
    ));

    let info = DocInfo {
        id: 7,
        path: Some("a.echo".into()),
        language: "echo".into(),
        version: 3,
        line_count: 10,
    };
    let lenses = h.dispatch(Call::CodeLenses(info), false).wait();
    let Ok(CallOutput::Lenses(lenses)) = lenses else {
        panic!("{lenses:?}")
    };
    // `args` is JSON text whose key order depends on whether serde_json's
    // `preserve_order` feature is on in the guest build, so compare parsed.
    let [lens] = lenses.as_slice() else {
        panic!("{lenses:?}")
    };
    assert_eq!(
        (lens.line, lens.title.as_str(), lens.command.as_str()),
        (9, "7|Some(\"a.echo\")|echo|3|10", "echo")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&lens.args).ok(),
        Some(json!({"op": "log", "message": "lens"}))
    );

    let (view_panel, p) = h.open_view("echo-file", r#"{"path":"a.echo-view"}"#);
    assert_eq!(p.wait(), Ok(CallOutput::Done));
    let opened = last_with(h, "export", "open-view");
    assert_eq!(
        opened,
        json!({"export": "open-view", "panel": view_panel, "contribution": "echo-file", "args": "{\"path\":\"a.echo-view\"}"})
    );

    let kinds = [
        EventKind::Click,
        EventKind::Submit,
        EventKind::Change("txt".into()),
        EventKind::Toggle(true),
        EventKind::Select(4),
        EventKind::Tab(1),
        EventKind::Sort((2, false)),
        EventKind::RowActivate(u64::MAX),
        EventKind::TreeToggle("root".into()),
        EventKind::TreeActivate("child".into()),
    ];
    for kind in kinds {
        let expected = format!("{kind:?}");
        let event = Event {
            node: 3,
            kind,
            values: vec![(5, "SELECT 1".into())],
        };
        let r = h.dispatch(Call::UiEvent { panel, event }, true).wait();
        assert_eq!(r, Ok(CallOutput::Done));
        let got = last_with(h, "export", "handle-ui-event");
        assert_eq!(got["kind"], expected.as_str());
        assert_eq!(got["values"], json!([[5, "SELECT 1"]]));
    }

    let rows = h
        .dispatch(
            Call::FetchRows {
                panel,
                table: 9,
                start: 98,
                count: 5,
            },
            false,
        )
        .wait();
    let Ok(CallOutput::Rows(page)) = rows else {
        panic!("{rows:?}")
    };
    assert_eq!((page.start, page.rows.len(), page.at_end), (98, 2, true));
    assert_eq!(
        page.rows[0],
        [
            Cell::Empty,
            Cell::Int(-98),
            Cell::Real(49.0),
            Cell::Text("row 98".into()),
            Cell::Boolean(true),
            Cell::Blob(980)
        ]
    );

    assert_eq!(h.close_view(view_panel).wait(), Ok(CallOutput::Done));
    assert_eq!(last_with(h, "export", "view-closed")["panel"], view_panel);
    let r = command(h, "echo", json!({"op": "fail", "message": "nope"}), false);
    assert_eq!(r, Err(CallError::Plugin("nope".into())));
    let r = command(h, "missing", json!({}), false);
    assert_eq!(
        r,
        Err(CallError::Plugin("unknown command `missing`".into()))
    );

    h.shutdown();
    assert!(h.wait_stopped(std::time::Duration::from_secs(5)));
    assert_eq!(
        json_lines(h).last().unwrap(),
        &json!({"export": "deactivate"})
    );
}
