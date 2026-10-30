//! Correctness review: findings against docs/PLUGINS.md §5, §7 and the
//! app guide in README.md. A failing test is an open finding.
//!
//! Needs the fixtures: run `scripts/build-test-plugins.sh` first.

#[allow(dead_code)]
#[path = "sandbox/common.rs"]
mod common;
#[allow(dead_code)]
#[path = "sandbox/loopback.rs"]
mod loopback;

use std::sync::Arc;
use std::time::{Duration, Instant};

use arcode_plugin_host::bindings::ui::{
    Cell, Column, Node, Patch, RowPage, TableProps, View, Widget,
};
use arcode_plugin_host::consent::DenyAll;
use arcode_plugin_host::rows::RowCache;
use arcode_plugin_host::ui_tree::PanelTree;
use arcode_plugin_host::{Call, CallError, CallOutput, Services};
use serde_json::json;

use common::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn plain(app: &App) -> Services {
    services(
        app,
        Arc::new(DenyAll),
        Arc::new(FakeEditor::default()),
        Arc::new(FakeHttp::default()),
    )
}

fn run(args: serde_json::Value) -> Call {
    Call::Command {
        command: "run".into(),
        args: args.to_string(),
    }
}

/// §5 "Cancellation, cooperative first": a cancelled call gets the grace
/// period to stop on its own before the epoch callback traps. The grace is
/// measured from `cancel()`, not from when the call starts, so a call the
/// app cancels while it is still queued (Esc, closing the panel) is
/// hard-killed at its first epoch tick (~10 ms of guest time), with no
/// chance to poll `host.cancelled()`, and the trap counts as a crash
/// (three in ten minutes disable the plugin).
#[test]
fn a_call_cancelled_while_queued_is_not_hard_killed_without_grace() {
    let _serial = timing_lock();
    let app = App::start(None);
    let (rt, _cache) = runtime(|c| {
        c.budgets.command = ms(400);
        c.budgets.cancel_grace = ms(300);
        c.crash.max_crashes = 10;
        c.crash.backoff = Duration::ZERO;
    });
    let spin = handle(&rt, fixture("spin"), plain(&app), None);
    assert_eq!(
        spin.dispatch(run(json!({"noop": true})), false).wait(),
        Ok(CallOutput::Done)
    );
    let first = spin.dispatch(run(json!({})), true);
    let queued = spin.dispatch(run(json!({})), true);
    queued.cancel.cancel();
    assert_eq!(
        first.wait(),
        Err(CallError::Trapped("exceeded 400 ms CPU".into()))
    );
    let started = Instant::now();
    let result = queued.wait();
    let ran = started.elapsed();
    match result {
        Err(CallError::Trapped(reason)) => {
            assert_eq!(reason, "did not stop after cancel");
            assert!(
                ran >= ms(300),
                "a call cancelled before it started was hard-killed {ran:?} after it \
                 started, without the 300 ms cooperative grace"
            );
        }
        // Not running a call that was cancelled before it started is fine too.
        other => eprintln!("queued cancelled call: {other:?}"),
    }
}

fn int_page(start: u64, count: u32, total: u64) -> RowPage {
    let end = (start + u64::from(count)).min(total);
    RowPage {
        start,
        rows: (start..end)
            .map(|i| vec![Cell::Int(i64::try_from(i).unwrap_or(i64::MAX))])
            .collect(),
        at_end: end == total,
    }
}

/// Answers every fetch the cache asks for, from a table of `total` rows.
fn serve(cache: &mut RowCache, total: u64) {
    for _ in 0..100 {
        let Some(request) = cache.next_fetch() else {
            return;
        };
        let page = int_page(request.start, request.count, total);
        assert!(cache.page_arrived(request, page));
    }
    panic!("the cache kept asking for pages");
}

/// README: on a `TableChange` without `reset` the app calls
/// `set_row_count` and keeps the pages (the plugin grew the table, e.g.
/// appended rows, without bumping `generation`). The partial last page
/// cached before the growth stays "present", so its missing rows are
/// never fetched and the grid shows those rows as loading forever.
#[test]
fn rows_of_a_partial_last_page_are_fetched_after_the_table_grows() {
    let mut cache = RowCache::new(Some(250));
    cache.set_viewport(0, 249);
    serve(&mut cache, 250);
    assert!(cache.row(249).is_some());

    cache.set_row_count(Some(450));
    cache.set_viewport(200, 449);
    serve(&mut cache, 450);
    let missing: Vec<u64> = (200..450).filter(|i| cache.row(*i).is_none()).collect();
    assert!(
        missing.is_empty(),
        "{} visible rows never fetched, first {:?}",
        missing.len(),
        missing.first()
    );
}

