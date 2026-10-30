# arcode-plugin-host

The sandboxed WASM plugin host (docs/PLUGINS.md). No GPUI. The app talks to
it only through **one facade type, channels and traits**:

- `PluginManager`: every call the app makes. Each returns at once.
- `async_channel::Receiver<PluginOutput>`: everything plugins send the app.
- The traits in `bridge` and `consent`: what the host needs from the app.

The UI thread never waits on a plugin. Plugin threads wait on the app
(prompts, quick picks, editor writes), never the other way around.

## Setup

```rust
use std::sync::Arc;
use arcode_plugin_host::{AppServices, ManagerConfig, PluginManager, ProjectContext};
use arcode_plugin_host::net::UreqTransport;
use arcode_plugin_host::paths::PluginPaths;

let paths = PluginPaths::for_current_user().expect("home directory");
let mut config = ManagerConfig::new(paths);   // persist = true, §5 defaults
config.dev_folders = dev_folders;             // "Plugins: Load Unpacked…"
let app = AppServices {
    editor: Arc::new(my_editor_bridge),       // see "Editor" below
    index: None,                              // None: walk the project (.gitignore-aware)
    http: Arc::new(UreqTransport::default()), // ureq + rustls, env proxies honoured
    consent: None,                            // None: prompts arrive as PluginOutput::Consent
};
let (manager, outputs) = PluginManager::new(config, app);
// Drain `outputs` on a GPUI task (like the LSP event channel).
manager.set_project(Some(ProjectContext { root, name }));
```

`PluginManager::new` reads `plugin.toml` files only: no engine, no thread,
no wasm. `manager.registry()` lists plugins, their contributions
(commands, panels, languages, openers, settings, lens languages) and
discovery problems, which is everything needed to populate the palette,
the activity bar and the Plugins view at startup. A plugin's instance
(thread + wasmtime store) starts on its first call and unloads after 10
idle minutes unless it has an open panel.

## What the app calls

Every method below returns immediately. Methods that run plugin code return a
`Pending { id, cancel, result }`: await `result` (an `async_channel`
receiver yielding `Result<CallOutput, CallError>`) on a GPUI task, and call
`cancel.cancel()` to cancel. The host sets the user-gesture flag (§4) for you.

| Situation | Call | Gesture | Result |
|---|---|---|---|
| Palette, keybinding, lens click, status-item click | `run_command(&qualified_id, args_json, about_doc)` | yes | `Done` or `CallError::Plugin(msg)` |
| Editor became visible / 300 ms after the last edit (the app debounces) | `code_lenses(doc_id)` → `Vec<LensCall { plugin, pending }>` | no | `CallOutput::Lenses(Vec<CodeLens>)` |
| Activity-bar entry / "View: …" for a contributed panel | `show_panel(&qualified_id)` → `PanelCall { plugin, panel, pending }` | no | `Done`; the view arrives as `PanelView` |
| File tree opens a file | `opener_for(rel_path)` then `open_with(&opener, "a/b.db")` → `PanelCall` | no | as above; the plugin gets `{"path": ...}` |
| Widget event | `ui_event(&plugin, panel, Event { node, kind, values })` | per `event_is_gesture(kind)` | `Done` |
| Table needs rows | `fetch_rows(&plugin, panel, table, PageRequest)` | no | `CallOutput::Rows(RowPage)` |
| User closed a panel | `close_panel(&plugin, panel)` | no | `Done` |
| Settings → Plugins → network switch | `set_network_enabled(bool)` | | |
| Plugins view: enable/disable, settings | `is_enabled(&id)`, `set_enabled(&id, bool)`, `set_user_settings(&id, map)` | | persisted in `plugins.toml`; enabling an installed plugin approves its `plugin.wasm` |
| "Plugins: Install from Folder…" | on a background task: `install::install_from_folder(src, &paths.installed)` then `install::precompile(manager.runtime(), &report)`; then `finish_install(&report)` (or the blocking `install_from_folder(src)`) | | the plugin starts disabled |
| Permissions page | `grants(&id)`, `revoke(&id, &capability)`, `background_network_active()` | | revoking a file grant recycles the instance |
| Plugin log, network log, counters | `instance(&id)?.log()`, `.network_log()`, `.stats()`, `.open_panels()`, `.panel_view(panel)` | | |
| "[Restart]" after a crash | `restart(&id)` | | |
| "Plugins: Reload" | `reload(dev_folders)` | | |
| Project opened / switched / closed | `set_project(Option<ProjectContext>)` | | stops instances |
| Exit | `shutdown()` (also on drop) | | |

