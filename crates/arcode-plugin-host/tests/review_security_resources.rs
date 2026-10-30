//! Security review: resource exhaustion (§1 priority 2, "a plugin cannot
//! freeze or bloat the editor"). A failing test is an open finding.
//!
//! Needs the fixtures: run `scripts/build-test-plugins.sh` first.

#[allow(dead_code)]
#[path = "sandbox/common.rs"]
mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use arcode_plugin_host::bindings::ui::{Cell, Node, RowPage, TreeItem, View, Widget};
use arcode_plugin_host::bridge::ProjectIndex;
use arcode_plugin_host::consent::{ConsentAnswer, ScriptedConsent};
use arcode_plugin_host::grants::GrantScope;
use arcode_plugin_host::plugin_log::LOG_CAP_BYTES;
use arcode_plugin_host::rows::{MAX_PAGE_BYTES, check_page};
use arcode_plugin_host::ui_tree::{MAX_STRING_BYTES, PanelTree, widget_bytes};
use arcode_plugin_host::{Call, CallOutput, InstanceHandle, OutputSender, Services};
use serde_json::{Value, json};

use common::*;

/// The `review-abuse` fixture with no project; `tweak` adjusts the runtime.
fn abuse(
    services: Services,
    tweak: impl FnOnce(&mut arcode_plugin_host::RuntimeConfig),
) -> (InstanceHandle, tempfile::TempDir) {
    let (rt, cache) = runtime(tweak);
    (handle(&rt, fixture("review-abuse"), services, None), cache)
}

fn default_services(app: &App) -> Services {
    services(
        app,
        Arc::new(ScriptedConsent::default()),
        Arc::new(FakeEditor::default()),
        Arc::new(FakeHttp::default()),
    )
}

fn run(h: &InstanceHandle, args: Value) {
    assert_eq!(command(h, "run", args, true), Ok(CallOutput::Done));
}

/// Finding: the 1 MiB log cap counts text bytes only, so empty lines are
/// free. A guest that writes newlines to stdout (or calls `host.log("")`)
/// adds one `LogLine` per byte (or call) that is never evicted: 1 MiB of
/// `\n` keeps a million entries (~48 MB of host memory), without bound.
#[test]
fn a_stdout_newline_flood_stays_under_the_log_cap() {
    let app = App::start(None);
    let (h, _cache) = abuse(default_services(&app), |_| {});
    run(&h, json!({"op": "stdout-newlines", "mib": 1}));
    let lines = h.log().lines().len();
    let entry = std::mem::size_of::<arcode_plugin_host::plugin_log::LogLine>();
    assert!(
        lines * entry <= 2 * LOG_CAP_BYTES,
        "{lines} log lines kept ({} bytes of entries) for a 1 MiB cap",
        lines * entry
    );
}

/// Same finding through `host.log("")`.
#[test]
fn empty_host_log_lines_stay_under_the_log_cap() {
    let app = App::start(None);
    let (h, _cache) = abuse(default_services(&app), |_| {});
    run(&h, json!({"op": "log-empty", "count": 300_000}));
    let lines = h.log().lines().len();
    let entry = std::mem::size_of::<arcode_plugin_host::plugin_log::LogLine>();
    assert!(
        lines * entry <= 2 * LOG_CAP_BYTES,
        "{lines} empty log lines kept"
    );
}

/// Finding: a guest blocked in a WASI wait (`std::thread::sleep`, i.e.
/// `monotonic-clock.subscribe-duration` + `poll`) is neither charged CPU
/// nor stoppable: the epoch callback that implements hard cancel only
/// runs while wasm executes. §5: "After a 2 s grace period the epoch
/// callback traps, as a hard cancel" does not happen; the instance (and
/// every queued call, panel and task chip) is stuck for as long as the
/// plugin likes.
#[test]
fn cancel_stops_a_guest_sleeping_in_wasi() {
    let _t = timing_lock();
    let app = App::start(None);
    let (h, _cache) = abuse(default_services(&app), |c| {
        c.budgets.cancel_grace = Duration::from_millis(200);
    });
    let call = Call::Command {
        command: "run".into(),
        args: json!({"op": "sleep", "ms": 6000}).to_string(),
    };
    let pending = h.dispatch(call, true);
    std::thread::sleep(Duration::from_millis(300));
    let cancelled_at = Instant::now();
    pending.cancel.cancel();
    let result = pending.wait();
    let took = cancelled_at.elapsed();
    assert!(
        took < Duration::from_millis(1500),
        "cancel returned after {took:?} ({result:?})"
    );
}

/// Finding: the 8 MiB string cap is per `render`/`apply-patches` call,
/// not per panel. A guest that holds one 4 MiB string re-sends it as a new
/// node on every call, and the host keeps every copy (up to 20,000 nodes):
/// host memory grows without bound while guest memory stays flat.
#[test]
fn a_panel_cannot_accumulate_unbounded_text() {
    let app = App::start(None);
    let (h, _cache) = abuse(default_services(&app), |_| {});
    run(&h, json!({"op": "panel-grow", "calls": 6, "mib": 4}));
    let (panel, _) = h.open_panels().into_iter().next().expect("panel");
    let snapshot = h.panel_view(panel).expect("view");
    let kept: usize = snapshot
        .nodes
        .values()
        .map(|n| widget_bytes(&n.widget))
        .sum();
    assert!(
        kept <= MAX_STRING_BYTES,
        "panel keeps {kept} bytes of text (per-call cap {MAX_STRING_BYTES}): {}",
        last_with(&h, "op", "panel-grow")
    );
}

