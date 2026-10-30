//! The step-10 end-to-end tests (harness in `mod.rs`).

use std::time::{Duration, Instant};

use arcode_plugin_host::bindings::ui::{Tone, Widget};
use arcode_plugin_host::consent::{ConsentAnswer, ConsentKind, ScriptedConsent};
use arcode_plugin_host::net::NetworkOutcome;
use arcode_plugin_host::{CallOutput, PluginOutput};
use serde_json::{Value, json};

use super::{ALLOW_PROJECT, DOC, client, routes};
use crate::loopback::Server;

#[test]
fn lenses_send_and_render_a_json_response() {
    let c = client(
        ScriptedConsent::always(ALLOW_PROJECT),
        Server::start(routes),
    );
    let lenses = c.lenses();
    let lines: Vec<u32> = lenses.iter().map(|l| l.0).collect();
    assert_eq!(lines, [4, 11, 14, 17]);
    assert!(lenses.iter().all(|l| l.1 == "▶ Send request"));
    let args: Value = serde_json::from_str(&lenses[0].2).unwrap();
    assert_eq!(args, json!({"doc": DOC, "line": 4}));

    c.select_environment();
    assert!(c.saw(
        |o| matches!(o, PluginOutput::Status { text: Some(t), .. } if t == "HTTP env: local")
    ));
    let started = Instant::now();
    assert_eq!(c.send(4).wait(), Ok(CallOutput::Done));
    let elapsed = started.elapsed();

    // What the server received: variables, env and private env resolved.
    let seen = c.server.requests();
    assert_eq!(seen.len(), 1);
    let s = &seen[0];
    assert_eq!((s.method.as_str(), s.path.as_str()), ("POST", "/items?x=1"));
    assert_eq!(s.header("authorization"), Some("Bearer s3cret"));
    assert_eq!(s.header("content-type"), Some("application/json"));
    let body: Value = serde_json::from_slice(&s.body).unwrap();
    assert_eq!(body["name"], "widget");
    assert_eq!(body["id"].as_str().map(str::len), Some(36), "a fresh $uuid");

    // Consent: the workspace (combined wording) at activation, then the host.
    let kinds: Vec<ConsentKind> = c.consent.asked().into_iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [
            ConsentKind::Workspace {
                write: false,
                network: true
            },
            ConsentKind::Network {
                method: "POST".into(),
                scheme: "http".into(),
                host: "127.0.0.1".into(),
                port: c.server.port,
            },
        ]
    );
    assert!(c.saw(
        |o| matches!(o, PluginOutput::TaskStarted { title, .. } if title == "Sending create")
    ));

    // The panel: status, timing, size, JSON viewer, headers.
    let w = c.panel();
    assert!(
        w.iter().any(
            |w| matches!(w, Widget::Badge(b) if b.text == "201 Created" && b.tone == Tone::Success)
        ),
        "{w:?}"
    );
    assert!(w.contains(&Widget::Json(r#"{"created": true}"#.into())));
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::Text(t) if t.text == "17 B"))
    );
    let timing = w
        .iter()
        .find_map(|w| match w {
            Widget::Text(t) if t.text.ends_with(" ms)") => Some(t.text.clone()),
            _ => None,
        })
        .expect("timing text");
    let numbers: Vec<u128> = timing
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse().ok())
        .collect();
    let [total, headers] = numbers[..] else {
        panic!("{timing}")
    };
    assert!(headers <= total, "{timing}");
    assert!(
        headers >= 20,
        "the server waits 20 ms before answering: {timing}"
    );
    assert!(total <= elapsed.as_millis(), "{timing} vs {elapsed:?}");
    let header_rows = w.iter().find_map(|w| match w {
        Widget::KeyValues(kv) if kv.iter().any(|(k, _)| k == "x-reply") => Some(kv.clone()),
        _ => None,
    });
    assert!(header_rows.is_some_and(|kv| kv.contains(&("x-reply".into(), "yes".into()))));
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::Code(c) if c.text.starts_with("HTTP/1.1 201 Created\n")))
    );
    // History has the exchange; the network log has the status.
    assert!(w.iter().any(|w| matches!(w, Widget::KeyValues(kv)
        if kv.iter().any(|(k, v)| k.ends_with(&format!("POST {}", c.server.url("/items?x=1"))) && v.starts_with("201 · ")))));
    let log: Vec<NetworkOutcome> = c
        .instance()
        .network_log()
        .entries()
        .into_iter()
        .map(|e| e.outcome)
        .collect();
    assert_eq!(log, [NetworkOutcome::Status(201)]);
}

