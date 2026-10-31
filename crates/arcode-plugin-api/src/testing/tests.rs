//! Tests of the fakes and of SDK code that goes through them ([`Panel`]).

use super::{reset, with};
use crate::editor::{self, EditorError, Position, Range, TextEdit};
use crate::kv::{self, KvError, Scope};
use crate::ui::{self, Panel, Patch, Tone, badge, text, vstack};
use crate::{host, http};

#[test]
fn panel_renders_once_then_patches_only_changes() {
    reset();
    let mut panel = Panel::open("response", "Response", None).unwrap();
    let tree = |status: &str| {
        vstack([
            badge(status, Tone::Success).key("status"),
            text("body").key("body"),
        ])
    };
    panel.rerender(&tree("200")).unwrap();
    panel.rerender(&tree("200")).unwrap();
    panel.rerender(&tree("404")).unwrap();
    with(|h| {
        let p = h.panel("response").unwrap();
        assert_eq!(p.renders, 1);
        assert_eq!(
            p.patch_calls.len(),
            1,
            "unchanged tree must not send anything"
        );
        assert!(matches!(&p.patch_calls[0][..], [Patch::Upsert(_)]));
    });
    let mut ids = ui::IdMap::new();
    let fresh = ui::flatten(&tree("404"), &mut ids).unwrap();
    with(|h| {
        assert_eq!(
            h.panel("response").unwrap().view.to_view().nodes.len(),
            fresh.nodes.len()
        )
    });
    assert!(panel.node_id("status").is_some());
}

#[test]
fn panel_resyncs_with_full_render_after_an_error() {
    reset();
    let mut panel = Panel::open("p", "P", None).unwrap();
    panel.rerender(&text("a")).unwrap();
    with(|h| h.panels.values_mut().for_each(|p| p.closed = true));
    assert!(panel.rerender(&text("b")).is_err());
    with(|h| h.panels.values_mut().for_each(|p| p.closed = false));
    panel.rerender(&text("b")).unwrap();
    with(|h| assert_eq!(h.panel("p").unwrap().renders, 2));
}

#[test]
fn kv_enforces_keys_and_quota_per_scope() {
    reset();
    assert_eq!(kv::set(Scope::Global, "", b"x"), Err(KvError::InvalidKey));
    assert_eq!(
        kv::set(Scope::Global, "bad\nkey", b"x"),
        Err(KvError::InvalidKey)
    );
    let big = vec![0u8; 700 * 1024];
    kv::set(Scope::Global, "a", &big).unwrap();
    assert_eq!(
        kv::set(Scope::Global, "b", &big),
        Err(KvError::QuotaExceeded)
    );
    kv::set(Scope::Project, "b", &big).unwrap();
    kv::set(Scope::Global, "a", &big).unwrap(); // overwriting does not double-count
    assert_eq!(kv::keys(Scope::Global, ""), vec!["a".to_owned()]);
    kv::delete(Scope::Global, "a");
    assert_eq!(kv::get(Scope::Global, "a"), None);
}

#[test]
fn editor_fake_reads_ranges_and_checks_versions() {
    reset();
    let doc = with(|h| h.add_document(Some("a.http"), "http", "GET /\nHost: x\n"));
    let pos = |line, column| Position { line, column };
    assert_eq!(
        editor::text(
            doc,
            Some(Range {
                start: pos(1, 0),
                end: pos(1, 4)
            })
        )
        .unwrap(),
        "Host"
    );
    assert_eq!(
        editor::apply_edits(doc, 7, &[]),
        Err(EditorError::VersionMismatch(1))
    );
    let edit = TextEdit {
        range: Range {
            start: pos(0, 0),
            end: pos(0, 3),
        },
        text: "POST".into(),
    };
    assert_eq!(editor::apply_edits(doc, 1, &[edit]), Ok(2));
    assert!(editor::text(doc, None).unwrap().starts_with("POST /"));
    assert_eq!(
        editor::open_file("../x", None),
        Err(EditorError::InvalidPath)
    );
}

#[test]
fn begin_task_needs_a_gesture_and_http_defaults_to_denied() {
    reset();
    assert!(!host::begin_task("t"));
    with(|h| h.gesture = true);
    assert!(host::begin_task("t"));
    let req = http::Request {
        method: "GET".into(),
        url: "http://127.0.0.1/".into(),
        headers: vec![],
        body: vec![],
        timeout_ms: 1000,
        follow_redirects: true,
        max_response_bytes: 1024,
    };
    assert_eq!(http::send(&req), Err(http::HttpError::Denied));
    with(|h| h.http = Some(Box::new(|_| Err(http::HttpError::Timeout))));
    assert_eq!(http::send(&req), Err(http::HttpError::Timeout));
    with(|h| assert_eq!(h.requests.len(), 2));
}

#[test]
fn settings_parse_into_types() {
    reset();
    with(|h| h.settings = r#"{"timeout-ms": 5}"#.into());
    let v: serde_json::Value = host::settings_as().unwrap();
    assert_eq!(v["timeout-ms"], 5);
}