`event_is_gesture` is true for `click`, `submit`, `select`, `tab`, `sort`,
`row-activate` and `tree-activate`; false for `change`, `toggle` and
`tree-toggle`. Only gesture calls may send HTTP requests or start a task.

`Event.values` must carry the current text of every `input` the user edited
since the previous event, so a Run click delivers the query in the same call.

Refusals before dispatch are `ManagerError::{UnknownPlugin,
UnknownContribution, Disabled}`. Call failures are `CallError`:

| `CallError` | Show |
|---|---|
| `Plugin(msg)` | the plugin's own message, as plain text |
| `Trapped(reason)` | "<Plugin> stopped: <reason> [Restart]" in its panels (also sent as `Crashed`) |
| `Activation(msg)` | a notice: the plugin could not start (import check, API version, `activate` error) |
| `Backoff(d)`, `Disabled` | the plugin is restarting / disabled for the session |
| `LensesDisabled` | lenses are off for this plugin for the session (3 budget overruns) |
| `Superseded` | nothing: a newer lens/row request replaced this one |
| `Busy`, `Stopped` | nothing, or a quiet status message |

## What the app receives (`PluginOutput`)

| Variant | What to do |
|---|---|
| `Consent { request, reply }` | Show host chrome built from `request` (`request.message()`, method/host/port from `request.kind`), then `reply.send(ConsentAnswer::Allow { scope, write })`, `Deny`, or drop `reply` to dismiss. Workspace prompts have a write checkbox when `kind` says `write: true`. Only sent when `AppServices.consent` is `None`. |
| `QuickPick { title, items, reply }` | Show a quick pick; `reply.send(Some(index))`, or drop `reply` on Escape. |
| `PanelOpened { plugin, panel, contribution, title, location }` | Create (or focus) the panel at `location` (or the manifest's). |
| `PanelView { plugin, panel, snapshot, changes }` | Redraw from `snapshot` (an `Arc<PanelSnapshot>`); see below. |
| `PanelTitle { .. }`, `PanelClosed { .. }` | Retitle / remove the panel. |
| `Notify { plugin, level, message }` | Toast (already rate-limited and capped). |
| `Status { plugin, text, command }` | The plugin's status item; clicking runs `command` with `run_command`. `text: None` removes it. Coalesced: at most one per 100 ms, the latest when the call returns. |
| `NeedsApproval { plugin }` | An installed plugin's `plugin.wasm` changed since the user enabled it; it does not run. Ask, then `set_enabled(&plugin, true)` (approves the new file) or `false`. |
| `TaskStarted { plugin, call, title, cancel }` / `TaskFinished { plugin, call }` | Show / hide "title… [Cancel]"; the button calls `cancel.cancel()`. |
| `Crashed { plugin, reason, disabled }` | Show "<Plugin> stopped: <reason>" in its panels, with [Restart] when `disabled`. |
| `Unloaded { plugin }` | Informational (idle unload). |

Plugin-supplied strings in outputs are plain text, length-capped by the
host. Never render them as markup. In prompts, plugin text is already
quoted by `consent::quote`.

## Panels and tables

The plugin thread validates every `render` / `apply-patches` with
`ui_tree::PanelTree` (dangling ids, cycles, two parents, child counts per
widget, 20k nodes, depth 64, 8 MiB per panel of text and list elements)
and applies it atomically.
The app only ever sees valid trees:

- `snapshot.root`, `snapshot.node(id)`, `snapshot.children(id)`: the whole
  view. Nodes are `Arc`s shared with the previous snapshot.
- `changes.full`: the view was replaced (`render`). Otherwise
  `changes.dirty` and `changes.removed` say which nodes to redraw or drop.
  Element ids are `(panel, node-id)`.
- `changes.inputs`: inputs whose `value` the **plugin** changed. Only these
  overwrite what the user is typing (§7 "Uncontrolled inputs"). Keep text,
  scroll positions, split ratios, the active tab and tree expansion on the
  app side.
- `changes.tables`: `TableChange { table, row_count, reset }`. Call
  `cache.reset(row_count)` when `reset`, otherwise
  `cache.set_row_count(row_count)`.

Each `table` widget keeps a `rows::RowCache` (pure, no threads):

```rust
cache.set_viewport(first_visible_row, last_visible_row);
if let Some(req) = cache.next_fetch() {            // at most one outstanding
    let pending = manager.fetch_rows(&plugin, panel, table, req)?;
    // later, on the GPUI task:
    match pending.result.recv().await {
        Ok(Ok(CallOutput::Rows(page))) => { cache.page_arrived(req, page); }
        _ => cache.page_failed(req),               // superseded, error
    }
    // then call next_fetch() again and notify the view
}
let row: Option<&[Cell]> = cache.row(index);       // None: draw a placeholder
```

Pages are 200 rows, at most 6 pages are cached, visible pages come first and
one page is prefetched. Text cells arrive cut to 4 KiB, and a page over
4 MiB (32 bytes per cell plus its text) is refused with `CallError::Plugin`.
`set_row_count` forgets an end learned from an `at-end` page and refetches
a partial last page, so a table that grew without a reset fills in.

## Editor

Implement `bridge::EditorBridge` (called on plugin threads):

- Publish a `DocSnapshot { id, path, language, version, rope, selections }`
  after each edit while any plugin is active (cloning a `Rope` is O(1)).
  `snapshot()` reads it, so `editor.text` never touches the UI thread.
- `apply_edits(doc, expected_version, edits)`: hop to the UI thread, refuse
  with `VersionMismatch(current)` if the document moved, apply as **one undo
  step**, return the new version. The host has already validated the ranges
  against the snapshot and asked for editor-write consent. Wait at most
  `bridge::UI_REPLY_TIMEOUT` (1 s) for the UI thread.
- `open_file(rel_path, at)` (path already validated: relative, no `..`, and
  a regular file inside the project reached without following symlinks)
  and `open_scratch(title, language, text)`.
- `editor = "context"` covers the call's origin document and documents of a
  language the plugin contributes, unless the app has that language built
  in: set `ManagerConfig.runtime.builtin_languages` from the syntax
  registry (the default is a list of common language ids).

`memory_editor::MemoryEditor` is a complete in-memory implementation. Use it
headless, in tests, and as a reference for the version and range rules.

## Other traits

- `bridge::ProjectIndex::find_files(matcher, limit)`: project-relative paths
  with `/` that match the glob and honour `.gitignore`. With `index: None`
  the manager uses `index::WalkIndex`. Plug in the app's file index when
  there is one.
- `bridge::HttpTransport`: use `net::UreqTransport`. Set
  `TransportOptions { use_env_proxy: false, .. }` to ignore `HTTPS_PROXY`.
  The host checks the master switch, the gesture, `network.hosts`, the
  method (an HTTP token of at most 32 characters) and per-host consent
  before calling it, and again on every redirect hop: `allow_redirect`
  receives the hop's method and absolute URL, and the prompt shows that
  hop's method, scheme, host and port.
- `consent::ConsentProvider`: optional. The default `ChannelConsent` turns
  prompts into `PluginOutput::Consent`. A custom provider is called on the
  plugin thread, may block, and must return `Dismissed` once the token is
  cancelled. `ScriptedConsent` answers from a script (tests).

## Files on disk

All under the user's directories (`paths::PluginPaths`), never in the
project: global grants in `<config>/arcode/plugin-grants.toml`, project
grants in `<data>/arcode/projects/<hash>/plugin-grants.toml`, kv data in
`<data>/arcode/plugin-data/<id>/{global,<hash>}.kv`, and compiled components
in `<cache>/arcode/plugins/*.cwasm`. With `ManagerConfig.persist = false`,
grants, kv and `plugins.toml` stay in memory. `<config>/arcode/plugins.toml`
holds `enabled` and the approved `sha256` per plugin (§3); installed
plugins live in `<data>/arcode/plugins/<id>/`.

Release packaging puts the bundled plugins next to the binary:
`ARCODE_WASM_PLUGINS=<prefix>/share/arcode/plugins scripts/build-plugins.sh`
(the host looks in `<exe>/../share/arcode/plugins`).

## Tests

```sh
scripts/build-test-plugins.sh            # fixtures -> target/test-plugins/
scripts/build-plugins.sh                 # bundled plugins -> target/wasm-plugins/
cargo test -p arcode-plugin-host         # unit tests + tests/sandbox + tests/db_viewer
cargo run --release -p arcode-plugin-host --example plugin_bench   # activation + RSS (§13 step 12)
```

`tests/db_viewer` runs the real `arcode.db-viewer` against SQLite
fixtures from `plugins/db-viewer/tests/fixtures/`, including a 1M-row
database the plugin generates itself (about 10 s in a debug build).

Fixture guests live in `test-programs/` (their own cargo workspace). The
HTTP tests use a loopback server only.
