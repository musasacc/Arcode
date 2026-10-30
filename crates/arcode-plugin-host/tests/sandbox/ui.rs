//! Step 8: `ui-big` pages a million-row table through `RowCache`, the UI
//! caps hold through the real import, and `PanelTree` accepts every patch
//! set the SDK's diff emits (seeded property test).

use std::sync::Arc;

use arcode_plugin::testing::random::{Rng, random_tree};
use arcode_plugin::ui::{FlatView, IdMap, diff, flatten};
use arcode_plugin_host::bindings::ui::{Cell, Widget};
use arcode_plugin_host::bridge::NoEditor;
use arcode_plugin_host::consent::DenyAll;
use arcode_plugin_host::rows::{MAX_CACHED_PAGES, PAGE_ROWS, RowCache};
use arcode_plugin_host::ui_tree::{MAX_NODES, PanelTree, TableChange};
use arcode_plugin_host::{Call, CallError, CallOutput, InstanceHandle, PluginOutput};
use serde_json::{Value, json};

use crate::common::*;
use crate::convert;

struct Big {
    h: InstanceHandle,
    app: App,
    _cache: tempfile::TempDir,
}

fn big() -> Big {
    let app = App::start(None);
    let services = services(
        &app,
        Arc::new(DenyAll),
        Arc::new(NoEditor),
        Arc::new(FakeHttp::default()),
    );
    let (rt, cache) = runtime(|_| {});
    let h = handle(&rt, fixture("ui-big"), services, None);
    Big {
        h,
        app,
        _cache: cache,
    }
}

fn render(h: &InstanceHandle, args: Value) -> Value {
    assert_eq!(command(h, "render", args, false), Ok(CallOutput::Done));
    last_with(h, "op", "render")["result"].clone()
}

/// Opens the big panel; returns (panel, table node).
fn open(b: &Big) -> (u32, u32) {
    let (panel, pending) = b.h.open_view("big", "{}");
    assert_eq!(pending.wait(), Ok(CallOutput::Done));
    let view = b.h.panel_view(panel).expect("rendered");
    let (table, rows) = view
        .nodes
        .values()
        .find_map(|n| match &n.widget {
            Widget::Table(t) => Some((n.id, t.row_count)),
            _ => None,
        })
        .expect("a table");
    assert_eq!(rows, Some(1_000_000));
    (panel, table)
}

/// Drives `cache` like a table widget would until it is satisfied.
fn settle(b: &Big, panel: u32, table: u32, cache: &mut RowCache) {
    while let Some(req) = cache.next_fetch() {
        let call = Call::FetchRows {
            panel,
            table,
            start: req.start,
            count: req.count,
        };
        match b.h.dispatch(call, false).wait() {
            Ok(CallOutput::Rows(page)) => assert!(cache.page_arrived(req, page)),
            other => panic!("fetch failed: {other:?}"),
        }
    }
}

fn fetched(h: &InstanceHandle) -> Vec<(u64, u64)> {
    json_lines(h)
        .into_iter()
        .filter(|v| v["export"] == "fetch-rows")
        .map(|v| {
            (
                v["start"].as_u64().unwrap_or(0),
                v["count"].as_u64().unwrap_or(0),
            )
        })
        .collect()
}

#[test]
fn ui_big_fetches_only_the_requested_pages() {
    let b = big();
    let (panel, table) = open(&b);
    let mut cache = RowCache::new(Some(1_000_000));
    for (first, last) in [(0, 40), (500_000, 500_040), (999_990, 999_999)] {
        cache.set_viewport(first, last);
        let started = std::time::Instant::now();
        settle(&b, panel, table, &mut cache);
        eprintln!("rows {first}..{last}: settled in {:?}", started.elapsed());
        assert!(cache.cached_rows() <= MAX_CACHED_PAGES * PAGE_ROWS as usize);
    }
    let rows = u64::from(PAGE_ROWS);
    assert_eq!(
        fetched(&b.h),
        [
            (0, rows),
            (200, rows),
            (500_000, rows),
            (500_200, rows),
            (999_800, rows)
        ],
        "visible pages plus one ahead, nothing past the end"
    );
    assert_eq!(
        cache.row(999_999).map(|r| r[0].clone()),
        Some(Cell::Int(999_999))
    );
    assert_eq!(cache.row_count(), Some(1_000_000));
}

#[test]
fn ui_caps_hold_through_the_import() {
    let b = big();
    let h = &b.h;
    assert_eq!(render(h, json!({"depth": 2})), "ok"); // activates (compiles)
    let started = std::time::Instant::now();
    assert_eq!(render(h, json!({"nodes": MAX_NODES})), "ok");
    eprintln!("20k-node render, warm: {:?}", started.elapsed());
    let panel = h.open_panels()[0].0;
    let before = h.panel_view(panel).unwrap();
    assert_eq!(before.nodes.len(), MAX_NODES);
    let r = render(h, json!({"nodes": MAX_NODES + 1}));
    assert!(r.as_str().unwrap().contains("TooLarge"), "{r}");
    assert_eq!(render(h, json!({"depth": 64})), "ok");
    let r = render(h, json!({"depth": 65}));
    assert!(r.as_str().unwrap().contains("InvalidTree"), "{r}");
    assert!(r.as_str().unwrap().contains("deeper than 64"), "{r}");
    let depth_64 = h.panel_view(panel).unwrap();
    assert_eq!(depth_64.nodes.len(), 64, "the refused view changed nothing");
    assert!(
        h.log()
            .lines()
            .iter()
            .any(|l| l.text.contains("view refused"))
    );
}

#[test]
fn rows_changed_and_oversized_pages() {
    let b = big();
    let (panel, table) = open(&b);
    assert_eq!(render(&b.h, json!({"rows-changed": 5})), "ok");
    assert!(
        b.app
            .wait_for(|o| matches!(o, PluginOutput::PanelView { changes, .. }
        if changes.tables == [TableChange { table, row_count: Some(5), reset: true }]))
    );
    assert_eq!(render(&b.h, json!({"huge": true})), "ok");
    let call = Call::FetchRows {
        panel,
        table,
        start: 0,
        count: 200,
    };
    match b.h.dispatch(call, false).wait() {
        Err(CallError::Plugin(m)) => assert!(m.contains("limit"), "{m}"),
        other => panic!("expected the page to be refused: {other:?}"),
    }
}

/// The host side of the step-8 property: for seeded random tree
/// sequences, `PanelTree` accepts the SDK's render and every diff, and
/// ends up with exactly the SDK's fresh flattening.
#[test]
fn panel_tree_accepts_every_sdk_diff() {
    for seed in 0..200 {
        let mut rng = Rng::new(seed);
        let mut ids = IdMap::new();
        let mut tree = PanelTree::new();
        let mut prev: Option<FlatView> = None;
        for step in 0..20 {
            let depth = 1 + rng.below(5) as u32;
            let next = flatten(&random_tree(&mut rng, depth), &mut ids).expect("unique keys");
            let result = match &prev {
                None => tree.render(convert::view(&next.to_view())),
                Some(p) => tree.apply(convert::patches(&diff(Some(p), &next))),
            };
            let (snapshot, _) = result.unwrap_or_else(|e| panic!("seed {seed} step {step}: {e}"));
            assert_eq!(
                snapshot.to_view(),
                convert::view(&next.to_view()),
                "seed {seed} step {step}"
            );
            prev = Some(next);
        }
    }
}
