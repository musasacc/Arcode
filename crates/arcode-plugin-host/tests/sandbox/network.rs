//! Step 7: the `http` import end to end, with the real ureq transport
//! against a loopback server (never the external network).

use std::sync::Arc;
use std::time::{Duration, Instant};

use arcode_plugin_host::bridge::NoEditor;
use arcode_plugin_host::consent::{ConsentAnswer, ConsentKind, ScriptedConsent};
use arcode_plugin_host::grants::{Capability, GrantScope, GrantState};
use arcode_plugin_host::net::{NetworkOutcome, TransportOptions, UreqTransport};
use arcode_plugin_host::{Call, CallOutput, InstanceHandle, Runtime};
use serde_json::{Value, json};

use crate::common::*;
use crate::loopback::{Reply, Server};

const ALLOW_ONCE: ConsentAnswer = ConsentAnswer::Allow {
    scope: GrantScope::Once,
    write: false,
};
const ALLOW_ALWAYS: ConsentAnswer = ConsentAnswer::Allow {
    scope: GrantScope::Global,
    write: false,
};

struct Net {
    h: InstanceHandle,
    rt: Arc<Runtime>,
    consent: Arc<ScriptedConsent>,
    services: arcode_plugin_host::Services,
    _app: App,
    _cache: tempfile::TempDir,
}

fn net(consent: ScriptedConsent, activate_url: Option<String>) -> Net {
    let app = App::start(None);
    let consent = Arc::new(consent);
    let transport = UreqTransport::new(&TransportOptions {
        use_env_proxy: false,
        ..TransportOptions::default()
    });
    let services = services(
        &app,
        consent.clone(),
        Arc::new(NoEditor),
        Arc::new(transport),
    );
    let (rt, cache) = runtime(|_| {});
    let mut spec = spec(fixture("net"), services.clone(), None);
    if let Some(url) = activate_url {
        spec.user_settings
            .insert("activate-url".into(), Value::from(url));
    }
    let h = InstanceHandle::new(rt.clone(), spec);
    Net {
        h,
        rt,
        consent,
        services,
        _app: app,
        _cache: cache,
    }
}

/// Runs `send` as a user gesture and returns its JSON result.
fn send(h: &InstanceHandle, args: Value) -> Value {
    assert_eq!(command(h, "send", args, true), Ok(CallOutput::Done));
    last_with(h, "op", "send")["result"].clone()
}

#[test]
fn no_request_without_a_user_gesture() {
    let server = Server::start(|_| Reply::text(200, "hi"));
    let n = net(
        ScriptedConsent::always(ALLOW_ALWAYS),
        Some(server.url("/from-activate")),
    );
    let r = send(&n.h, json!({"url": server.url("/ok")}));
    assert_eq!(r["status"], 200);
    let activate = last_with(&n.h, "export", "activate");
    assert_eq!(activate["send"], json!({"err": "HttpError::NoUserGesture"}));
    let paths: Vec<_> = server.requests().into_iter().map(|s| s.path).collect();
    assert_eq!(paths, ["/ok"], "only the gesture call reached the server");
    // A non-gesture command call is refused too.
    let r = command(&n.h, "send", json!({"url": server.url("/x")}), false);
    assert_eq!(r, Ok(CallOutput::Done));
    assert_eq!(
        last_with(&n.h, "op", "send")["result"],
        json!({"err": "HttpError::NoUserGesture"})
    );
    let outcomes: Vec<_> =
        n.h.network_log()
            .entries()
            .into_iter()
            .map(|e| e.outcome)
            .collect();
    assert_eq!(
        outcomes,
        [
            NetworkOutcome::Refused("no user gesture".into()),
            NetworkOutcome::Status(200),
            NetworkOutcome::Refused("no user gesture".into()),
        ]
    );
}

