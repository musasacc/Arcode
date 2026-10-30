//! Security review: consent prompts for `http.send`. Loopback only; no
//! test touches the external network. A failing test is an open finding.
//!
//! Needs the fixtures: run `scripts/build-test-plugins.sh` first.

#[allow(dead_code)]
#[path = "sandbox/common.rs"]
mod common;
#[allow(dead_code)]
#[path = "sandbox/loopback.rs"]
mod loopback;

use std::sync::Arc;

use arcode_plugin_host::bridge::NoEditor;
use arcode_plugin_host::consent::{ConsentAnswer, ConsentKind, ScriptedConsent};
use arcode_plugin_host::grants::GrantScope;
use arcode_plugin_host::net::{TransportOptions, UreqTransport};
use arcode_plugin_host::{CallOutput, InstanceHandle};
use serde_json::{Value, json};

use common::*;
use loopback::{Reply, Server};

const ALLOW_ONCE: ConsentAnswer = ConsentAnswer::Allow {
    scope: GrantScope::Once,
    write: false,
};

fn net(consent: Arc<ScriptedConsent>) -> (InstanceHandle, App, tempfile::TempDir) {
    let app = App::start(None);
    let transport = UreqTransport::new(&TransportOptions {
        use_env_proxy: false,
        ..TransportOptions::default()
    });
    let svc = services(&app, consent, Arc::new(NoEditor), Arc::new(transport));
    let (rt, cache) = runtime(|_| {});
    (handle(&rt, fixture("net"), svc, None), app, cache)
}

fn send(h: &InstanceHandle, args: Value) -> Value {
    assert_eq!(command(h, "send", args, true), Ok(CallOutput::Done));
    last_with(h, "op", "send")["result"].clone()
}

/// An HTTP method token (RFC 9110 `token`).
fn is_token(method: &str) -> bool {
    !method.is_empty()
        && method
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Finding: the method shown in the host-drawn network prompt is the
/// plugin's raw `request.method` (only upper-cased): not validated, not
/// quoted, not length-capped, newlines kept. The prompt is asked before
/// the transport rejects the method, so a plugin can make the host chrome
/// say whatever it wants next to an innocent-looking host. §4: "The host,
/// method and port shown come from the parsed URL, not from plugin text.
/// Plugin-supplied strings appear only quoted, as plain text, and are
/// length-capped."
#[test]
fn the_network_prompt_never_shows_raw_plugin_text_as_the_method() {
    let server = Server::start(|_| Reply::text(200, "ok"));
    let consent = Arc::new(ScriptedConsent::new([ConsentAnswer::Deny]));
    let (h, _app, _cache) = net(consent.clone());
    let spoof = "GET request to https://api.github.com:443 (already approved).\n\
                 Click Allow to continue. Details: POST";
    let r = send(&h, json!({"url": server.url("/"), "method": spoof}));
    assert!(r["err"].is_string(), "{r}");
    for asked in consent.asked() {
        let message = asked.message();
        if let ConsentKind::Network { method, .. } = &asked.kind {
            assert!(
                is_token(method) && method.len() <= 32 && !message.contains('\n'),
                "prompt shows plugin text as the method: {message:?}"
            );
        }
    }
}

/// Finding: consent for a redirect hop is asked with the *first* hop's
/// scheme (`allow_redirect` only passes host and port), so an
/// `http -> https` (or `https -> http`) redirect is shown with the wrong
/// scheme, e.g. "https://evil.test:80" for a plain-text hop.
#[test]
fn a_redirect_prompt_shows_the_redirect_targets_scheme() {
    let server = Server::start(|s| match s.path.as_str() {
        "/up" => Reply::redirect(302, "https://localhost:9/next"),
        _ => Reply::text(200, "ok"),
    });
    let consent = Arc::new(ScriptedConsent::new([ALLOW_ONCE, ConsentAnswer::Deny]));
    let (h, _app, _cache) = net(consent.clone());
    let r = send(&h, json!({"url": server.url("/up")}));
    assert_eq!(r, json!({"err": "HttpError::Denied"}));
    let asked = consent.asked();
    assert_eq!(asked.len(), 2, "{asked:?}");
    match &asked[1].kind {
        ConsentKind::Network { scheme, host, .. } => {
            assert_eq!(host, "localhost");
            assert_eq!(scheme, "https", "redirect prompt: {:?}", asked[1].message());
        }
        other => panic!("unexpected prompt {other:?}"),
    }
}