/// Same with an unknown count: the end learned from an `at-end` page
/// survives `set_row_count(None)`, so the cache never pages past the old
/// end of a table that grew.
#[test]
fn an_unknown_count_after_growth_pages_past_the_old_end() {
    let mut cache = RowCache::new(None);
    cache.set_viewport(0, 199);
    serve(&mut cache, 150);
    assert_eq!(cache.row_count(), Some(150));

    // The plugin appended rows and reports the count as unknown again.
    cache.set_row_count(None);
    cache.set_viewport(0, 299);
    serve(&mut cache, 300);
    let missing: Vec<u64> = (0..300).filter(|i| cache.row(*i).is_none()).collect();
    assert!(
        missing.is_empty(),
        "{} visible rows never fetched, first {:?}; row_count() = {:?}",
        missing.len(),
        missing.first(),
        cache.row_count()
    );
}

fn table_node(id: u32, row_count: Option<u64>) -> Node {
    Node {
        id,
        widget: Widget::Table(TableProps {
            columns: vec![Column {
                title: "n".into(),
                width: None,
                numeric: true,
                sortable: false,
            }],
            row_count,
            generation: 1,
        }),
        children: vec![],
        grow: false,
    }
}

/// `apply-patches` is "validated and applied atomically" and the app gets
/// one consistent `Changes`. With `rows-changed(t, 5)` followed by an
/// `upsert` of the same table (row count 3) in one patch set, the snapshot
/// says 3 rows while the `TableChange` tells the app's row cache 5.
#[test]
fn table_change_row_count_matches_the_snapshot() {
    let mut tree = PanelTree::new();
    tree.render(View {
        root: 1,
        nodes: vec![table_node(1, Some(3))],
    })
    .expect("render");
    let (snapshot, changes) = tree
        .apply(vec![
            Patch::RowsChanged((1, Some(5))),
            Patch::Upsert(table_node(1, Some(3))),
        ])
        .expect("apply");
    let Some(Widget::Table(props)) = snapshot.node(1).map(|n| n.widget.clone()) else {
        panic!("table");
    };
    let change = changes.tables.first().expect("a table change");
    assert_eq!(
        change.row_count, props.row_count,
        "TableChange disagrees with the snapshot"
    );
}

/// §4 "Prompts are host chrome ... The host, method and port shown come
/// from the parsed URL". For a redirect hop the prompt reuses the first
/// hop's scheme and method: a POST to `http://127.0.0.1` answered with
/// `303 → https://localhost/x` asks "POST http://localhost:<port>" although
/// the hop that would be sent is `GET https://localhost:<port>/x`.
#[test]
fn redirect_prompts_show_the_hop_that_would_be_sent() {
    use arcode_plugin_host::consent::{ConsentAnswer, ConsentKind, ScriptedConsent};
    use arcode_plugin_host::grants::GrantScope;
    use arcode_plugin_host::net::{TransportOptions, UreqTransport};
    use loopback::{Reply, Server};

    let server = Server::start(|_| Reply::redirect(303, "https://localhost:1/x"));
    let app = App::start(None);
    let consent = Arc::new(ScriptedConsent::new([
        ConsentAnswer::Allow {
            scope: GrantScope::Once,
            write: false,
        },
        ConsentAnswer::Deny,
    ]));
    let transport = UreqTransport::new(&TransportOptions {
        use_env_proxy: false,
        ..TransportOptions::default()
    });
    let services = services(
        &app,
        consent.clone(),
        Arc::new(FakeEditor::default()),
        Arc::new(transport),
    );
    let (rt, _cache) = runtime(|_| {});
    let h = handle(&rt, fixture("net"), services, None);
    let args = json!({"url": server.url("/form"), "method": "POST", "body": "x"});
    assert_eq!(command(&h, "send", args, true), Ok(CallOutput::Done));
    let asked = consent.asked();
    assert_eq!(asked.len(), 2, "{asked:?}");
    let ConsentKind::Network {
        method,
        scheme,
        host,
        port,
    } = &asked[1].kind
    else {
        panic!("{asked:?}");
    };
    assert_eq!((host.as_str(), *port), ("localhost", 1));
    assert_eq!(
        (method.as_str(), scheme.as_str()),
        ("GET", "https"),
        "the redirect prompt must describe the redirect hop"
    );
}

