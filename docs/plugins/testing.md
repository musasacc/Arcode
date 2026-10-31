# Testing and debugging

A plugin is ordinary Rust, so most of it can be tested with a plain
`cargo test` on your machine, without building any WebAssembly. This page
covers native unit tests against the SDK's fake host, end-to-end tests
against the real host, and how to debug a plugin running in Arcode.

## Native tests and the fake host

When your crate is compiled for anything other than `wasm32`, every host
function in the SDK (`host::notify`, `editor::text`, `kv::set`,
`http::send`, `Panel::open`, ...) goes to `arcode_plugin::testing`
instead of Arcode. That module keeps a `FakeHost` per test thread, which
you arrange before calling your plugin and inspect afterwards.

Two things make this work:

1. `crate-type = ["cdylib", "rlib"]` in `Cargo.toml` (the `rlib` lets the
   test binary link your code).
2. `export_plugin!` only for wasm:

   ```rust
   #[cfg(target_arch = "wasm32")]
   arcode_plugin::export_plugin!(MyPlugin);
   ```

   Because the export is the only thing that uses your plugin type outside
   tests, a native `cargo build` warns that it is never constructed. Make
   the type `pub`, or silence the warning for native builds at the top of
   `src/lib.rs`:

   ```rust
   #![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
   ```

Then call your `Plugin` methods directly:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use arcode_plugin::testing;

    #[test]
    fn say_hello_notifies() {
        testing::reset();                                  // a fresh fake host
        testing::with(|h| h.settings = r#"{"greeting": "Hi"}"#.into());
        let mut plugin = MyPlugin::default();
        plugin.command("say-hello", Json::null()).unwrap();
        let shown = testing::with(|h| h.notifications.clone());
        assert_eq!(shown, [(host::Level::Info, "Hi!".to_owned())]);
    }
}
```

- `testing::reset()` empties this thread's fake host. Call it at the start
  of every test.
- `testing::with(|h| ...)` runs a closure with `&mut FakeHost`. Do not
  call SDK functions inside the closure (that would borrow the fake twice
  and panic); arrange, call your plugin, then inspect.
- Each test runs on its own thread, so tests do not see each other's
  state and can run in parallel.

### What `FakeHost` holds

All fields are public.

| Field | Used by | Meaning |
|---|---|---|
| `log` | `host::log` | Every log line, `(Level, String)`. |
| `notifications` | `host::notify` | Every notification. No rate limit in the fake. |
| `status` | `host::set_status` | The last status item, `(text, command)`. |
| `quick_pick_answers` | `host::quick_pick` | Answers consumed front to back; empty means Escape. |
| `quick_picks` | `host::quick_pick` | Titles of every quick pick shown. |
| `settings` | `host::settings` | JSON text; empty means `{}`. |
| `cancelled` | `host::cancelled` | What `cancelled()` returns. |
| `gesture` | `host::begin_task` | Whether the current call counts as a user action. `begin_task` succeeds only when it is `true`. |
| `tasks` | `host::begin_task` | Titles of tasks started. |
| `kv` | `kv::*` | The store, keyed by `(Scope, key)`. Key rules and the 1 MiB quota are enforced like in Arcode. |
| `root_name` | `workspace::root_name` | |
| `files` | `workspace::find_files` | Returned as is (the glob is ignored), capped by `limit`. |
| `files_error` | `workspace::find_files` | Makes it fail. |
| `documents` | `editor::*` | Documents by id; add them with `h.add_document(path, language, text)`. |
| `current_document` | `editor::current_document` | The id it returns. |
| `opened_files` | `editor::open_file` | Every call. |
| `scratches` | `editor::open_scratch` | Every call, `(title, language, text)`. |
| `requests` | `http::send` | Every request sent. |
| `http` | `http::send` | A closure that answers requests; `None` answers `HttpError::Denied`. |
| `panels` | `ui::*`, `Panel` | Every panel by id. `h.panel("contribution")` finds the open one. |

A `FakePanel` has `contribution`, `title`, `view` (the tree as Arcode
would hold it after every render and patch), `renders` (number of full
renders), `patch_calls` (every patch set) and `closed`.

The fake follows Arcode where that is cheap: `apply_edits` checks the
version and ranges and bumps the version, `kv::set` enforces the key
rules and the quota, `begin_task` needs `gesture`. It does **not** check
capabilities: a fake document is readable whatever your manifest says,
and `http::send` goes to your closure without any consent step. Test
those paths end to end (below) or in Arcode.

### Testing a request

```rust
use arcode_plugin::http::{Request, Response, Timing};

