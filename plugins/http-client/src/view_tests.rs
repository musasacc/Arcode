//! `view` tests: the element trees for each outcome.

use super::*;

/// Every widget of a tree, depth first (through the SDK's flatten).
fn flat(e: &Element) -> Vec<Widget> {
    let mut ids = IdMap::new();
    let view = flatten(e, &mut ids).expect("valid tree");
    let mut out = Vec::new();
    collect(&view, view.root, &mut out);
    out
}

fn collect(view: &FlatView, id: NodeId, out: &mut Vec<Widget>) {
    let node = &view.nodes[&id];
    out.push(node.widget.clone());
    for &c in &node.children {
        collect(view, c, out);
    }
}

fn response(status: u16, content_type: &str, body: &[u8]) -> http::Response {
    http::Response {
        status,
        reason: "Reason".into(),
        version: "HTTP/1.1".into(),
        final_url: "http://x/".into(),
        remote_addr: None,
        headers: vec![("content-type".into(), content_type.into())],
        body: body.to_vec(),
        truncated: false,
        timing: http::Timing {
            headers_ms: 3,
            total_ms: 9,
        },
    }
}

fn exchange(outcome: Outcome) -> Exchange {
    Exchange {
        name: None,
        method: "GET".into(),
        url: "http://x/".into(),
        outcome,
    }
}

#[test]
fn a_json_response_has_badge_timing_tabs_and_json_viewer() {
    let ex = exchange(Outcome::Response(response(
        200,
        "application/json",
        b"{\"a\":1}",
    )));
    let w = flat(&render(&ex, 0, &[]));
    assert!(w.contains(&Widget::Badge(BadgeProps {
        text: "200 Reason".into(),
        tone: Tone::Success
    })));
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::Text(t) if t.text == "9 ms (headers 3 ms)"))
    );
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::Text(t) if t.text == "7 B"))
    );
    assert!(w.contains(&Widget::Json("{\"a\":1}".into())));
    assert!(w.iter().any(
        |w| matches!(w, Widget::Tabs(t) if t.titles == ["Body", "Headers", "Raw", "History"])
    ));
    assert!(w.contains(&Widget::KeyValues(vec![(
        "content-type".into(),
        "application/json".into()
    )])));
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::Button(b) if b.label == "Open in editor" && b.enabled))
    );
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::Code(c) if c.text.starts_with("HTTP/1.1 200 Reason\n")))
    );
}

#[test]
fn bodies_by_kind() {
    let html = flat(&render(
        &exchange(Outcome::Response(response(404, "text/html", b"<p>"))),
        1,
        &[],
    ));
    assert!(html.iter().any(
        |w| matches!(w, Widget::Code(c) if c.text == "<p>" && c.language.as_deref() == Some("html"))
    ));
    assert!(
        html.iter()
            .any(|w| matches!(w, Widget::Badge(b) if b.tone == Tone::Warning))
    );
    assert!(
        html.iter()
            .any(|w| matches!(w, Widget::Tabs(t) if t.active == 1))
    );
    let bin = flat(&render(
        &exchange(Outcome::Response(response(500, "image/png", &[0x89, 0x50]))),
        0,
        &[],
    ));
    assert!(
        bin.iter()
            .any(|w| matches!(w, Widget::Code(c) if c.text.starts_with("00000000  89 50")))
    );
    assert!(
        bin.iter()
            .any(|w| matches!(w, Widget::Badge(b) if b.tone == Tone::Danger))
    );
    let empty = flat(&render(
        &exchange(Outcome::Response(response(204, "", b""))),
        0,
        &[],
    ));
    assert!(
        empty
            .iter()
            .any(|w| matches!(w, Widget::EmptyState((t, _)) if t == "No body"))
    );
    assert!(
        empty
            .iter()
            .any(|w| matches!(w, Widget::Button(b) if b.label == "Open in editor" && !b.enabled))
    );
}

#[test]
fn large_bodies_are_cut_in_the_panel() {
    let big = format!("[{}]", "1,".repeat(MAX_BODY_VIEW)).replace(",]", "]");
    let ex = exchange(Outcome::Response(response(
        200,
        "application/json",
        big.as_bytes(),
    )));
    let w = flat(&render(&ex, 0, &[]));
    assert!(!w.iter().any(|w| matches!(w, Widget::Json(_))));
    let code = w.iter().find_map(|w| match w {
        Widget::Code(c) if c.language.as_deref() == Some("json") => Some(c.text.len()),
        _ => None,
    });
    assert_eq!(code, Some(MAX_BODY_VIEW));
    let strings: usize = w.iter().map(|w| format!("{w:?}").len()).sum();
    assert!(strings < 8 * 1024 * 1024, "{strings}");
}

#[test]
fn sending_failed_and_cancelled_states() {
    let w = flat(&render(&exchange(Outcome::Sending), 0, &[]));
    assert!(w.contains(&Widget::Spinner("Sending GET http://x/…".into())));
    let w = flat(&render(
        &exchange(Outcome::Failed("no route".into())),
        0,
        &[],
    ));
    assert!(
        w.iter().any(
            |w| matches!(w, Widget::Text(t) if t.text == "no route" && t.tone == Tone::Danger)
        )
    );
    let w = flat(&render(&exchange(Outcome::Cancelled), 0, &[]));
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::Badge(b) if b.text == "Cancelled"))
    );
    let w = flat(&empty());
    assert!(
        w.iter()
            .any(|w| matches!(w, Widget::EmptyState((t, _)) if t == "No response yet"))
    );
}

#[test]
fn history_tab_lists_entries() {
    let entries = [
        Entry {
            time_ms: 1_700_000_000_000,
            name: None,
            method: "GET".into(),
            url: "http://a/".into(),
            status: Some(200),
            error: None,
            total_ms: 5,
            size: 2048,
        },
        Entry {
            time_ms: 0,
            name: None,
            method: "POST".into(),
            url: "http://b/".into(),
            status: None,
            error: Some("cancelled".into()),
            total_ms: 0,
            size: 0,
        },
    ];
    let ex = exchange(Outcome::Response(response(200, "text/plain", b"x")));
    let w = flat(&render(&ex, 3, &entries));
    assert!(w.contains(&Widget::KeyValues(vec![
        (
            "2023-11-14 22:13:20".into(),
            "GET http://a/ → 200 · 5 ms · 2.0 KB".into()
        ),
        (
            "1970-01-01 00:00:00".into(),
            "POST http://b/ → cancelled".into()
        ),
    ])));
    assert_eq!(status_tone(101), Tone::Accent);
    assert_eq!(status_tone(302), Tone::Accent);
}