#[test]
fn a_response_round_trips_with_headers_and_timing() {
    let server = Server::start(|s| {
        let mut r = Reply::text(
            201,
            &format!("{} {}", s.method, String::from_utf8_lossy(&s.body)),
        );
        r.headers.push(("x-reply".into(), "yes".into()));
        r
    });
    let n = net(ScriptedConsent::always(ALLOW_ALWAYS), None);
    let r = send(
        &n.h,
        json!({"url": server.url("/echo?q=1"), "method": "put", "body": "ping",
               "headers": {"x-test": "1"}}),
    );
    assert_eq!(r["status"], 201);
    assert_eq!(r["reason"], "Created", "canonical reason");
    assert_eq!(r["version"], "HTTP/1.1");
    assert_eq!(r["body"], "PUT ping");
    assert_eq!(r["final-url"], server.url("/echo?q=1"));
    let headers = r["headers"].as_array().unwrap();
    assert!(headers.contains(&json!(["x-reply", "yes"])), "{headers:?}");
    let timing = r["timing"].as_array().unwrap();
    assert!(timing[0].as_u64() <= timing[1].as_u64());
    let seen = &server.requests()[0];
    assert_eq!(
        (seen.method.as_str(), seen.path.as_str()),
        ("PUT", "/echo?q=1")
    );
    assert_eq!(seen.header("x-test"), Some("1"));
    assert!(seen.header("user-agent").unwrap().starts_with("Arcode/"));
    // The consent prompt showed host data parsed from the URL.
    let asked = n.consent.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].kind,
        ConsentKind::Network {
            method: "PUT".into(),
            scheme: "http".into(),
            host: "127.0.0.1".into(),
            port: server.port,
        }
    );
}

#[test]
fn consent_once_deny_and_always() {
    let server = Server::start(|_| Reply::text(200, "ok"));
    let url = json!({"url": server.url("/")});
    // Once: allowed this time, asked again next time; Deny is remembered.
    let n = net(
        ScriptedConsent::new([ALLOW_ONCE, ConsentAnswer::Deny]),
        None,
    );
    assert_eq!(send(&n.h, url.clone())["status"], 200);
    assert_eq!(send(&n.h, url.clone()), json!({"err": "HttpError::Denied"}));
    assert_eq!(send(&n.h, url.clone()), json!({"err": "HttpError::Denied"}));
    assert_eq!(n.consent.asked().len(), 2, "deny is not asked again");
    assert_eq!(server.requests().len(), 1);
    // Always: asked once, stored globally.
    let n = net(ScriptedConsent::new([ALLOW_ALWAYS]), None);
    assert_eq!(send(&n.h, url.clone())["status"], 200);
    assert_eq!(send(&n.h, url.clone())["status"], 200);
    assert_eq!(n.consent.asked().len(), 1);
    let grants = n
        .services
        .permissions
        .store
        .lock()
        .unwrap()
        .grants()
        .list(n.h.plugin().id());
    assert_eq!(
        grants,
        [(
            Capability::Network {
                host: "127.0.0.1".into()
            },
            GrantState::Allow,
            GrantScope::Global
        )]
    );
}

#[test]
fn redirects_re_ask_for_new_hosts() {
    let other = Server::start(|_| Reply::text(200, "elsewhere"));
    let other_port = other.port;
    let server = Server::start(move |s| match s.path.as_str() {
        "/start" => Reply::redirect(302, "/final"),
        "/see-other" => Reply::redirect(303, "final"),
        "/away" => Reply::redirect(307, &format!("http://localhost:{other_port}/x")),
        _ => Reply::text(200, &format!("{} {}", s.method, s.path)),
    });
    let project = ConsentAnswer::Allow {
        scope: GrantScope::Project,
        write: false,
    };
    let n = net(ScriptedConsent::new([project, ConsentAnswer::Deny]), None);
    let r = send(&n.h, json!({"url": server.url("/start")}));
    assert_eq!(
        (r["status"].clone(), r["body"].clone()),
        (json!(200), json!("GET /final"))
    );
    assert_eq!(r["final-url"], server.url("/final"));
    // 303 after a POST: a body-less GET.
    let r = send(
        &n.h,
        json!({"url": server.url("/see-other"), "method": "POST", "body": "x"}),
    );
    assert_eq!(r["body"], "GET /final");
    assert_eq!(n.consent.asked().len(), 1, "same host: no new prompt");
    // A redirect to another host asks again; denied, nothing is sent there.
    let r = send(&n.h, json!({"url": server.url("/away")}));
    assert_eq!(r, json!({"err": "HttpError::Denied"}));
    let asked = n.consent.asked();
    assert_eq!(asked.len(), 2);
    assert!(
        matches!(&asked[1].kind, ConsentKind::Network { host, port, .. }
        if host == "localhost" && *port == other_port)
    );
    assert_eq!(other.connections(), 0);
    // Not following: the 3xx itself comes back, no prompt.
    let r = send(&n.h, json!({"url": server.url("/away"), "follow": false}));
    assert_eq!(r["status"], 307);
    assert_eq!(n.consent.asked().len(), 2);
}