#[test]
fn fetch_shows_the_status() {
    testing::reset();
    testing::with(|h| {
        h.gesture = true;
        h.http = Some(Box::new(|req: &Request| {
            Ok(Response {
                status: 200,
                reason: "OK".into(),
                version: "HTTP/1.1".into(),
                final_url: req.url.clone(),
                remote_addr: None,
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: br#"{"ok": true}"#.to_vec(),
                truncated: false,
                timing: Timing { headers_ms: 3, total_ms: 5 },
            })
        }));
    });
    let mut plugin = MyPlugin::default();
    plugin.command("fetch", Json::null()).unwrap();
    assert_eq!(testing::with(|h| h.requests.len()), 1);
}
```

### Testing a panel

Read node ids from your `Panel` (tests in the same crate can reach private
fields) and send events the way Arcode would, including the input values:

```rust
#[test]
fn greet_button_greets() {
    testing::reset();
    let mut plugin = HelloWorld::default();
    plugin.command("show-panel", Json::null()).unwrap();
    let panel_id = testing::with(|h| *h.panels.keys().next().unwrap());

    let p = plugin.panel.as_ref().unwrap();
    let click = Event {
        node: p.node_id("greet").unwrap(),
        kind: EventKind::Click,
        values: vec![(p.node_id("name").unwrap(), "Ada".into())],
    };
    plugin.ui_event(panel_id, click).unwrap();

    let texts: Vec<String> = testing::with(|h| {
        h.panel("hello").unwrap().view.nodes.values()
            .filter_map(|n| match &n.widget {
                Widget::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect()
    });
    assert!(texts.contains(&"Hello, Ada!".to_owned()));
}
```

`plugins/hello-world/src/tests.rs` has this test and a few more; the
HTTP Client's `src/tests.rs` drives a whole request flow.

### Files

Project files are read with `std::fs` under `/project`, which does not
exist on your machine. Make the root configurable and point it at a
temporary folder in tests. The HTTP Client does it like this:

```rust
pub struct HttpClient { project: PathBuf, /* ... */ }

impl Default for HttpClient {
    fn default() -> Self {
        Self::with_project(workspace::PROJECT_DIR)
    }
}
```

Tests create `HttpClient::with_project(temp_dir.path())`.

### Random UI trees

`arcode_plugin::testing::random` generates seeded random widget trees
that satisfy every host rule. The SDK and the host use it to check that
diffing and patching always end up with the same tree as a fresh render.
You only need it if you build your own tree tooling.

## End-to-end tests against the real host

To test the compiled `plugin.wasm` with the real sandbox, permission
checks and budgets, use `arcode-plugin-host` headless: a
`PluginManager` with an in-memory editor and scripted consent answers,
and no GPUI. Arcode's own tests for the bundled plugins work this way;
see `crates/arcode-plugin-host/tests/sandbox/http_client/mod.rs` and
`crates/arcode-plugin-host/tests/db_viewer/`.

The pattern:

```rust
use std::sync::Arc;
use arcode_plugin_host::bridge::DocSnapshot;
use arcode_plugin_host::consent::{ConsentAnswer, ScriptedConsent};
use arcode_plugin_host::grants::GrantScope;
use arcode_plugin_host::memory_editor::MemoryEditor;
use arcode_plugin_host::net::UreqTransport;
use arcode_plugin_host::paths::PluginPaths;
use arcode_plugin_host::{AppServices, CallOutput, ManagerConfig, PluginManager, ProjectContext, QualifiedId};

let dirs = tempfile::tempdir()?;
let paths = PluginPaths::with_bases(
    None,
    &dirs.path().join("config"),
    &dirs.path().join("data"),
    &dirs.path().join("cache"),
);
let mut config = ManagerConfig::new(paths);
config.dev_folders = vec!["dist/acme.word-count".into()];   // your built plugin folder

let editor = Arc::new(MemoryEditor::default());
editor.publish(DocSnapshot {
    id: 1,
    path: Some("notes.md".into()),
    language: "markdown".into(),
    version: 1,
    rope: ropey::Rope::from_str("one two three"),
    selections: vec![],
});
let app = AppServices {
    editor,
    index: None,                                  // walk the project folder
    http: Arc::new(UreqTransport::default()),
    consent: Some(Arc::new(ScriptedConsent::new(vec![ConsentAnswer::Allow {
        scope: GrantScope::Project,
        write: false,
    }]))),
};
let (mut manager, outputs) = PluginManager::new(config, app);
manager.set_project(Some(ProjectContext { root: project_dir, name: "demo".into() }));

let command = QualifiedId::parse("acme.word-count/count").unwrap();
let result = manager.run_command(&command, "null", Some(1))?.wait();
assert_eq!(result, Ok(CallOutput::Done));
// `outputs` receives PluginOutput::Notify, PanelView, Status, ... events.
```

Check the exact constructors against the host crate when you write such a
test; the host's own tests are the reference. They never touch the
external network: HTTP tests run against a `TcpListener` on 127.0.0.1.

Inside the Arcode repository, the host's tests load the bundled plugins
from `target/wasm-plugins`, so build them first:

```sh
scripts/build-plugins.sh                  # bundled plugins -> target/wasm-plugins/
scripts/build-test-plugins.sh             # sandbox fixtures -> target/test-plugins/
cargo test -p arcode-plugin-host
```

### The host's fixture plugins

`crates/arcode-plugin-host/test-programs/` holds tiny plugins that each
check one sandbox rule, built by `scripts/build-test-plugins.sh`. They
are worth reading to see what the host refuses and how:

| Fixture | What it checks |
|---|---|
| `echo` | Round-trips every API type through the host. |
| `spin`, `spin-cancellable` | CPU budget overruns, cooperative cancel. |
| `alloc-bomb` | The memory limit. |
| `crash` | Crash backoff and disabling after repeated crashes. |
| `stdout-flood` | The log cap. |
| `fs-escape` | `..`, absolute paths and symbolic links out of `/project`, writes without a write grant. |
| `sockets` | Raw sockets are denied. |
| `net` | HTTP outside a user action, consent answers, redirects, cancel. |
| `undeclared-import` | Importing `http` without a `network` capability is refused. |
| `wasi-http` | The standard WASI HTTP interface is refused. |
| `ui-big` | Big tables fetch only the pages they need. |
| `hostile` | Calls host functions the way a hostile plugin would (floods, oversized input). |

## Debugging a running plugin

- **Logs.** `host::log`, `println!` and `eprintln!` go to your plugin's
  log. **Plugins: Show Log** shows every plugin's log; the **Log** button
  on a plugin's card shows one plugin's log and its network log. Reopen
  the tab to refresh it.
- **Crashes.** A trap (a panic, running out of memory, overrunning the
  CPU budget, not stopping after cancel) shows "<Plugin> stopped:
  <reason>" in your panels and in the log, for example "exceeded 2 s CPU"
  or "out of memory (limit 64 MiB)". Panics in Rust print their message
  to stderr first, so the log has it.
- **Refused plugins.** If your plugin does not show up at all, its
  manifest was refused: the end of **Plugins: Show Log** says why.
  If it shows up but fails to start, the notification says why, for
  example an import Arcode does not allow.
- **Arcode's own log** goes to stderr. Start Arcode from a terminal with
  `RUST_LOG=arcode_plugin_host=debug,arcode=info arcode` for details on
  discovery, activation and the plugin cache.
- **Iterating.** Rebuild, copy `plugin.wasm` into your folder, run
  **Plugins: Reload**. There is no file watcher.
- **A clean slate.** Start Arcode with `ARCODE_CONFIG_DIR` and
  `ARCODE_DATA_DIR` pointing at empty folders to test first-time prompts
  without touching your real grants, settings and stored data.

There is no debugger for plugins. Keep logic in plain functions you can
unit-test natively, and keep the `Plugin` impl thin.