/// Finding: elements with empty strings cost nothing against the UI cap,
/// so one `render` of a `tree` with 500k empty items (or a `table` with
/// 500k columns, a `select` with 500k options) is accepted; the host then
/// holds ~100 bytes per element. Together with the per-call accounting
/// above, the cap does not bound host memory.
#[test]
fn empty_elements_count_against_the_ui_cap() {
    let item = TreeItem {
        key: String::new(),
        label: String::new(),
        detail: None,
        icon: None,
        depth: 0,
        expandable: false,
        expanded: false,
    };
    let view = View {
        root: 1,
        nodes: vec![Node {
            id: 1,
            widget: Widget::Tree(vec![item; 500_000]),
            children: vec![],
            grow: false,
        }],
    };
    let accepted = PanelTree::new().render(view).is_ok();
    assert!(!accepted, "500,000 empty tree items were accepted");
}

/// Finding: `check_page` caps text bytes per page but not cells per row
/// (non-text cells count 0). 200 rows x 5,000 `int` cells = 1M cells,
/// ~32 MB of host memory per page (6 cached pages per table), all
/// accepted although §5 says "4 MiB per row page" and the table declares
/// far fewer columns.
#[test]
fn a_row_page_is_capped_in_cells_not_only_text() {
    let rows = vec![vec![Cell::Int(1); 5_000]; 200];
    let page = RowPage {
        start: 0,
        rows,
        at_end: true,
    };
    let checked = check_page(page, 0, 200);
    let cells: usize = checked
        .as_ref()
        .map_or(0, |p| p.rows.iter().map(Vec::len).sum());
    let bytes = cells * std::mem::size_of::<Cell>();
    assert!(
        bytes <= MAX_PAGE_BYTES,
        "row page with {cells} cells ({bytes} bytes) accepted"
    );
}

/// Finding: the output channel is unbounded and `set-status` is not
/// coalesced, so one 2 s command queues as many 1 KiB status messages as
/// the guest can issue. With an app that drains slower than a plugin
/// produces (or not at all while busy), memory grows without bound.
#[test]
fn status_updates_do_not_queue_unboundedly() {
    let app = App::start(None);
    let (sender, rx) = OutputSender::channel();
    let svc = Services {
        output: sender,
        ..default_services(&app)
    };
    let (h, _cache) = abuse(svc, |_| {});
    run(
        &h,
        json!({"op": "status-flood", "count": 50_000, "bytes": 1024}),
    );
    let queued = rx.len();
    assert!(
        queued <= 1_000,
        "{queued} undrained outputs (~{} MiB) after one command",
        queued / 1024
    );
}

/// A project index whose every query costs `cost` of CPU (a big walk).
struct SlowIndex {
    cost: Duration,
}

impl ProjectIndex for SlowIndex {
    fn find_files(&self, _: &globset::GlobMatcher, _: usize) -> Vec<String> {
        let end = Instant::now() + self.cost;
        while Instant::now() < end {
            std::hint::spin_loop();
        }
        Vec::new()
    }
}

/// Finding: host-import time is not charged (by design for waits such as
/// prompts and HTTP), but CPU-heavy imports are not either. Each return to
/// wasm charges one 10 ms slice however long the import ran, so a guest
/// looping over an expensive import (`find-files` on a big project,
/// `apply-patches` on a 20k-node panel) burns budget x (import cost /
/// 10 ms) of host CPU. Here a 200 ms command runs for seconds.
#[test]
fn cpu_heavy_imports_are_charged_to_the_budget() {
    let _t = timing_lock();
    let tmp = tempfile::tempdir().unwrap();
    let app = App::start(None);
    let svc = Services {
        index: Arc::new(SlowIndex {
            cost: Duration::from_millis(100),
        }),
        permissions: arcode_plugin_host::consent::Permissions::new(
            arcode_plugin_host::grant_store::GrantStore::in_memory(true),
            Arc::new(ScriptedConsent::always(ConsentAnswer::Allow {
                scope: GrantScope::Once,
                write: false,
            })),
        ),
        ..default_services(&app)
    };
    let (rt, _cache) = runtime(|c| c.budgets.command = Duration::from_millis(200));
    let h = handle(&rt, fixture("review-abuse"), svc, Some(tmp.path()));
    let started = Instant::now();
    let r = command(&h, "run", json!({"op": "find-loop"}), true);
    let took = started.elapsed();
    assert!(r.is_err(), "{r:?}");
    assert!(
        took < Duration::from_millis(600),
        "a 200 ms command ran for {took:?} before it was stopped"
    );
}
