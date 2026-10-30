//! `net` fixture: sends HTTP requests through `arcode:plugin/http` so the
//! host's gesture rule, per-host consent, redirect re-checks, cancel and
//! body cap can be tested against a loopback server.
//!
//! - `activate`: if the `activate-url` setting is set, sends a GET there
//!   (outside any gesture, so it must fail) and reports the result.
//! - command `send` with `{"url", "method"?, "body"?, "max"?, "follow"?,
//!   "timeout"?, "headers"?}`: sends one request and reports it.
//!
//! Results are JSON lines in the plugin log:
//! `{"export": "activate", "send": ...}` or `{"op": "send", "result": ...}`.

use arcode_plugin::prelude::*;
use serde_json::{Value, json};

#[derive(Default)]
struct Net;

fn report(value: Value) {
    host::log(host::Level::Info, &value.to_string());
}

fn send(args: &Value) -> Value {
    let headers = args["headers"]
        .as_object()
        .map(|h| {
            h.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                .collect()
        })
        .unwrap_or_default();
    let request = http::Request {
        method: args["method"].as_str().unwrap_or("GET").to_owned(),
        url: args["url"].as_str().unwrap_or_default().to_owned(),
        headers,
        body: args["body"]
            .as_str()
            .unwrap_or_default()
            .as_bytes()
            .to_vec(),
        timeout_ms: args["timeout"].as_u64().unwrap_or(10_000) as u32,
        follow_redirects: args["follow"].as_bool().unwrap_or(true),
        max_response_bytes: args["max"].as_u64().unwrap_or(0),
    };
    match http::send(&request) {
        Ok(r) => json!({
            "status": r.status,
            "reason": r.reason,
            "version": r.version,
            "final-url": r.final_url,
            "headers": r.headers,
            "body": String::from_utf8_lossy(&r.body),
            "length": r.body.len(),
            "truncated": r.truncated,
            "timing": [r.timing.headers_ms, r.timing.total_ms],
        }),
        Err(e) => json!({"err": format!("{e:?}")}),
    }
}

impl Plugin for Net {
    fn activate(&mut self, _reason: Activation) -> Result<()> {
        let settings: Value = host::settings_as()?;
        if let Some(url) = settings["activate-url"].as_str().filter(|u| !u.is_empty()) {
            report(json!({"export": "activate", "send": send(&json!({"url": url}))}));
        }
        Ok(())
    }

    fn command(&mut self, id: &str, args: Json) -> Result<()> {
        if id != "send" {
            return Err(Error::msg(format!("unknown command `{id}`")));
        }
        let args: Value = args.parse()?;
        report(json!({"op": "send", "result": send(&args), "cancelled": host::cancelled()}));
        Ok(())
    }
}

arcode_plugin::export_plugin!(Net);