/// `net::resolve` claims to resolve `Location` "like a browser", but dot
/// segments are kept: `../x` from `/api/v1/items` must be `/api/x`
/// (RFC 3986 §5.2.4), not `/api/v1/../x`.
#[test]
fn redirect_locations_remove_dot_segments() {
    use arcode_plugin_host::net::resolve;
    let base = "http://a.test/api/v1/items";
    assert_eq!(resolve(base, "../x").as_deref(), Ok("http://a.test/api/x"));
    assert_eq!(
        resolve(base, "./y").as_deref(),
        Ok("http://a.test/api/v1/y")
    );
}

/// The bundled HTTP client, registered as bundled (the `arcode.` prefix is
/// reserved) from `target/wasm-plugins` (`scripts/build-plugins.sh`).
fn bundled_http_client() -> arcode_plugin_host::RegisteredPlugin {
    use arcode_plugin_host::PluginSource;
    use arcode_plugin_host::manifest::UserLimits;
    use arcode_plugin_host::registry::load;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm-plugins/arcode.http-client");
    assert!(
        dir.join("plugin.wasm").is_file(),
        "run scripts/build-plugins.sh http-client"
    );
    let manifest = load(&dir, PluginSource::Bundled).expect("manifest");
    let limits = manifest.limits.clamp(&UserLimits::default());
    arcode_plugin_host::RegisteredPlugin {
        manifest,
        dir,
        source: PluginSource::Bundled,
        limits,
        shadowed: Vec::new(),
    }
}

/// §9: "Open in editor" calls `open-scratch` with the body. Appendix B
/// says the plugin caps responses at 16 MiB so "the body, its text and the
/// view fit in the manifest's 64 MiB memory limit" and that Open in editor
/// "gets the whole body (JSON pretty-printed)". Pretty-printing goes
/// through `serde_json::Value`, which needs many times the text size, so a
/// 15 MiB JSON array (well under the cap, not truncated) traps the plugin
/// (out of memory or the 1 s `handle-ui-event` budget) instead of opening.
#[test]
fn open_in_editor_handles_a_large_json_body() {
    use arcode_plugin_host::bindings::ui::{Event, EventKind};
    use arcode_plugin_host::bridge::DocSnapshot;
    use arcode_plugin_host::consent::{ConsentAnswer, ScriptedConsent};
    use arcode_plugin_host::grants::GrantScope;
    use arcode_plugin_host::memory_editor::MemoryEditor;
    use arcode_plugin_host::net::{TransportOptions, UreqTransport};
    use loopback::{Reply, Server};

    let _serial = timing_lock();
    let mut body = String::from("[");
    while body.len() < 15 * 1024 * 1024 {
        body.push_str(r#"{"id":12345,"v":"x"},"#);
    }
    body.push_str(r#"{"id":0,"v":"end"}]"#);
    let body = body.into_bytes();
    let server = Server::start(move |_| Reply {
        status: 200,
        headers: vec![("content-type".into(), "application/json".into())],
        body: body.clone(),
        delay: Duration::ZERO,
    });
    let project = tempfile::tempdir().expect("project");
    let file = format!("GET {}\n", server.url("/big"));
    let editor = Arc::new(MemoryEditor::default());
    editor.publish(DocSnapshot {
        id: 1,
        path: Some("big.http".into()),
        language: "http".into(),
        version: 1,
        rope: ropey::Rope::from_str(&file),
        selections: vec![],
    });
    let app = App::start(None);
    let consent = Arc::new(ScriptedConsent::always(ConsentAnswer::Allow {
        scope: GrantScope::Global,
        write: false,
    }));
    let transport = UreqTransport::new(&TransportOptions {
        use_env_proxy: false,
        ..TransportOptions::default()
    });
    let services = services(&app, consent, editor.clone(), Arc::new(transport));
    let (rt, _cache) = runtime(|_| {});
    let h = arcode_plugin_host::InstanceHandle::new(
        rt,
        spec(bundled_http_client(), services, Some(project.path())),
    );
    let send = Call::Command {
        command: "send".into(),
        args: json!({"doc": 1, "line": 0}).to_string(),
    };
    assert_eq!(h.dispatch_about(send, true, 1).wait(), Ok(CallOutput::Done));
    let (panel, _) = h.open_panels().first().cloned().expect("response panel");
    let snapshot = h.panel_view(panel).expect("view");
    let button = snapshot
        .nodes
        .values()
        .find(|n| matches!(&n.widget, Widget::Button(b) if b.label == "Open in editor"))
        .map(|n| n.id)
        .expect("Open in editor button");
    let click = Call::UiEvent {
        panel,
        event: Event {
            node: button,
            kind: EventKind::Click,
            values: vec![],
        },
    };
    let result = h.dispatch(click, true).wait();
    assert_eq!(result, Ok(CallOutput::Done), "Open in editor on a 15 MiB JSON body");
}
