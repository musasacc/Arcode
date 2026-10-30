//! The plugin end to end against the SDK's fake host (native `cargo test`).

use arcode_plugin::http::{HttpError, Request, Response, Timing};
use arcode_plugin::testing::{self, FakePanel};
use arcode_plugin::ui::{Event, EventKind, Tone, Widget};
use arcode_plugin::{Activation, Json, Plugin, kv};

use crate::test_support::TempDir;
use crate::{HttpClient, LENS_TITLE, SendArgs, view};

const FILE: &str = "\
@path = /users

### List users
GET {{base}}{{path}}?id={{$randomInt 1 2}}
Accept: application/json
Authorization: Bearer {{token}}

### Create
# @name create
POST {{base}}{{path}}
Content-Type: application/json

{\"name\": \"{{who}}\"}
";

/// A project with environment files, a document, and a gesture call.
struct Setup {
    plugin: HttpClient,
    doc: u64,
    _dir: TempDir,
}

fn setup() -> Setup {
    testing::reset();
    let dir = TempDir::new("plugin");
    std::fs::create_dir_all(dir.path().join("api")).unwrap();
    std::fs::write(
        dir.path().join("http-client.env.json"),
        r#"{"dev": {"base": "http://127.0.0.1:9", "who": "ann"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("http-client.private.env.json"),
        r#"{"dev": {"token": "t0k"}}"#,
    )
    .unwrap();
    let doc = testing::with(|h| {
        h.gesture = true;
        h.add_document(Some("api/users.http"), "http", FILE)
    });
    Setup {
        plugin: HttpClient::with_project(dir.path()),
        doc,
        _dir: dir,
    }
}

fn respond(status: u16, content_type: &str, body: &str) {
    let (ct, body) = (content_type.to_owned(), body.as_bytes().to_vec());
    testing::with(|h| {
        h.http = Some(Box::new(move |req: &Request| {
            Ok(Response {
                status,
                reason: "OK".into(),
                version: "HTTP/1.1".into(),
                final_url: req.url.clone(),
                remote_addr: None,
                headers: vec![("Content-Type".into(), ct.clone())],
                body: body.clone(),
                truncated: false,
                timing: Timing {
                    headers_ms: 4,
                    total_ms: 6,
                },
            })
        }));
    });
}

fn send(s: &mut Setup, line: u32) {
    let args = Json::from_value(&SendArgs { doc: s.doc, line });
    s.plugin.command("send", args).unwrap();
}

fn panel() -> FakePanel {
    testing::with(|h| h.panel("response").cloned()).expect("response panel")
}

fn widgets(p: &FakePanel) -> Vec<Widget> {
    p.view.nodes.values().map(|n| n.widget.clone()).collect()
}

fn select_dev(s: &mut Setup) {
    testing::with(|h| h.quick_pick_answers = vec![Some(1)]);
    s.plugin
        .command("select-environment", Json::null())
        .unwrap();
}

#[test]
fn one_lens_per_request() {
    let mut s = setup();
    let info = arcode_plugin::editor::document_info(s.doc).unwrap();
    let lenses = s.plugin.code_lenses(&info);
    assert_eq!(lenses.len(), 2);
    assert_eq!(lenses.iter().map(|l| l.line).collect::<Vec<_>>(), [3, 9]);
    assert!(
        lenses
            .iter()
            .all(|l| l.title == LENS_TITLE && l.command == "send")
    );
    assert_eq!(
        Json::from(lenses[1].args.clone()).parse::<SendArgs>(),
        Ok(SendArgs {
            doc: s.doc,
            line: 9
        })
    );
}

#[test]
fn send_resolves_variables_and_renders_the_response() {
    let mut s = setup();
    select_dev(&mut s);
    assert_eq!(
        testing::with(|h| h.status.clone()),
        Some((
            Some("HTTP env: dev".into()),
            Some("select-environment".into())
        ))
    );
    respond(201, "application/json", r#"{"id": 7}"#);
    send(&mut s, 12);
    let req = testing::with(|h| h.requests[0].clone());
    assert_eq!(req.method, "POST");
    assert_eq!(req.url, "http://127.0.0.1:9/users");
    assert_eq!(req.body, br#"{"name": "ann"}"#);
    assert_eq!(
        req.timeout_ms, 30_000,
        "manifest default (fake settings are {{}})"
    );
    assert_eq!(testing::with(|h| h.tasks.clone()), ["Sending create"]);
    let p = panel();
    assert_eq!(p.title, "Response: create");
    let w = widgets(&p);
    assert!(
        w.iter().any(
            |w| matches!(w, Widget::Badge(b) if b.text == "201 OK" && b.tone == Tone::Success)
        )
    );
    assert!(w.contains(&Widget::Json(r#"{"id": 7}"#.into())));
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::Text(t) if t.text == "6 ms (headers 4 ms)"))
    );
    // Rendered once with a spinner, then patched with the response.
    assert_eq!(p.renders, 1);
    assert_eq!(p.patch_calls.len(), 1);
    let history = crate::history::load();
    assert_eq!(history.len(), 1);
    assert_eq!(
        (history[0].status, history[0].name.as_deref()),
        (Some(201), Some("create"))
    );
}

#[test]
fn headers_get_secrets_from_the_private_env() {
    let mut s = setup();
    select_dev(&mut s);
    respond(200, "text/plain", "ok");
    send(&mut s, 4);
    let req = testing::with(|h| h.requests[0].clone());
    assert_eq!(req.url, "http://127.0.0.1:9/users?id=1");
    assert!(
        req.headers
            .contains(&("Authorization".into(), "Bearer t0k".into()))
    );
}

#[test]
fn without_an_environment_unknown_variables_fail_in_the_panel() {
    let mut s = setup();
    respond(200, "text/plain", "ok");
    send(&mut s, 3);
    assert!(testing::with(|h| h.requests.is_empty()), "nothing sent");
    let w = widgets(&panel());
    assert!(
        w.iter().any(
            |w| matches!(w, Widget::Text(t) if t.text.starts_with("unknown variable {{base}}"))
        )
    );
}

#[test]
fn denied_cancelled_and_cancelled_before_sending() {
    let mut s = setup();
    select_dev(&mut s);
    send(&mut s, 3); // no responder: the fake answers `denied`
    let w = widgets(&panel());
    assert!(w.iter().any(|w| matches!(w, Widget::Text(t) if t.text.contains("not allowed") && t.text.contains("127.0.0.1:9"))));
    testing::with(|h| h.http = Some(Box::new(|_| Err(HttpError::Cancelled))));
    send(&mut s, 3);
    assert!(
        widgets(&panel())
            .iter()
            .any(|w| matches!(w, Widget::Badge(b) if b.text == "Cancelled"))
    );
    testing::with(|h| h.cancelled = true);
    let before = testing::with(|h| h.requests.len());
    send(&mut s, 3);
    assert_eq!(
        testing::with(|h| h.requests.len()),
        before,
        "not sent once cancelled"
    );
    let errors: Vec<_> = crate::history::load()
        .into_iter()
        .map(|e| e.error)
        .collect();
    assert_eq!(errors.len(), 3);
    assert_eq!(errors[0].as_deref(), Some("cancelled"));
}

#[test]
fn send_without_args_uses_the_cursor() {
    let mut s = setup();
    select_dev(&mut s);
    respond(200, "text/plain", "ok");
    testing::with(|h| {
        h.current_document = Some(s.doc);
        let d = h.documents.get_mut(&s.doc).unwrap();
        d.selections = vec![arcode_plugin::editor::Range {
            start: arcode_plugin::editor::Position {
                line: 13,
                column: 0,
            },
            end: arcode_plugin::editor::Position {
                line: 13,
                column: 0,
            },
        }];
    });
    s.plugin.command("send", Json::null()).unwrap();
    assert_eq!(testing::with(|h| h.requests[0].method.clone()), "POST");
    // Outside any request: a notice, nothing sent.
    testing::with(|h| h.documents.get_mut(&s.doc).unwrap().selections.clear());
    s.plugin.command("send", Json::null()).unwrap();
    assert_eq!(
        testing::with(|h| (h.requests.len(), h.notifications.len())),
        (1, 1)
    );
}

#[test]
fn buttons_and_tabs() {
    let mut s = setup();
    select_dev(&mut s);
    respond(200, "application/json", r#"{"a":[1,2]}"#);
    send(&mut s, 3);
    let id = testing::with(|h| *h.panels.keys().next().unwrap());
    let node = |s: &Setup, key: &str| {
        s.plugin
            .panel
            .as_ref()
            .and_then(|p| p.node_id(key))
            .unwrap()
    };
    let (open, again, tabs) = (
        node(&s, view::OPEN_IN_EDITOR),
        node(&s, view::SEND_AGAIN),
        node(&s, view::TABS),
    );
    let click = |node| Event {
        node,
        kind: EventKind::Click,
        values: vec![],
    };
    s.plugin.ui_event(id, click(open)).unwrap();
    let scratch = testing::with(|h| h.scratches[0].clone());
    assert_eq!(scratch.1, "json");
    assert_eq!(scratch.2, "{\n  \"a\": [\n    1,\n    2\n  ]\n}");
    let tab = Event {
        node: tabs,
        kind: EventKind::Tab(2),
        values: vec![],
    };
    s.plugin.ui_event(id, tab).unwrap();
    s.plugin.ui_event(id, click(again)).unwrap();
    assert_eq!(testing::with(|h| h.requests.len()), 2);
    assert!(
        widgets(&panel())
            .iter()
            .any(|w| matches!(w, Widget::Tabs(t) if t.active == 2))
    );
    // Events for other panels are ignored.
    s.plugin.ui_event(id + 1, click(again)).unwrap();
    assert_eq!(testing::with(|h| h.requests.len()), 2);
}

#[test]
fn restore_redraws_and_environment_choices() {
    let mut s = setup();
    s.plugin.activate(Activation::Restore).unwrap();
    assert_eq!(
        testing::with(|h| h.status.clone()),
        Some((
            Some("HTTP env: none".into()),
            Some("select-environment".into())
        ))
    );
    let id = arcode_plugin::ui::Panel::open("response", "Response", None)
        .unwrap()
        .id();
    s.plugin.open_view(id, "response", Json::null()).unwrap();
    assert!(
        widgets(&panel())
            .iter()
            .any(|w| matches!(w, Widget::EmptyState((t, _)) if t == "No response yet"))
    );
    assert!(s.plugin.open_view(id, "other", Json::null()).is_err());
    select_dev(&mut s);
    assert_eq!(
        testing::with(|h| h.quick_picks.clone()),
        ["Select HTTP environment"]
    );
    assert_eq!(crate::env::selected().as_deref(), Some("dev"));
    testing::with(|h| h.quick_pick_answers = vec![Some(0)]);
    s.plugin
        .command("select-environment", Json::null())
        .unwrap();
    assert_eq!(crate::env::selected(), None);
    assert!(kv::keys(kv::Scope::Project, "").is_empty());
    s.plugin.view_closed(id);
    assert!(s.plugin.panel.is_none());
    assert!(s.plugin.command("nope", Json::null()).is_err());
}
