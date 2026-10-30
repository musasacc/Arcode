//! `undeclared-import` fixture: calls `http.send` (so the component imports
//! `arcode:plugin/http`) while its manifest declares no `network`. The host
//! must refuse it at load time (§4 import check).

use arcode_plugin::prelude::*;

#[derive(Default)]
struct Undeclared;

impl Plugin for Undeclared {
    fn command(&mut self, _id: &str, _args: Json) -> Result<()> {
        let request = http::Request {
            method: "GET".into(),
            url: "http://127.0.0.1:9/".into(),
            headers: Vec::new(),
            body: Vec::new(),
            timeout_ms: 1000,
            follow_redirects: false,
            max_response_bytes: 0,
        };
        let _ = http::send(&request);
        Ok(())
    }
}

arcode_plugin::export_plugin!(Undeclared);