#[test]
fn non_json_and_sniffed_json_bodies() {
    let c = client(
        ScriptedConsent::always(ALLOW_PROJECT),
        Server::start(routes),
    );
    c.select_environment();
    assert_eq!(c.send(11).wait(), Ok(CallOutput::Done));
    let w = c.panel();
    assert!(w.iter().any(
        |w| matches!(w, Widget::Badge(b) if b.text == "404 Not Found" && b.tone == Tone::Warning)
    ));
    assert!(w.iter().any(|w| matches!(w, Widget::Code(c)
        if c.text == "<h1>missing</h1>" && c.language.as_deref() == Some("html"))));
    assert!(!w.iter().any(|w| matches!(w, Widget::Json(_))));
    assert_eq!(c.send(14).wait(), Ok(CallOutput::Done));
    let w = c.panel();
    assert!(
        w.contains(&Widget::Json(r#"[{"sniffed": true}]"#.into())),
        "text/plain JSON is sniffed"
    );
    // One network prompt for the host, one workspace prompt.
    assert_eq!(c.consent.asked().len(), 2);
}

#[test]
fn cancel_during_a_slow_response() {
    let c = client(
        ScriptedConsent::always(ALLOW_PROJECT),
        Server::start(routes),
    );
    c.select_environment();
    let pending = c.send(17);
    let deadline = Instant::now() + Duration::from_secs(10);
    while c.server.requests().is_empty() {
        assert!(Instant::now() < deadline, "the request never arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(50));
    let cancelled = Instant::now();
    pending.cancel.cancel();
    assert_eq!(
        pending.wait(),
        Ok(CallOutput::Done),
        "the plugin handles the cancel"
    );
    let took = cancelled.elapsed();
    eprintln!("http-client: cancel returned after {took:?}");
    assert!(took < Duration::from_millis(500), "cancel took {took:?}");
    let w = c.panel();
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::Badge(b) if b.text == "Cancelled")),
        "{w:?}"
    );
    assert!(
        w.iter().any(
            |w| matches!(w, Widget::KeyValues(kv) if kv.iter().any(|(_, v)| v == "cancelled"))
        )
    );
    assert!(c.saw(|o| matches!(o, PluginOutput::TaskFinished { .. })));
}

#[test]
fn denied_consent_sends_nothing() {
    let consent = ScriptedConsent::new([ALLOW_PROJECT, ConsentAnswer::Deny]);
    let c = client(consent, Server::start(routes));
    c.select_environment();
    assert_eq!(c.send(4).wait(), Ok(CallOutput::Done));
    assert_eq!(c.server.connections(), 0, "the server saw no connection");
    let w = c.panel();
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::Badge(b) if b.text == "Failed" && b.tone == Tone::Danger))
    );
    assert!(
        w.iter().any(|w| matches!(w, Widget::Text(t)
        if t.text.contains("not allowed") && t.text.contains("Permissions"))),
        "{w:?}"
    );
    // Deny is remembered for the project: no second prompt, still nothing sent.
    assert_eq!(c.send(14).wait(), Ok(CallOutput::Done));
    assert_eq!(c.consent.asked().len(), 2);
    assert_eq!(c.server.connections(), 0);
    let log: Vec<NetworkOutcome> = c
        .instance()
        .network_log()
        .entries()
        .into_iter()
        .map(|e| e.outcome)
        .collect();
    assert_eq!(log.len(), 2);
    assert!(
        log.iter().all(|o| matches!(o, NetworkOutcome::Refused(_))),
        "{log:?}"
    );
}