#[test]
fn cancel_during_a_slow_response_returns_promptly() {
    let _t = timing_lock();
    let server = Server::start(|_| Reply {
        delay: Duration::from_secs(5),
        ..Reply::text(200, "late")
    });
    let n = net(ScriptedConsent::always(ALLOW_ALWAYS), None);
    let call = Call::Command {
        command: "send".into(),
        args: json!({"url": server.url("/slow")}).to_string(),
    };
    let pending = n.h.dispatch(call, true);
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.requests().is_empty() {
        assert!(Instant::now() < deadline, "request never arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(50));
    let cancelled_at = Instant::now();
    pending.cancel.cancel();
    assert_eq!(pending.wait(), Ok(CallOutput::Done));
    let took = cancelled_at.elapsed();
    assert!(took < Duration::from_millis(100), "cancel took {took:?}");
    let line = last_with(&n.h, "op", "send");
    assert_eq!(line["result"], json!({"err": "HttpError::Cancelled"}));
    assert_eq!(line["cancelled"], true);
}

#[test]
fn the_body_cap_sets_truncated() {
    let server = Server::start(|_| Reply::text(200, &"z".repeat(10_000)));
    let n = net(ScriptedConsent::always(ALLOW_ALWAYS), None);
    let url = server.url("/big");
    let r = send(&n.h, json!({"url": url, "max": 1000}));
    assert_eq!(
        (r["length"].clone(), r["truncated"].clone()),
        (json!(1000), json!(true))
    );
    let r = send(&n.h, json!({"url": url, "max": 10_000}));
    assert_eq!(
        (r["length"].clone(), r["truncated"].clone()),
        (json!(10_000), json!(false))
    );
    let r = send(&n.h, json!({"url": url}));
    assert_eq!(
        (r["length"].clone(), r["truncated"].clone()),
        (json!(10_000), json!(false))
    );
}

#[test]
fn the_master_switch_denies_everything() {
    let server = Server::start(|_| Reply::text(200, "ok"));
    let n = net(ScriptedConsent::always(ALLOW_ALWAYS), None);
    n.rt.set_network_enabled(false);
    let r = send(&n.h, json!({"url": server.url("/")}));
    assert_eq!(r, json!({"err": "HttpError::Denied"}));
    assert!(
        n.consent.asked().is_empty(),
        "no prompt while networking is off"
    );
    assert_eq!(server.connections(), 0);
    n.rt.set_network_enabled(true);
    assert_eq!(send(&n.h, json!({"url": server.url("/")}))["status"], 200);
}

#[test]
fn transport_errors_are_typed() {
    // A port nobody listens on (bound, then closed).
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let n = net(ScriptedConsent::always(ALLOW_ALWAYS), None);
    let r = send(&n.h, json!({"url": format!("http://127.0.0.1:{port}/")}));
    assert!(
        r["err"].as_str().unwrap().starts_with("HttpError::Connect"),
        "{r}"
    );
    let silent = Server::start(|_| Reply {
        delay: Duration::from_secs(5),
        ..Reply::text(200, "late")
    });
    let r = send(&n.h, json!({"url": silent.url("/"), "timeout": 200}));
    assert_eq!(r, json!({"err": "HttpError::Timeout"}));
    let r = send(
        &n.h,
        json!({"url": silent.url("/"), "method": "BAD METHOD"}),
    );
    assert!(
        r["err"]
            .as_str()
            .unwrap()
            .starts_with("HttpError::InvalidRequest"),
        "{r}"
    );
}
