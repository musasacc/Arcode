# Arcode plugin system (WASM), API 0.1

Status: **accepted design, to implement**. Read `docs/ARCHITECTURE.md` first.

The design is judged by two bundled plugins: `arcode.http-client`, which runs `.http` files, and `arcode.db-viewer`, which browses and queries SQLite files. Both are ordinary sandboxed plugins with no private host hooks. If one of them needs something the API lacks, we change the API, not the plugin.

Priorities, in order:
1. A plugin cannot read, write or send anything the user did not allow.
2. A plugin cannot freeze or bloat the editor.
3. An idle plugin costs nothing: no engine, no thread, no timer.
4. The API is small, typed and pleasant to write against.

---

## 1. Decisions at a glance

| Topic | Decision |
|---|---|
| Engine | **wasmtime 50** with Cranelift, the component model and the **sync** embedding API. The `Engine` is created on first activation, never at startup. |
| ABI | Component model plus WIT: package `arcode:plugin@0.1.0`, guests built for `wasm32-wasip2`. |
| Files | Only through WASI: the project is preopened at `/project` (read, or read-write) using cap-std. There is no second file API. |
| Network | Our own `arcode:plugin/http`, implemented on the host with `ureq` 3 and rustls. It works only inside a user-gesture call, asks consent per host, and re-checks on every redirect. `wasi:http` is refused; `wasi:sockets` is linked but always denied. |
| UI | A declarative, flat, keyed node tree. The host renders it with native widgets and applies patches. Tables pull rows a page at a time through `fetch-rows`. |
| Threads | One OS thread per *active* instance, blocked on its mailbox while idle. One shared epoch ticker runs only while some call is in flight. |
| Limits | An epoch budget that counts only time spent in wasm, a memory cap through `StoreLimits`, cooperative `cancelled()`, and a trap that recycles the instance. |
| Install | From a local folder only. No registry, no update checks, no network. |
| Trust | Grants live only in user config/data dirs. Nothing inside a repository can install, enable or authorize a plugin. |

---

## 2. Engine

wasmtime is the only option that combines the component model, epoch interruption, `ResourceLimiter` and JIT speed. Speed matters here: the DB viewer runs SQLite in wasm and the HTTP client handles multi-MB bodies, and both must stay usable on old laptops. Rejected engines are listed in §15.

**Dependencies.** Everything below sits behind the `plugins` cargo feature of `arcode`, on by default, so a minimal build drops it.

```toml
wasmtime = { version = "=50.x.y", default-features = false,
             features = ["runtime", "cranelift", "component-model", "std"] }
wasmtime-wasi = { version = "=50.x.y" }   # p2, sync linker (add_to_linker_sync)
```

We do not enable `cache` (it pulls zstd and a TOML config), `wat`, `pooling-allocator`, `profiling` or `parallel-compilation`. wasmtime and wit-bindgen are pinned exactly, like GPUI. `wasmtime-wasi` internally pulls tokio, which is already in our tree, but we only use the sync API.

**Engine config:**
- `epoch_interruption(true)`
- `max_wasm_stack(512 KiB)`
- `memory_init_cow(true)`
- `parallel_compilation(false)`, so an install never pegs every core
- `wasm_threads(false)`
- the default on-demand allocator, so RSS is only the pages a plugin actually touches

**Compiled-artifact cache.** Compilation happens at install time, or on first activation if the cache is missing. It runs on a background thread while the Plugins view shows "Preparing…". The result is written with `Engine::precompile_component` to `<cache_dir>/arcode/plugins/<sha256(wasm)>-<precompile_compatibility_hash>.cwasm`, in a directory with mode 0700.

Activation then calls `unsafe Component::deserialize_file`, which mmaps the file. We only do this for files we wrote, named by content hash. If deserialization fails, we recompile.

**Startup cost: zero.** At launch Arcode reads only the `plugin.toml` files. Contributions (commands, languages, openers, panels, lens languages, settings) are registered from the manifests alone, with no wasm read and no engine created.

**Measured exit criterion (Step 0).** If wasmtime adds more than 15 MB to the stripped binary, or the engine plus one idle HTTP instance exceeds 10 MB RSS, we revisit before writing more code. Only `engine.rs` and `bindings.rs` touch wasmtime types, so the engine can be swapped.

---

## 3. Packaging, manifest, discovery

A plugin is a folder with these files:
- `plugin.toml`
- `plugin.wasm`, a component
- optionally `README.md` and `LICENSE`

There are no icons or images in the folder: plugins name icons from the host icon set, so we never decode untrusted image data.

### Locations

| What | Where |
|---|---|
| Bundled | `<exe>/../share/arcode/plugins/<id>/`, read-only. In dev builds: `target/wasm-plugins/<id>/`. Shipped as files, not `include_bytes!`. |
| Installed | `<data_dir>/arcode/plugins/<id>/`. "Plugins: Install from Folder…" validates the folder and copies it (regular files only, wasm ≤ 32 MiB), then precompiles. |
| Dev | "Plugins: Load Unpacked…" references a folder for the session only. "Plugins: Reload" is explicit; there is no file watcher. |
| State | `<config_dir>/arcode/plugins.toml`: `enabled` and `sha256` per plugin. The hash is checked at activation; a mismatch disables the plugin and asks for re-approval. |
| Global grants | `<config_dir>/arcode/plugin-grants.toml` |
| Project grants | `<data_dir>/arcode/projects/<hash(root)>/plugin-grants.toml`. **Never in the project tree.** |
| kv data | `<data_dir>/arcode/plugin-data/<id>/{global, <hash(root)>}.kv` |

Newly installed third-party plugins start **disabled**. Enabling one shows its capability summary. An update that requests more capabilities stays disabled until the user approves the difference.

There is no registry in 0.1. A later opt-in registry (`plugins.registry-url`, empty by default) would be a minisign-signed static index, fetched only when the user opens "Browse".

### Manifest

Validation rules:
- Unknown keys are errors.
- `id` must match `^[a-z0-9][a-z0-9.-]{2,63}$`. The `arcode.` prefix is reserved for bundled plugins.
- Contribution ids are local to the plugin and namespaced as `<plugin-id>/<id>` by the host.
- `api` must be a version the host has bindings for; otherwise the plugin is refused with a clear message.
- Limits are clamped to the user's maxima.

**Activation is derived from contributions; there is no `startup` event.** A plugin activates when one of these happens:
- a contributed command runs;
- a document of a contributed lens language becomes visible;
- an opener pattern matches a file being opened;
- a contributed panel is shown.

`arcode.http-client/plugin.toml`:

```toml
[plugin]
id = "arcode.http-client"
name = "HTTP Client"
version = "0.1.0"
api = "0.1"
entry = "plugin.wasm"
description = "Send requests from .http / .rest files"

[capabilities]
workspace = "read"            # none | read | read-write   -> WASI preopen /project
editor = "context"            # none | context | all-open
editor-write = false
network = { hosts = ["*"] }   # hosts it may *ask* for; gesture-bound, consent per host
# network = { hosts = [...], background = true }  # never granted by default

[limits]
memory-mb = 64                # default 64; clamped to settings.plugins.max-memory-mb (256)
task-seconds = 60             # CPU budget after host.begin-task; max 600

[[contributes.languages]]
id = "http"
name = "HTTP Request"
extensions = ["http", "rest"]
line-comment = "#"

[[contributes.commands]]
id = "send"
title = "HTTP: Send Request at Cursor"
when = "language == http"     # 0.1 supports only `language == <id>`
key = "secondary-enter"

[[contributes.commands]]
id = "select-environment"
title = "HTTP: Select Environment"
when = "language == http"

[[contributes.code-lenses]]
languages = ["http"]

[[contributes.panels]]
id = "response"
title = "Response"
location = "editor-beside"    # sidebar | bottom | editor | editor-beside
icon = "globe"                # host icon name

[[contributes.settings]]
key = "timeout-ms"
type = "integer"              # boolean | integer | string | enum
default = 30000
description = "Request timeout"

[[contributes.settings]]
key = "follow-redirects"
type = "boolean"
default = true
```

`arcode.db-viewer/plugin.toml`:

```toml
[plugin]
id = "arcode.db-viewer"
name = "Database Viewer"
version = "0.1.0"
api = "0.1"
entry = "plugin.wasm"
description = "Browse and query SQLite databases in the project"

[capabilities]
workspace = "read"            # 0.1 is read-only by design (see §12)
editor = "none"

[limits]
memory-mb = 128
task-seconds = 300            # long queries, cancellable

[[contributes.openers]]       # opening a matching file shows the plugin view, not the binary notice
id = "sqlite"
patterns = ["*.sqlite", "*.sqlite3", "*.db"]
title = "SQLite Database"

[[contributes.panels]]
id = "databases"
title = "Databases"
location = "sidebar"          # adds an activity-bar entry
icon = "database"

[[contributes.commands]]
id = "refresh"
title = "DB: Refresh Schema"

[[contributes.commands]]
id = "new-query"
title = "DB: New Query"

[[contributes.settings]]
key = "page-size"
type = "integer"
default = 200

[[contributes.settings]]
key = "max-result-rows"
type = "integer"
default = 100000
```

---

## 4. Permission and consent model

A grant is keyed by `(plugin, capability[, host])`. Its state is `ask`, `allow` or `deny`, and its scope is `once`, `project` or `global`. Deciding a grant is a pure state machine (`grants.rs`) and is unit-tested exhaustively.

| Capability | Manifest | Default | When the user is asked |
|---|---|---|---|
| log, notify, status, quick-pick, ui, kv, own settings | implicit | allow | never |
| editor, document the call is about | `editor = "context"` | allow | never. "Context" means the lens/command origin document, or a document of a language the plugin contributes. |
| editor, any open document | `editor = "all-open"` | ask | at enable; labelled high-risk |
| editor write | `editor-write = true` | ask | first use per project |
| workspace read | `workspace = "read"` | ask | first activation in a project, before the instance is created |
| workspace write | `workspace = "read-write"` | ask | same prompt, separate checkbox. The user may grant read only. |
| network, user-initiated | `network.hosts` | ask | first `send` per host: **Allow once / This project / Always / Deny** |
| network, background | `network.background = true` | **deny** | only via an explicit toggle in Plugins → plugin → Permissions |
| process, env vars, raw sockets, clipboard, files outside the project | not offered in 0.1 | – | – |

**Static vs dynamic.**
- Files are a *static* grant: they become the WASI preopen (`DirPerms`/`FilePerms`) when the instance is created. Revoking one recycles the instance.
- Network and editor-write are *dynamic*: they are checked in host code on every call.

**User gesture.** The host stamps each dispatched call with `gesture: bool`.

These calls carry `gesture = true`:
- `handle-command` invoked from the palette, a keybinding, a lens click or a status-item click;
- `handle-ui-event` for `click`, `submit`, `select`, `tab`, `sort`, `row-activate` and `tree-activate`.

These calls never carry it:
- `activate`, `provide-code-lenses`, `open-view`, `fetch-rows`, `view-closed`;
- `change` and `tree-toggle` events.

The gesture covers only host calls nested inside that call, and at most 64 network requests (enough for "send all in file").

`http.send` outside a gesture returns `no-user-gesture` unless background network is granted. So "the HTTP plugin sends only when you click Send" is **enforced by the host**.

**AI plugins are off by default without special-casing.** An AI plugin needs `network.background` and usually `editor = "all-open"`. Such a plugin always installs disabled. Background network is never granted implicitly. A status-bar dot shows while any plugin holds a background-network grant. Finally, `settings.plugins.network = false` is a master switch that turns off all plugin networking.

**Prompts are host chrome**, never drawn inside a plugin panel.
- The host, method and port shown come from the parsed URL, not from plugin text.
- Plugin-supplied strings appear only quoted, as plain text, and are length-capped.
- A plugin with both `workspace` and `network` gets the combined wording: "can read this project and send data to hosts you approve".
- While a prompt is open, the plugin's thread waits on a oneshot. The UI thread never waits on a plugin.

**Visibility and revocation.** Plugins → plugin → Permissions lists every grant with a Revoke button. An in-memory **Network log** lists every request each plugin made: time, method, host and status. Bodies are not logged.

**Repository trust.** No file in a project can grant, install or enable anything. A test covers a malicious `.arcode/plugin-grants.toml`.

**WASI context** (`wasi.rs`):
- no env, args or stdin;
- stdout and stderr go to the plugin log (line-buffered, 1 MiB ring buffer);
- clocks and random allowed;
- `allow_tcp(false)`, `allow_udp(false)`, `allow_ip_name_lookup(false)`, and a socket address check that always denies;
- preopen `/project` only when granted.

wasmtime-wasi's cap-std directories refuse `..`, absolute-path and symlink escapes.

**Load-time import check** (`imports_check.rs`) walks `component.component_type().imports()`:
- Allowed: `arcode:plugin/*@0.1.x` and `wasi:{cli,clocks,random,io,filesystem,sockets}@0.2.x`.
- Refused: anything else, including `wasi:http`.
- Refused: a component that imports `arcode:plugin/http` while the manifest declares no `network`.

---

## 5. Threading, budgets, cancellation

```
GPUI main thread --Dispatch{call, gesture, cancel, budget}--> plugin-<id> thread (owns Store)
      ^                                                          |
      +---- PluginOutput (async-channel, drained by a GPUI task) <+  ui snapshots, results, prompts
```

**Instance thread.**
- Activation spawns `plugin-<id>` with a 2 MiB stack.
- The thread owns the `Store` and blocks on `mailbox.recv_timeout(10 min)`, so it uses 0 % CPU while idle. Calls into one instance are serial: guest code is single-threaded.
- If the timeout fires while the plugin has no open panel, the thread calls `deactivate` (100 ms budget), drops the `Store` and exits. That is idle unload, with no extra timer.

**The UI never waits on a plugin.** Every call is dispatched with a reply channel that a GPUI task awaits. Results arrive through the same output channel the LSP uses, with no polling.

**Queue hygiene.**
- `provide-code-lenses` is debounced 300 ms after the last edit. It runs only for visible editors and coalesces per document, so only the newest version is kept.
- `fetch-rows` coalesces per table, latest visible range wins.
- The mailbox is bounded at 64; when full, the oldest coalescable item is dropped.

**Editor reads never hop to the UI thread.**
- While any plugin is active, the app publishes `DocSnapshot { id, path, language, version, rope, selections }` after each edit to a shared `Arc<RwLock<HashMap<DocId, DocSnapshot>>>`. Cloning a ropey `Rope` is O(1).
- `editor.text` reads that map.
- `apply-edits`, `open-file` and `open-scratch` are messages to the UI thread. The plugin thread waits up to 1 s for the reply. `apply-edits` checks `expected-version` and becomes one undo step.

**CPU budget with epochs.**
- One global ticker thread calls `engine.increment_epoch()` every 10 ms **only while `in_flight > 0`**. Otherwise it parks on a `Condvar`.
- Each call sets `epoch_deadline_callback`, which returns `UpdateDeadline::Continue(1)`. The callback can only fire while wasm is running, so the number of callback invocations counts 10 ms *slices of guest execution*. Time spent in host imports (HTTP, consent prompts, quick-pick) does not count. The callback traps when the slices exceed the budget, or when hard cancel is set.

| Export | Guest CPU budget | What happens on overrun |
|---|---|---|
| `activate` | 1 s | Activation fails and a notice is shown. |
| `provide-code-lenses` | 100 ms | The result is dropped. After 3 strikes, lenses are off for the session. |
| `open-view`, `handle-ui-event`, `fetch-rows` | 1 s | Trap. |
| `handle-command` | 2 s | Trap. |
| any call after `begin-task` (gesture only) | `limits.task-seconds` | Trap. The status bar shows "title… [Cancel]". |

**Cancellation, cooperative first.**
1. Cancel (the task chip, the Esc key, or closing the panel) sets the call's token.
2. `host.cancelled()` returns true. The DB viewer checks it from SQLite's progress handler.
3. Blocking imports return `cancelled`. `http.send` runs ureq on a helper thread and selects on the result or the token.
4. After a 2 s grace period the epoch callback traps, as a hard cancel.

**Memory.**
- `StoreLimitsBuilder` sets `memory_size` to the manifest value (default 64 MiB, user cap 256 MiB, hard cap 512 MiB).
- It also caps `memories`, `tables` and `instances`; exact values are measured in Step 3, starting at 2, 8 and 16.
- `table_elements` is 100k, and `trap_on_grow_failure(false)`, so the guest allocator sees the failure inside its own sandbox.

**Host-side caps:**
- UI: 20k nodes per panel, depth 64, 8 MiB of strings per `render` or `apply-patches`, 4 MiB per row page.
- Display: cells are shown truncated to 4 KiB.
- HTTP bodies: 50 MiB, then `truncated`.
- kv: 1 MiB per scope.
- Notifications: 5 per minute.

**Crashes.**
- Any trap poisons the `Store`, so we drop it.
- The plugin's panels show "HTTP Client stopped: exceeded 2 s CPU [Restart]".
- The next event re-instantiates the plugin with `activate(restore)` and `open-view` for each panel that is still open.
- Three traps in 10 minutes disable the plugin for the session.

**Accepted residual risk:** wasmtime runs in-process. The host crate talks to the app only through channels and traits, so moving it out of process later is mechanical.

---

## 6. WIT: `crates/arcode-plugin-api/wit/arcode-plugin.wit`

This is the complete 0.1 API. It was validated with `wit-parser` 0.243.

```wit
package arcode:plugin@0.1.0;

interface types {
  type doc-id = u64;
  type panel-id = u32;
  type node-id = u32;
  /// JSON text; the SDK (de)serializes it with serde.
  type json = string;
  /// Zero-based. `column` counts chars, like arcode-buffer.
  record position { line: u32, column: u32 }
  record range { start: position, end: position }
  enum level { trace, debug, info, warn, error }
}

interface host {
  use types.{level, json};
  /// "0.1.0"
  api-version: func() -> string;
  /// Goes to the plugin's log in the Plugins view (ring buffer, 1 MiB).
  log: func(level: level, message: string);
  /// Toast, plain text. Rate-limited by the host (5 per minute).
  notify: func(level: level, message: string);
  /// The plugin's single status-bar item; none clears it. Clicking it runs
  /// `command` (a contributed command id) as a user gesture.
  set-status: func(text: option<string>, command: option<string>);
  /// Host-drawn quick pick. Blocks this plugin until the user picks or
  /// escapes; the wait does not count against the CPU budget.
  quick-pick: func(title: string, items: list<string>) -> option<u32>;
  /// JSON object: manifest defaults merged with the user's values.
  settings: func() -> json;
  /// True once the user cancelled the current call. Cheap; poll it in loops.
  cancelled: func() -> bool;
  /// Turns the current call into a visible task: "<title>... [Cancel]" in the
  /// status bar and the CPU budget rises to limits.task-seconds.
  /// Ignored (returns false) outside a user-gesture call.
  begin-task: func(title: string) -> bool;
}

interface kv {
  enum scope { global, project }
  variant kv-error { quota-exceeded, invalid-key }
  /// Keys: 1..=128 bytes, printable ASCII. Quota: 1 MiB per (plugin, scope).
  get: func(scope: scope, key: string) -> option<list<u8>>;
  set: func(scope: scope, key: string, value: list<u8>) -> result<_, kv-error>;
  delete: func(scope: scope, key: string);
  keys: func(scope: scope, prefix: string) -> list<string>;
}

/// File *contents* go through WASI: the project is preopened at /project.
/// This interface only adds what WASI cannot do cheaply.
interface workspace {
  variant fs-error { denied, invalid-glob(string), io(string) }
  /// Display name of the project root, never its absolute path.
  root-name: func() -> option<string>;
  /// Project-relative paths matching `glob`, honouring .gitignore,
  /// answered from the host's file index. limit is capped at 10000.
  find-files: func(glob: string, limit: u32) -> result<list<string>, fs-error>;
}

interface editor {
  use types.{doc-id, position, range};
  record doc-info {
    id: doc-id,
    /// Project-relative path; none for untitled buffers.
    path: option<string>,
    language: string,
    version: u64,
    line-count: u32,
  }
  record text-edit { range: range, text: string }
  variant editor-error { denied, no-such-document, version-mismatch(u64), invalid-range, invalid-path }
  /// The document this call is about (lens or command origin), else the focused one.
  current-document: func() -> option<doc-info>;
  document-info: func(doc: doc-id) -> result<doc-info, editor-error>;
  /// Read from a rope snapshot; never waits on the UI thread.
  text: func(doc: doc-id, range: option<range>) -> result<string, editor-error>;
  selections: func(doc: doc-id) -> result<list<range>, editor-error>;
  /// One undo step. Fails if the document moved past expected-version.
  /// Returns the new version.
  apply-edits: func(doc: doc-id, expected-version: u64, edits: list<text-edit>) -> result<u64, editor-error>;
  open-file: func(path: string, at: option<position>) -> result<_, editor-error>;
  /// Untitled, unsaved buffer (e.g. "open response body in editor").
  open-scratch: func(title: string, language: string, text: string) -> result<doc-id, editor-error>;
}

interface http {
  record request {
    method: string,
    url: string,
    headers: list<tuple<string, string>>,
    body: list<u8>,
    timeout-ms: u32,
    follow-redirects: bool,
    /// Clamped by the host (default and maximum 50 MiB).
    max-response-bytes: u64,
  }
  record timing { headers-ms: u32, total-ms: u32 }
  record response {
    status: u16,
    reason: string,
    version: string,
    final-url: string,
    remote-addr: option<string>,
    headers: list<tuple<string, string>>,
    body: list<u8>,
    truncated: bool,
    timing: timing,
  }
  variant http-error {
    denied,
    no-user-gesture,
    cancelled,
    timeout,
    invalid-request(string),
    dns(string),
    connect(string),
    tls(string),
    protocol(string),
    io(string),
  }
  /// Blocks this plugin's thread only. Consent is checked per host, again
  /// on every redirect. Allowed only inside a user-gesture call unless the
  /// user granted network.background.
  send: func(req: request) -> result<response, http-error>;
}

interface ui {
  use types.{panel-id, node-id};

  enum tone { normal, muted, accent, success, warning, danger }
  enum axis { horizontal, vertical }
  enum location { sidebar, bottom, editor, editor-beside }

  record stack-props { axis: axis, gap: u8, padding: u8 }
  record split-props { axis: axis, ratio: f32 }
  record tabs-props { titles: list<string>, active: u32 }
  record text-props { text: string, tone: tone, mono: bool, bold: bool, wrap: bool }
  record badge-props { text: string, tone: tone }
  record button-props { label: string, icon: option<string>, primary: bool, enabled: bool }
  record input-props {
    value: string,
    placeholder: string,
    multiline: bool,
    /// Highlight with this arcode-syntax language (e.g. "sql").
    language: option<string>,
    submit-on-enter: bool,
    /// Opt in to debounced change events. Off: values ride along with the next event.
    notify-changes: bool,
  }
  record select-props { options: list<string>, selected: option<u32> }
  record checkbox-props { label: string, checked: bool }
  record column { title: string, width: option<u32>, numeric: bool, sortable: bool }
  record table-props {
    columns: list<column>,
    /// none = unknown: the host pages until a page reports at-end.
    row-count: option<u64>,
    /// Bump to drop every cached page (new query, new sort).
    generation: u32,
  }
  record tree-item {
    key: string,
    label: string,
    detail: option<string>,
    icon: option<string>,
    depth: u8,
    expandable: bool,
    expanded: bool,
  }
  record code-props { text: string, language: option<string>, line-numbers: bool }

  variant widget {
    stack(stack-props),
    split(split-props),
    tabs(tabs-props),
    scroll,
    text(text-props),
    badge(badge-props),
    button(button-props),
    input(input-props),
    select(select-props),
    checkbox(checkbox-props),
    key-values(list<tuple<string, string>>),
    table(table-props),
    tree(list<tree-item>),
    code(code-props),
    /// Raw JSON text; the host parses it off the UI thread and shows a foldable view.
    json(string),
    spinner(string),
    /// (title, hint)
    empty-state(tuple<string, string>),
    divider,
    spacer,
  }

  /// Flat arena: WIT has no recursive types. Children are ids in the same panel.
  record node { id: node-id, widget: widget, children: list<node-id>, grow: bool }
  record view { root: node-id, nodes: list<node> }

  variant patch {
    /// Insert or replace one node (its children list included).
    upsert(node),
    /// Remove a node and every descendant no longer referenced.
    remove(node-id),
    set-root(node-id),
    /// Drop cached pages of a table; optionally set a new row count.
    rows-changed(tuple<node-id, option<u64>>),
  }

  variant cell { empty, int(s64), real(f64), text(string), boolean(bool), blob(u64) }
  record row-page { start: u64, rows: list<list<cell>>, at-end: bool }

  variant event-kind {
    click,
    submit,
    change(string),
    toggle(bool),
    select(u32),
    tab(u32),
    /// (column, ascending)
    sort(tuple<u32, bool>),
    row-activate(u64),
    tree-toggle(string),
    tree-activate(string),
  }
  record event {
    node: node-id,
    kind: event-kind,
    /// Current text of every input edited since the previous event.
    values: list<tuple<node-id, string>>,
  }

  variant ui-error { unknown-contribution, no-such-panel, invalid-tree(string), too-large }

  /// Opens (or focuses) a panel of a manifest-declared contribution. Never waits on the UI.
  open-panel: func(contribution: string, title: string, location: option<location>) -> result<panel-id, ui-error>;
  /// Replace the whole view.
  render: func(panel: panel-id, view: view) -> result<_, ui-error>;
  /// Validated and applied atomically; on error nothing changes.
  apply-patches: func(panel: panel-id, patches: list<patch>) -> result<_, ui-error>;
  set-title: func(panel: panel-id, title: string);
  close-panel: func(panel: panel-id);
}

interface guest {
  use types.{panel-id, node-id, json};
  use editor.{doc-info};
  use ui.{event, row-page};

  variant activation {
    command(string),
    language(string),
    opener(string),
    panel(string),
    /// Re-created after an idle unload or a crash, with views to restore.
    restore,
  }
  record code-lens { line: u32, title: string, command: string, args: json }

  activate: func(reason: activation) -> result<_, string>;
  deactivate: func();
  handle-command: func(command: string, args: json) -> result<_, string>;
  provide-code-lenses: func(doc: doc-info) -> list<code-lens>;
  /// A contributed panel or opener was shown in `panel`. args: {"path": ...} for openers.
  open-view: func(panel: panel-id, contribution: string, args: json) -> result<_, string>;
  handle-ui-event: func(panel: panel-id, event: event) -> result<_, string>;
  fetch-rows: func(panel: panel-id, table: node-id, start: u64, count: u32) -> result<row-page, string>;
  view-closed: func(panel: panel-id);
}

world plugin {
  // The host also links a locked-down WASI 0.2 (cli, clocks, random, io,
  // filesystem with preopens only, sockets present but always denied).
  import host;
  import kv;
  import workspace;
  import editor;
  import http;
  import ui;
  export guest;
}
```

The host side is `wasmtime::component::bindgen!({ path: "../arcode-plugin-api/wit", world: "plugin", trappable_imports: true })`, sync.

Every import is implemented in `host_impl/*.rs`. Each one first checks the capability and the gesture, then does its work.

---

## 7. UI contribution model

Plugins never touch GPUI. They describe a view, and the host renders it with `arcode-ui-kit` widgets, so plugin UI follows the theme and low-end mode automatically.

**Pipeline.**
1. The guest SDK builds an ordinary recursive Rust tree with string keys.
2. The SDK maps keys to stable `node-id`s and diffs against the panel's previous tree **inside the guest**. It then emits `render` the first time and `apply-patches` afterwards, so a status change sends one `upsert`. Diffing costs plugin CPU, never UI-thread time.
3. `arcode-plugin-host::ui_tree::PanelTree` (no GPUI) validates the patch set on the plugin thread and applies it atomically. Validation rejects dangling ids, cycles, wrong child counts and cap violations.
4. The app receives `Arc<PanelSnapshot>` plus the dirty node ids. The panel's `PluginPanelView` entity calls `cx.notify()` once. Element ids are `(panel, node-id)`.

**Uncontrolled inputs.**
- The host owns the text being edited, the scroll position, split ratios, the active tab and tree expansion. There is no per-keystroke traffic.
- Each event carries the current value of every input edited since the previous event. A Run click therefore delivers the SQL text in the same call.
- A re-render overwrites an input only if the plugin changed that input's `value` since its last render.

### Widget schema

| Widget | Children | Events | Host behaviour |
|---|---|---|---|
| `stack` | any | – | flex row or column, `gap`/`padding` in 4 px units |
| `split` | exactly 2 | – | draggable ratio, kept on the host |
| `tabs` | one per title | `tab(i)` | switches locally, then reports the change |
| `scroll` | 1 | – | |
| `text`, `badge` | 0 | – | always selectable and copyable; plain text, never markup |
| `button` | 0 | `click` | gesture |
| `input` | 0 | `submit`, `change` (opt-in, debounced 300 ms) | multiline input with `language` is a small `arcode-editor` with highlighting |
| `select` / `checkbox` | 0 | `select(i)` / `toggle(b)` | |
| `key-values` | 0 | – | two-column, copyable |
| `table` | 0 | `row-activate`, `sort` | virtualized `uniform_list` (see below) |
| `tree` | 0 | `tree-toggle`, `tree-activate` | flat list with `depth`, virtualized; shows expansion at once with a spinner until the plugin patches in the children |
| `code` | 0 | – | read-only `arcode-editor`, so it is virtualized and highlighted for free |
| `json` | 0 | – | parsed off-thread with serde_json (depth ≤ 128), lazily folded; above 20 MiB shown as `code` |
| `spinner`, `empty-state`, `divider`, `spacer` | 0 | – | |

**Big tables never cross the boundary in full.**
- A `table` declares its columns and `row-count` (`none` means unknown).
- The host requests 200-row pages around the viewport through `fetch-rows`, prefetches one page ahead, and keeps an LRU of 6 pages per table. At most a few hundred rows are ever in host memory.
- `rows-changed` or a new `generation` drops the cache.
- Scrolling a million-row table moves at most two pages per frame's worth of scrolling. Fetches are coalesced, latest range wins.

There is deliberately no drawing, canvas, HTML or webview API. New widgets are added in the host when a real plugin needs one.

---

## 8. Guest SDK (`arcode-plugin-api`)

```rust
use arcode_plugin::{prelude::*, ui::*};

#[derive(Default)]
struct HttpClient { panel: Option<Panel>, last: Option<Exchange> }

impl Plugin for HttpClient {
    fn code_lenses(&mut self, doc: &DocInfo) -> Vec<CodeLens> {
        let Ok(text) = editor::text(doc.id, None) else { return vec![] };
        parse::requests(&text)
            .map(|r| CodeLens::new(r.line, "▶ Send request", "send", &SendArgs { doc: doc.id, line: r.line }))
            .collect()
    }
    fn command(&mut self, id: &str, args: Json) -> Result<()> {
        match id {
            "send" => self.send(args.parse()?),
            "select-environment" => env::select(),
            _ => Ok(()),
        }
    }
}
arcode_plugin::export_plugin!(HttpClient);
```

**What the SDK provides:**
- The `Plugin` trait with default implementations for every export, so authors implement only what they contribute.
- Typed args through serde.
- The `ui` builders (`vstack`, `hstack`, `tabs`, `table`, …), plus flatten and diff.
- `Panel::rerender()`.
- `TablePager`: a row-page cache helper for plugin-side result sets.
- A `testing` module with fake host imports, so plugin logic is tested natively with `cargo test`.

---

## 9. Reference plugin: HTTP client (`plugins/http-client`, ~300 KB)

**Modules:**
- `parse.rs`: `###` separators, `# @name`, request line, headers, body, and `< ./file.json` body includes.
- `vars.rs`: `@var = value`, `{{var}}`, and `{{$uuid}}`/`{{$timestamp}}`/`{{$randomInt}}`, which use WASI random and clocks.
- `env.rs`: reads `/project/http-client.env.json` and `http-client.private.env.json` with `std::fs`. The selected environment is kept in project-scope `kv`, shown with `set-status(…, "select-environment")`, and switched with `quick-pick`.
- `view.rs`, `history.rs`: the last 50 exchanges, kept in project-scope `kv`.

**Flow:**
1. Opening `a.http` activates the plugin. The first activation in this project prompts for workspace read.
2. `provide-code-lenses` reads the text with `editor.text` and caches the parse by `(doc, version)`. It returns a "▶ Send request" lens on each request line.
3. Clicking the lens calls `handle-command("send", {doc, line})` as a gesture. The plugin resolves variables and checks `host.cancelled()`.
4. `ui.open-panel("response")` renders a spinner.
5. `http.send` shows a consent prompt on the first request to each host, then blocks only this plugin's thread. The user can cancel from the task chip.
6. `apply-patches` fills in the response view:
   - a status `badge` toned by status class, plus `headers-ms`/`total-ms` and the size;
   - `tabs`: **Body** (`json` if the content type is JSON, `code` with a guessed language otherwise, a hex summary if binary), **Headers** (`key-values`), **Raw**;
   - an "Open in editor" button that calls `open-scratch`.

**API surface used:** `editor` (context), WASI fs read, `http`, `ui`, `kv`, `host` (`quick-pick`, `set-status`, `settings`, `cancelled`). JetBrains `> {% %}` response scripts are out of scope, since there is no JS engine.

## 10. Reference plugin: DB viewer (`plugins/db-viewer`, ~1.5 MB)

**Build.** It uses `rusqlite` with the `bundled` feature for `wasm32-wasip2`, built with clang 18 and wasi-libc headers. Exact flags are pinned in `scripts/build-plugins.sh` (§13, Step 0):
- `SQLITE_THREADSAFE=0`
- `SQLITE_OMIT_LOAD_EXTENSION`
- `SQLITE_DEFAULT_MEMSTATUS=0`
- `SQLITE_OMIT_SHARED_CACHE`

**Opening files.**
- A database is opened as `file:/project/<rel>?mode=ro&nolock=1`. WASI has no `fcntl` locks.
- If a `-wal` file exists, the plugin uses `immutable=1` instead and shows "snapshot; uncheckpointed WAL changes not shown". WAL needs shm, which WASI lacks.
- SQLite reads pages with `pread`, so the file is never copied in full.
- The plugin sets `PRAGMA cache_size=-4096`.
- 0.1 is **read-only**. Statements that write return SQLite's read-only error, shown in the result area.

**Views.**
- **Sidebar `databases`:** `workspace.find-files("**/*.{sqlite,sqlite3,db}", 200)` runs only when the panel is shown or refreshed. The panel is a `tree` of databases, then tables and views (from `sqlite_schema`), then columns (from `pragma table_info`, loaded on `tree-toggle`).
- **Opener / table tab:** `open-view(panel, "sqlite", {path})` renders a `split`:
  - the schema `tree` on the left;
  - `tabs` with **Data** and **Query** on the right.
- **Data tab:** a `table` whose `row-count` comes from `count(*)`. If counting takes more than 200 ms of progress-handler steps, the count is `none`. `fetch-rows` uses keyset paging on `rowid` where the table has rowids, with `LIMIT/OFFSET` as the fallback. A `sort` event re-queries with `ORDER BY` and bumps `generation`.
- **Query tab:**
  - a multiline `input` with `language = "sql"` and `submit-on-enter` (Ctrl+Enter);
  - Run and Cancel buttons;
  - a results `table`.
- **Running a query.** Run calls `begin-task("Running query")`. The SQLite progress handler polls `host.cancelled()` every 10k VM steps. Results go into a plugin-side page store, capped at `max-result-rows` and 64 MiB with a "truncated" note, and are served through `fetch-rows`. Non-SELECT statements show "N rows changed".

**API surface used:** WASI fs (read), `workspace.find-files`, `ui` (`tree`, `table`, `tabs`, `split`, `input`), `host` (`begin-task`, `cancelled`, `notify`), and `kv` (recent queries).

**Postgres and MySQL arrive in API 0.2**, through two new interfaces:
- `arcode:plugin/tcp`: a connection resource to `host:port` with blocking read/write. It is gesture-initiated, consent is asked per `host:port`, and the host does TLS so plugins don't bundle rustls.
- `arcode:plugin/secrets`: host-drawn masked input, stored mode 0600, OS keyring later.

The guest would use the sans-IO `postgres-protocol` crate. The permission model above already covers it.

---

## 11. Versioning

- The WIT package is semver'd, and the manifest `api` is checked **before** compiling.
- **0.x.** Patch releases are additive only, through new functions or new interfaces. Records never change in place. A minor bump may break; the host keeps N-1 bindings (a second `bindgen!` module with shims) for one release.
- **1.x.** Additive only. wasmtime's semver-compatible import matching links older guests.
- A guest built for a newer API than the host supports fails with "requires Arcode ≥ x".
- The SDK gives every export a default implementation, so a new export does not break existing source.

---

## 12. Crate layout

Files stay under 400 lines, as in `ARCHITECTURE.md`.

```
crates/arcode-plugin-api/            guest SDK; builds for wasm32-wasip2 and natively (tests)
  wit/arcode-plugin.wit              single source of truth (host bindgen! reads it too)
  src/lib.rs  bindings.rs  plugin.rs (trait, export_plugin!)  args.rs
  src/ui/{builder,flatten,diff}.rs   src/pager.rs  src/testing.rs
crates/arcode-plugin-host/           NO GPUI. wasmtime, wasmtime-wasi, ureq+rustls, globset, sha2, toml
  src/lib.rs        PluginManager facade (channels + traits only)
  src/manifest.rs   registry.rs   install.rs   paths.rs
  src/engine.rs     cache.rs      bindings.rs  imports_check.rs  wasi.rs
  src/instance.rs   (thread, mailbox, restore)   dispatch.rs (coalescing)
  src/budget.rs     (ticker, epoch callback)     limits.rs
  src/grants.rs     (pure state machine)  consent.rs (ConsentProvider trait)
  src/bridge.rs     (EditorBridge, ProjectIndex traits; DocSnapshot)
  src/host_impl/{host,kv,workspace,editor,http,ui}.rs
  src/ui_tree.rs    (PanelTree, validation, patches)  rows.rs (page LRU + coalescing, pure)
  src/output.rs     (PluginOutput enum)
  test-programs/    own [workspace]; fixtures listed in §14
crates/arcode/src/plugins/           GPUI glue, behind feature "plugins"
  mod.rs  manager.rs  panel_view.rs  widgets/{table,tree,json,form}.rs
  consent_dialog.rs  plugins_view.rs (activity bar: list, enable, grants, log, network log)
  lenses.rs  status.rs  task_chip.rs
crates/arcode-editor/                generic, plugin-agnostic additions:
  block decorations ("virtual line above line N", shifted on edit) + gutter actions
plugins/                             separate cargo workspace, target wasm32-wasip2
  http-client/  db-viewer/
scripts/build-plugins.sh             release build -> target/wasm-plugins/<id>/{plugin.toml,plugin.wasm}
scripts/build-test-plugins.sh        test-programs -> target/test-plugins/
```

The root `Cargo.toml` adds `exclude = ["plugins", "crates/arcode-plugin-host/test-programs"]`. A plain `cargo test` does not build wasm. Integration tests that need a fixture fail with "run scripts/build-test-plugins.sh". CI runs that script first.

---

## 13. Implementation plan

Each step lands with its tests green.

0. **Spike: weight and SQLite.**
   - Add wasmtime to a throwaway binary and measure the stripped size delta, engine + echo RSS and cold compile time; record them in `docs/SPIKE.md` and apply the §2 exit criterion.
   - Install the wasi-libc headers (`apt install wasi-libc`; the sysroot is not present on this box today).
   - Build `rusqlite`/bundled for `wasm32-wasip2`, with `CFLAGS_wasm32_wasip2="--sysroot=/usr/share/wasi-sysroot"` plus target overrides if clang 18 rejects the `wasip2` triple.
   - Fallback A: build for `wasm32-wasip1` and componentize with the wasmtime p1 reactor adapter.
   - Fallback B: compile with `SQLITE_OS_OTHER` plus a small Rust VFS over `std::fs` and `FileExt::read_at`.
   - *Test:* the spike binary opens a fixture DB and prints a row count.
1. **API crate and echo fixture.**
   - WIT file, SDK skeleton, `export_plugin!` with defaults, `scripts/build-test-plugins.sh`.
   - *Tests:* the `echo` fixture builds, and SDK flatten/diff unit tests pass natively.
2. **Manifest and registry** (no wasmtime yet).
   - *Tests:* parsing both reference manifests, unknown-key errors, id regex, limit clamping, contribution namespacing, derived activation events, discovery order (bundled < installed < dev).
3. **Engine, cache, instance thread.**
   - *Tests:* `echo` round-trips every WIT type through a fake bridge; the second load deserializes with no compile (counter); the idle instance thread is blocked on recv; idle unload after a shortened timeout; `deactivate` is called.
4. **Budgets and limits.**
   - *Tests:*
     - `spin` traps within budget + 30 ms while a second instance keeps answering;
     - `spin-cancellable` returns early on cancel;
     - `alloc-bomb` hits the limit and the host survives;
     - `crash` triggers backoff, then auto-disable;
     - `stdout-flood` hits the log cap;
     - with no calls in flight, the ticker makes zero increments over 1 s.
5. **Grants, consent, WASI, import check.**
   - *Tests:*
     - the grant state machine (once/project/global/deny, revoke);
     - a malicious project-dir grant file is ignored;
     - `fs-escape` (`../`, `/etc/passwd`, a symlink to `/etc`) is denied;
     - `fs-escape` write without a write grant gets EACCES;
     - the `sockets` fixture's `TcpStream::connect` fails;
     - `undeclared-import` (imports `http` without `network`) is refused;
     - a `wasi:http` import is refused.
6. **host, kv, workspace, editor imports.**
   - *Tests:* kv quota and scopes; `find-files` against a temp project with `.gitignore`; `editor.text` reads snapshots; `apply-edits` version mismatch; `editor = "context"` denies other documents.
7. **http import.**
   - *Tests* against a `std::net::TcpListener` on 127.0.0.1, never the external network:
     - `net` fixture: send in `activate` returns `no-user-gesture`;
     - send in a gesture command asks consent, and once/always/deny each behave correctly;
     - a redirect to a new host re-prompts;
     - cancel during a slow response returns within 100 ms;
     - the body cap sets `truncated`;
     - the master switch denies everything.
8. **UI tree and rows** (pure).
   - *Tests:* `PanelTree` validation (cycles, dangling ids, caps, split child count); atomic patches; the row LRU and coalescing; an SDK property test (seeded random trees: render → diff → apply equals a fresh render); `ui-big` fetches only the requested pages.
9. **App integration.**
   - `plugins/` module, `panel_view`, widgets, consent dialog, Plugins view, task chip, status item, openers in the file tree.
   - `arcode-editor`: gutter actions first, then block decorations for above-line lenses.
   - *Tests:* `arcode-editor` layout tests (block rows shift line y-offsets, survive edits); non-GPUI logic in the host crate; a manual acceptance checklist in this doc's appendix.
10. **HTTP client plugin.**
    - *Tests:* native `parse`/`vars`/`env` unit tests (JetBrains samples); an end-to-end host test loads `http-client.wasm` with a scripted `ConsentProvider` against the loopback server and checks status, headers, JSON detection, timing fields and cancel.
11. **DB viewer plugin.**
    - *Tests:* native SQL-paging builder tests; an end-to-end test against a checked-in 64 KB fixture (tree, rows, query, error) and a 1M-row DB generated by the plugin's native test (deep paging stays under 1 s per page, cancel during `GROUP BY`); WAL fallback.
12. **Ship.**
    - Bundle into `share/arcode/plugins/` in release packaging.
    - Benchmark (`examples/plugin_bench.rs`): activation from cache under 30 ms; RSS under 5 MB (HTTP) and around 15 MB (DB).
    - Write the author guide in `crates/arcode-plugin-api/README.md`.

---

## 14. Testing summary

| Layer | How |
|---|---|
| Pure logic | Unit tests in `arcode-plugin-host` (manifest, grants, ui_tree, rows, imports_check) and in the SDK (flatten, diff). |
| Sandbox behaviour | Fixture guests in `test-programs/`: `echo`, `spin`, `spin-cancellable`, `alloc-bomb`, `crash`, `stdout-flood`, `fs-escape`, `sockets`, `net`, `undeclared-import`, `ui-big`. Each checks one behaviour. |
| Plugin logic | Plain native `cargo test` in `plugins/*`, using the SDK's `testing` fakes. |
| End-to-end | The built reference plugins under a headless `PluginManager` with a fake `EditorBridge`, a scripted `ConsentProvider`, a loopback HTTP server and SQLite fixtures. |
| Idle cost | Counter assertions: zero ticker increments, zero dispatches, instance thread blocked. |

No test touches the external network.

---

## 15. Rejected alternatives

| Alternative | Why rejected |
|---|---|
| wasmi | No component model. 10–50× slower, which makes SQLite and multi-MB JSON unusable on old laptops. |
| extism | An untyped bytes ABI on top of wasmtime anyway. No size win, and we would lose WIT typing. |
| Core wasm with a hand-made ABI | Saves about 1 MB and loses the typed, versioned contract and the WASI filesystem sandbox. |
| Async wasmtime with tokio actors | Fibers and async plumbing for no user-visible gain. Blocking only the plugin's own thread is simpler. |
| `http.start` with an `on-http-response` callback | Splits every request across two exports. A blocking `send` with host-owned cancel is simpler for authors. |
| `wasi:http` / `wasmtime-wasi-http` | There is no hook for per-host, gesture-bound consent, redirect re-checks or timing. It is refused at load. |
| `wasi:sockets` enabled in 0.1 | Same consent problem. Postgres gets a host-mediated `tcp` interface in 0.2. |
| A second file API next to WASI | Two enforcement paths to keep consistent. The preopen alone also serves C libraries like SQLite. |
| Process-spawn capability | It makes the sandbox meaningless. |
| Plugin-drawn UI (canvas, HTML, webview) | Heavy, slow on iGPUs, and lets plugins spoof host chrome. |
| Recursive UI types in WIT | Not expressible in WIT. A flat keyed arena also makes patches trivial. |
| Plugin-supplied icons or images | Decoding untrusted images. Host icon names only. |
| Markdown, blob and secrets APIs in 0.1 | No reference plugin needs them. They are added when one does. |
| `startup` activation, timers, subscriptions | Each creates idle cost or polling. |
| A file watcher for dev reload | Idle cost. "Reload Plugin" is explicit. |
| Grants or plugin lists inside the repo (`.arcode/`) | A cloned repository could pre-authorize itself. |
| Registry or update checks in 0.1 | Network access without a user action. |
| Pooling allocator | Reserves memory up front. |
| wasmtime `cache` feature | Pulls zstd and a TOML config. Our own 30-line cache is enough. |
| Files outside the project via instance recycle with an extra preopen | Deferred to 0.2. 0.1 needs only project files. |
| DB writes in 0.1 | WASI has no file locks, so concurrent writers could corrupt data. Read-only until a locking story exists. |
| Out-of-process plugin host | Costs extra RSS. It stays possible because the host crate is channel- and trait-only. |

## 16. Risks

1. **SQLite on wasip2.** The toolchain is a gap on this box: the sysroot is not installed, the Ubuntu wasi-libc package is a 2023 p1 snapshot, and clang 18 may not support the wasip2 triple. *Mitigation:* Step 0 runs first, with two fallbacks.
2. **wasmtime weight on old hardware.** *Mitigation:* measured exit criterion, the feature flag, lazy engine, AOT cache and serial compilation.
3. **Code lenses need block decorations** in `arcode-editor`, which do not exist yet. *Mitigation:* the gutter ▶ ships first and the API does not change.
4. **Blocking `send` serializes the plugin.** UI events wait while a request is in flight. *Mitigation:* the host owns the spinner and cancel. Component-model async (WASI p3) can come in a later API minor version.
5. **Budget accounting** could kill plugins falsely during long host calls. *Mitigation:* slice counting plus the `spin` and `net` tests.
6. **Exfiltration.** A plugin with project read and an approved host can send project data there. That is inherent. *Mitigation:* combined-risk wording, per-host consent, the gesture rule and the network log.
7. **Consent fatigue.** *Mitigation:* project and global scopes, plus prompts only on first use.
8. **WIT churn before 1.0.** *Mitigation:* a small surface, the N-1 shim policy, and pinned toolchains.
9. **Widget scope creep.** *Mitigation:* a widget is added only for a shipped plugin.

---

## Appendix A: how this design was chosen

Three proposals were scored from 1 to 10.

| | Coverage | Security | Ergonomics | Lightness | Implementability | Total |
|---|---|---|---|---|---|---|
| A (security) | 9 | 9 | 6 | 7 | 6 | 37 |
| B (power) | 9 | 7 | 9 | 6 | 5 | 36 |
| C (lean) | 8 | 8 | 7 | 9 | 8 | 40 |

**Base: C.** It has sync threads, zero idle cost, a single WASI file path, derived activation, and it correctly spotted the missing sysroot. Its WIT did not parse, because `patch` was both a type and a function in `ui`. It also lacked kv and file search.

**From A:**
- the load-time import cross-check;
- host-enforced gesture rule and host-owned plain-text prompts;
- grants never stored in the repository;
- hash check on activation;
- crash backoff;
- snapshot-based editor reads;
- table `generation`.

**From B:**
- the recursive-builder SDK with key diffing in the guest;
- uncontrolled inputs whose values ride along with events;
- typed `cell`s, `sort` events and openers;
- quick-pick and status-item commands;
- the master network switch and network log;
- the slice-counting CPU budget.

**Dropped from A:** the async `http.start` callback split and the duplicate file API.

**Dropped from B:** hyper with a custom timing connector, markdown, blobs, secrets, SVG icons and the large widget set.

---

## Appendix B: implementation notes and deviations (recorded during Steps 0-2)

> **Added during implementation.** Everything above is the accepted design;
> this appendix only records what the code does differently and why, so the
> design text stays readable. Numbers are in `docs/SPIKE.md`, "Plugin engine".

**Engine and WASI (Step 0 findings, affect Step 3+):**
- `wasmtime-wasi` 50's `p2` feature turns on `wasmtime/async` (and pulls
  tokio). We still use only the sync API (`add_to_linker_sync`); the feature
  list in §2 is the minimum *we* request.
- `Config::parallel_compilation` exists only with the `parallel-compilation`
  feature and `Config::wasm_threads` only with `threads`. With the §2 feature
  set compilation is already serial and threads are unavailable, so those two
  calls are dropped rather than set to `false`.
- `wasmtime-wasi` 50 replaced `DirPerms`/`FilePerms` with
  `FsPerms::{ReadOnly, ReadWrite}` for `preopened_dir`. §4 "static grants"
  maps onto that.
- rustc 1.97's `wasm32-wasip2` guests import WASI `@0.2.9`; the host links
  WASI by semver. `Linker::define_unknown_imports_as_traps` looks names up
  exactly and would shadow those semver-matched WASI imports with traps, so
  host code must not use it on plugin components.
- Every plugin component imports `arcode:plugin/types`, `arcode:plugin/editor`
  and `arcode:plugin/ui`, even if it calls none of their functions, because
  the guest exports `use` their types (`doc-info`, `event`, `row-page`).
  Unused interfaces (`http`, `kv`, `workspace`) are pruned by the component
  linker. So the §4 import check works for `http`, but the presence of
  `editor`/`ui` must not be read as a capability use (editor access stays a
  dynamic check).

**SQLite toolchain (Step 0, §10, §16 risk 1):**
- `rusqlite` 0.41.1 / `libsqlite3-sys` 0.39.1 `bundled` builds for
  `wasm32-wasip2` with clang 18 directly; fallbacks A (p1 + adapter) and B
  (`SQLITE_OS_OTHER` + Rust VFS) were not needed.
- Ubuntu's `wasi-libc` has no `/usr/share/wasi-sysroot`; headers live in
  `/usr/include/wasm32-wasi`. clang 18 accepts `--target=wasm32-wasip2` but
  then silently falls back to the host's glibc headers, so the pinned flags
  are `-nostdlibinc -isystem <wasi include>` (not `--sysroot`). Only headers
  are used; rustc links its own self-contained wasi-libc.
- `libsqlite3-sys` already sets `SQLITE_THREADSAFE=0` (plus the
  `_WASI_EMULATED_*` defines) for `wasm32-wasi*`; the other §10 flags go in
  through `LIBSQLITE3_FLAGS`. All of this lives in `scripts/build-plugins.sh`.

**API crate (Step 1):**
- `wit-bindgen` is pinned `=0.63.0` with default features off
  (`macros`, `realloc`): no async runtime in guests.
- The package is `arcode-plugin-api`, its library name is `arcode_plugin`
  (as in the §8 example).
- Imports are reached through SDK modules (`host`, `kv`, `workspace`,
  `editor`, `http`, `ui::Panel`). In wasm they are the generated bindings; on
  native targets the same functions are the `testing` fakes, so plugin code
  is identical in both. wit-bindgen generates `Display` (Debug-based) for the
  error variants; the SDK does not override it.
- `Plugin` methods: `activate`, `deactivate`, `command`, `code_lenses`,
  `open_view`, `ui_event`, `fetch_rows`, `view_closed`, all with defaults.
  The default `command`/`open_view` return an "unknown …" error rather than
  `Ok`, so a missing handler is visible.
- UI keys: an element's key is its explicit `.key(..)` or its sibling index;
  ids are allocated per key *path*, are never reused after removal, and
  duplicate explicit sibling keys are an error. Patch order is upserts,
  removes (top-most removed nodes only), then `set-root`.
- Fixture output layout: `target/test-plugins/<fixture>/{plugin.toml,plugin.wasm}`
  (override with `ARCODE_TEST_PLUGINS`). `echo` has one command, `echo`,
  whose JSON args select an operation (`{"op": "kv", ...}`) and reports
  results as JSON lines through `host.log`; it declares
  `network = { hosts = ["127.0.0.1"] }` because it imports `http`.

**Manifest and registry (Step 2) decisions the design left open:**
- `[[contributes.settings]]` requires `default` (it must match `type`);
  `type = "enum"` takes `options = [...]` and the default must be one of them.
- Language ids are global (shared with arcode-syntax and `when` clauses), not
  namespaced; commands, panels, openers and setting keys are namespaced as
  `<plugin-id>/<id>`.
- The lens activation event is named `LensLanguage` (only
  `contributes.code-lenses` languages activate a plugin, not
  `contributes.languages`).
- Opener patterns without `/` match the file name; with `/` they match the
  project-relative path and `*` does not cross directories.
- Bundled and installed plugins must sit in a folder named after their id,
  and their `plugin.toml` must be a regular file (no symlink); dev folders
  are exempt. `plugin.toml` is capped at 256 KiB.
- User-visible strings (name, titles, descriptions, options) are capped
  (64/128/512 chars) and may not contain control characters; `entry` must be
  a plain `*.wasm` file name; `network.hosts` entries are `*`, a host name,
  `*.suffix`, an IP literal (`[v6]`), optionally with `:port`, and the list
  must not be empty.
- Limits clamp to `1..=user max`; the user max is itself clamped to the hard
  caps (512 MiB, 600 s).

## Appendix B (continued): Steps 3-5

> **Added during implementation** of the engine, budgets and permissions
> (`crates/arcode-plugin-host`). Same rule as above: the design text is
> unchanged, this lists what the code does differently or decides where the
> design was silent.

**Bindings and API shape (Step 3):**
- wasmtime 50 renamed `bindgen!`'s `trappable_imports: true` to
  `imports: { default: trappable }`; the generated types also derive
  `PartialEq` (`additional_derives`). They are plain data and are re-exported
  from `bindings.rs` (`bindings::{ui, editor, http, kv, workspace, guest}`)
  instead of being mirrored by hand, so `output.rs` and the app use them
  directly. Only `engine.rs`, `cache.rs`, `live.rs`, `limits.rs`, `wasi.rs`
  and `bindings.rs` name wasmtime types.
- `PluginManager` (the §12 `lib.rs` facade) is not written yet. Steps 3-5
  expose the pieces it will own: `Runtime` (lazy engine, linker, ticker,
  `.cwasm` cache, master network switch), `InstanceHandle` (one plugin:
  mailbox, thread, panels, log, counters) and `Services` (the app's traits
  plus the `PluginOutput` sender). `InstanceHandle::dispatch` returns a
  `Pending { id, cancel, result }`; `result` is an `async-channel` receiver.
- `bridge.rs` gained `HttpTransport` (step 7 implements it with ureq). The
  host does every check (master switch, gesture, `network.hosts`, per-host
  consent) before calling it and passes an `allow_redirect(host, port)`
  callback so consent is re-checked per hop without the transport knowing
  about grants.
- "Activation spawns `plugin-<id>`" is implemented as: creating a handle
  costs nothing; the first dispatched call spawns the thread, which asks for
  the file grant, loads the component, runs the import check, instantiates
  and calls `activate(reason)` (the reason is derived from that call). The
  thread names are `plugin-<id>` (Linux shows the first 15 bytes) and the
  ticker thread is `plugin-epoch`, one per `Runtime`.
- `activate(restore)` is used after a trap *and* after an idle unload, as the
  WIT comment says; open panels are replayed with `open-view` after a trap.
- `ui.open-panel` for a contribution that already has an open panel returns
  that panel's id ("opens or focuses"). `PanelTree` validation is step 8;
  until then `render`/`apply-patches` only enforce the node and string caps.
- `kv` is validated and quota-checked but kept in memory per plugin (it
  survives restarts); writing `plugin-data/<id>/*.kv` is step 6.
- `editor.open-file` requires an open project and a relative `/` path
  without `.`/`..`; `open-scratch` caps the text at 64 MiB.
- `Store` caps measured (lowering each cap until instantiation failed): the
  Rust fixtures need 1 memory, 2 tables and 3 core instances. The §5
  starting values 2/8/16 are kept as headroom for C code (SQLite).
- Dependencies added to the host: `wasmtime`/`wasmtime-wasi` `=50.0.1`
  (§2 features), `sha2`, `bytes` (the WASI stream trait signature), `ropey`
  (`DocSnapshot`), `async-channel`, and `tokio` only for the `AsyncWrite`
  trait that `wasmtime_wasi::cli::StdoutStream` requires (tokio is already in
  the tree through wasmtime-wasi; no runtime is started by us).

**Budgets and limits (Step 4):**
- The epoch ticker runs on a fixed schedule (`next += 10 ms`) instead of
  `sleep(10 ms)` in a loop, so sleep overshoot does not accumulate; a spin
  call with a 300 ms budget traps after 310-330 ms of wall time (debug
  build). Because slices count guest execution, wall time can exceed
  budget + 30 ms only when the thread is descheduled; the `spin` test allows
  up to three attempts for that and always requires the budget trap itself.
- A trapped call is answered before its store is dropped (unmapping guest
  memory took several ms), and crash bookkeeping happens before the answer.
- Out-of-memory is reported as such: `Limiter` wraps `StoreLimits` and
  remembers a refused `memory.grow`, so the guest allocator's abort reads
  "out of memory (limit 16 MiB)" rather than "unreachable".
- Crash policy: the first trap restarts on the next event; the second waits
  1 s (doubling after that); the third within 10 minutes disables the plugin
  for the session. Calls during the backoff get `CallError::Backoff`. A trap
  in `activate` counts as a crash.
- A `provide-code-lenses` budget overrun is a *strike*, not a crash: the
  store is still dropped (any trap poisons it), and after three strikes lens
  calls return `LensesDisabled` while commands keep working.
- When the mailbox is full and holds no coalescable call, the new call is
  refused with `CallError::Busy` (the design only said which call to drop).
- Budgets, tick, idle timeout, hard-cancel grace, crash policy, store caps,
  mailbox size and the per-gesture request cap are all in `RuntimeConfig`
  (defaults are the §5 values); tests shorten them.
- The per-gesture request cap (64) counts requests made inside one gesture
  call.

**Grants, consent, WASI, import check (Step 5):**
- The design's "EACCES" for writes without a write grant is **EPERM** in
  practice: wasmtime-wasi answers `not-permitted` for a read-only preopen,
  which wasi-libc reports as errno 63. Rust maps both to
  `ErrorKind::PermissionDenied`; the test asserts that kind and errno 63, and
  that nothing was written.
- Grant scopes: `once` is never stored (for the file grant it covers one
  instance; the next instantiation asks again). **Deny** is remembered for
  the project; closing a prompt (`Dismissed`) denies this time only.
  Precedence is session > project > global > default; a global answer
  removes narrower ones so "Always" takes effect. With no project open,
  project answers live in memory for the session.
- Network grants are keyed by host (lower-case, no port); a manifest pattern
  with a port (`example.com:8080`) still limits which ports may be asked for.
- One workspace prompt covers read and the write checkbox; answering with
  write unticked stores write as denied in the same scope.
- `ConsentRequest` carries host-derived data only (method, scheme, host and
  port from the parsed URL; the manifest name quoted and capped at 80 chars
  by `consent::quote`). URLs with user info (`https://a@b/`) are refused.
- Grant files are TOML (`[[grant]] plugin/capability/host/state`), written
  atomically into 0700 directories; malformed entries are skipped (a broken
  file means "ask again"). The `.cwasm` cache dir is also 0700.
- The `wasi-http` fixture imports a stand-in `wasi:http/outgoing-handler`
  at version **0.3.0**: `wasm-component-ld` merges any `wasi:http@0.2.x`
  import with the real WASI WIT it embeds, so a 0.2 stand-in cannot be
  built. The host refuses `wasi:http` at every version.
- The import check also rejects unversioned interface names, other
  `arcode:*` packages, `arcode:plugin` 0.0.x, and reports 0.2+/1.x as
  "requires a newer Arcode".

## Appendix B (continued): Steps 6-8 and the facade

> **Added during implementation** of the imports, the network transport, the
> UI tree and `PluginManager` (`crates/arcode-plugin-host`). Same rule as
> above: the design text is unchanged; this lists what the code does
> differently or decides where the design was silent. The app-facing API is
> documented in `crates/arcode-plugin-host/README.md`.

**host, kv, workspace, editor (Step 6):**
- kv files use a small binary format (`ARCODE-KV 1` header, then
  length-prefixed key/value pairs). A scope is read on first use, never at
  startup. It is written atomically into an owner-only directory **once per
  call**, when the call returns, not on every `set`. A damaged file,
  invalid keys or a file over the quota read as empty, never as partial
  data. `InstanceSpec.kv` (`KvFiles`) says where the two scopes live; with
  no project open, the project scope lasts for the session only.
- `find-files` without an app index uses `index::WalkIndex` (the `ignore`
  crate). It honours `.gitignore`, `.ignore` and `.git/info/exclude` even
  outside a git repository, does **not** hide dotfiles (only `.git`), never
  follows symlinks, sorts by path and visits at most 500k entries per call.
- `apply-edits` is checked on the host before any prompt: the snapshot
  version must equal `expected-version` (otherwise `version-mismatch`), every
  range must lie inside the snapshot, ranges must not overlap (otherwise
  `invalid-range`), and at most 64 MiB of text may be inserted. Only then is
  editor-write consent asked and the bridge called. `memory_editor::MemoryEditor`
  is a complete reference `EditorBridge` that applies edits to the snapshots.
- `host.settings()` reads live values: `InstanceHandle::set_user_settings`
  (and `PluginManager::set_user_settings`) take effect on the next call
  without a restart.

**http (Step 7):**
- ureq `3.5` with `default-features = false, features = ["rustls"]` (ring
  provider, webpki roots; no gzip, no cookies). It adds **1.97 MB** to a
  stripped release binary on its own (docs/SPIKE.md), on top of wasmtime's
  14.81 MB. The §2 exit criterion covers wasmtime only and still holds, but
  the plugin feature as a whole is now about 16.8 MB.
- The host follows redirects itself (ureq's following is off) and calls
  `allow_redirect` before every hop. 303, and 301/302 after a POST, become a
  body-less GET; 307/308 keep the method and body. `Authorization`,
  `Cookie`, `Proxy-Authorization` and `Host` are dropped when the origin
  changes. At most 10 redirects.
- Each hop runs on a short-lived `plugin-http` helper thread. The plugin
  thread polls the result channel and the cancel token every 10 ms, so a
  cancel returns in about 8 ms even while the server stalls; the helper
  stops at the next body chunk or at the request timeout.
- `timeout-ms = 0` means 30 s; the maximum is 600 s. 4xx/5xx are ordinary
  responses. `reason` is the canonical reason phrase, because the `http`
  crate does not keep the server's own.
- `remote-addr` is always `none`: ureq 3 does not expose the peer address.
- Env proxies (`HTTPS_PROXY`, `NO_PROXY`, ...) are honoured by default
  (`TransportOptions::use_env_proxy`). Consent is still keyed on the URL's
  host. Tests turn proxies off.
- Network log: `net::NetworkLog`, 500 entries per plugin (time, method, host,
  port, and `Status(u16)`, `Refused(reason)` or `Failed(reason)`). It also
  logs requests the host refused (no gesture, denied, master switch).

**UI tree and rows (Step 8):**
- `remove(id)` removes that node only. After every accepted change, nodes
  unreachable from the root are dropped, which is exactly "every descendant
  no longer referenced". The SDK's `ui::apply` now does the same (it used to
  remove the whole subtree, which differed only when a patch set moved a
  removed node's child elsewhere). A node upserted without a parent is
  dropped.
- Validation also rejects a node with two parents (element ids must be
  unique per panel), `remove` or `rows-changed` of unknown nodes,
  `rows-changed` of a non-table, duplicate ids in `render`, and non-finite
  split ratios. Depth counts the root as level 1 (64 levels allowed).
- The app receives `PluginOutput::PanelView { snapshot: Arc<PanelSnapshot>,
  changes }` instead of raw views and patches. `Changes` lists dirty and
  removed nodes, the inputs whose `value` the plugin changed (the only ones
  that overwrite user text), and `TableChange`s (reset vs. resize).
  `rows-changed(id, none)` keeps the row count and still resets the cache.
- Every accepted change clones the node map (`Arc` per node) and
  re-validates the whole tree: about 20-30 ms for a one-node patch on a
  20,000-node panel in a debug build, and microseconds for typical panels.
  It runs on the plugin thread. Incremental validation is possible later if
  a real plugin needs it.
- `rows::RowCache` keeps **one** fetch per table outstanding, because the
  mailbox coalesces `fetch-rows` per table and a second request would
  supersede the first. When the viewport moves away from the outstanding
  page, a new request may be issued (latest range wins). Visible pages come
  first, then one page ahead, and nothing is fetched past a known end.
- `fetch-rows` answers are checked on the plugin thread: the page must start
  at the requested row (otherwise the call fails), extra rows are dropped,
  more than 4 MiB of cell text fails the call, and text cells are cut to
  4 KiB with an ellipsis.
- The SDK property test uses `arcode_plugin::testing::random` (a seeded
  xorshift generator of schema-valid trees). The host runs the same sequences
  through `PanelTree` (200 seeds × 20 steps) and checks the result equals
  the SDK's fresh flattening.

**Facade:**
- `PluginManager` (`manager/`) owns the registry, runtime, grant store,
  instance handles and output channel. Its calls return `Pending` at once and
  stamp the gesture flag: commands always carry it, `ui_event` follows
  `event_is_gesture`, and lenses, panels, openers and rows never do.
- Consent prompts become events: with `AppServices.consent = None` the
  manager uses `consent::ChannelConsent`, which sends
  `PluginOutput::Consent { request, reply }` and blocks the plugin thread on
  the reply, as quick picks do. A dropped reply means `Dismissed`.
- `PluginOutput::TaskStarted` carries the call's `CancelToken`, so the task
  chip's [Cancel] needs no lookup.
- Revoking a file grant calls `InstanceHandle::recycle()`. The next call
  first runs `deactivate`, drops the store, asks for the file grant again and
  re-activates with `restore`, before any guest code runs.
- `set_project` stops every instance (preopen, project grants and project
  kv change). `set_enabled(false)` stops one. Enabled state is per session:
  persisting `enabled` and `sha256` in `plugins.toml`, and `install.rs`,
  are still to do.

## Appendix B (continued): Step 10, the HTTP client plugin

> **Added during implementation** of `plugins/http-client`. Same rule as
> above: the design text is unchanged; this lists what the plugin does
> differently or decides where §9 was silent.

**Build and layout:**
- `plugins/` is its own cargo workspace (`plugins/Cargo.toml`, shared with
  the DB viewer); `scripts/build-plugins.sh http-client` writes
  `target/wasm-plugins/arcode.http-client/{plugin.toml,plugin.wasm,README.md}`.
  The release component is **387 KB** (§9 estimated ~300 KB).
- The crate is `crate-type = ["cdylib"]` and `export_plugin!` is behind
  `#[cfg(target_arch = "wasm32")]`: natively the component export names
  (`arcode:plugin/guest@0.1.0#activate`) are not valid symbols in the
  linker's version script, so a native cdylib with the export does not
  link. Native `cargo test` drives the `Plugin` impl directly against the
  SDK fakes. Every plugin that wants native tests needs the same `cfg`.
- Modules: `parse`, `vars`, `env`, `exchange` (request building, body
  classification), `history`, `view`, `commands`; all under 400 lines.

**Syntax decisions (§9 listed the features, not the rules):**
- Comments (`#`, `//`) and `# @directives` are recognised only before the
  body; the body is sent verbatim. Directives: `@name` (also `@name=x`;
  wins over the `### name` text), `@no-redirect`, `@timeout N` (seconds;
  `ms`/`s`/`m` suffixes).
- A bare URL means GET; standard methods are accepted in any case, any
  other all-upper-case word is a method (WebDAV). Indented lines right
  after the request line continue the URL. `/path` plus a `Host` header
  becomes `http://host/path`; a URL without a scheme gets `http://`.
  A header line before the first blank line that is not `Name: value`
  starts the body (lenient, no error).
- `< path` includes are relative to the `.http` file (absolute paths must
  be under `/project`), capped at 16 MiB and **not** variable-substituted
  (JetBrains' `<@` is not supported). Response handlers (`> {% %}`,
  `> x.js`) and `<> previous-response` lines are skipped.
- Variables: one map, the selected environment with the file's
  `@name = value` definitions on top (file wins, as in VS Code); values
  may reference other variables, cycles are an error, an unknown variable
  stops the request with a message in the panel. Dynamic variables:
  `$uuid`/`$random.uuid`/`$guid`, `$timestamp`, `$isoTimestamp`
  (`$datetime iso8601`), `$randomInt` (0..=1000), `$randomInt a b` and
  `$random.integer(a, b)` (`a..b`); `$processEnv`, `$dotenv` & co. are
  refused ("not available in the plugin sandbox"). Randomness is std's
  `RandomState`, whose keys come from WASI `random`, so no extra crate.
- Environment files: each of `http-client.env.json` and
  `http-client.private.env.json` is looked up from the `.http` file's
  directory up to `/project`, nearest wins (not only the project root as
  §9 says). A `$shared` section (VS Code) is inherited by every
  environment; private values override public ones; numbers, booleans and
  `null` become text, objects (JetBrains auth configs) are ignored.
  Without the workspace grant there is no preopen, so no environments.

**Behaviour decisions:**
- `send` always calls `host.begin-task("Sending <name or METHOD url>")`,
  so every request gets the task chip with [Cancel] and the 60 s budget.
  From the palette or the keybinding (no args), the request is the one
  around the first selection's start line.
- `max-response-bytes` is **16 MiB**, not the host's 50 MiB: the body, its
  text and the view must fit in the manifest's 64 MiB memory limit. The
  panel shows at most 3 MiB of body text (then a `code` excerpt with a
  hint instead of the JSON viewer) and 1 MiB of raw text, which keeps a
  render under the host's 8 MiB string cap; **Open in editor**
  (`open-scratch`, JSON pretty-printed) gets the whole body.
- JSON detection: `*/json` and `*/*+json` content types, plus sniffing
  (`{...}`/`[...]` that parses) when the type is missing, `text/plain` or
  `application/octet-stream`. Bodies that are not UTF-8 or contain NUL,
  and `image/audio/video/font` types, get a 4 KiB hex dump. Languages for
  `code`: xml, html, css, javascript, yaml, markdown, else none.
- The panel adds a **History** tab (the project's last 50 exchanges) and a
  **Send again** button. History entries store time, name, method, URL
  (capped at 2048 chars), status or error, total time and size: never
  headers or bodies, so secrets in headers are not written to kv.
- The status item reads "HTTP env: <name>" (or "none") from activation on
  and runs `select-environment`. Request failures (denied, timeout, DNS,
  ...) are shown in the panel with plain sentences and recorded in the
  history; the command itself still returns `ok`.
- Lenses: none for documents over 50,000 lines (100 ms lens budget); parses
  are cached per `(doc, version)` for up to 16 documents.

**Tests:** 46 native unit tests (parse with JetBrains-style samples, vars,
env, exchange, history, view, and the plugin flow against the SDK fakes)
and 4 end-to-end tests in `crates/arcode-plugin-host/tests/sandbox/http_client/`
(headless `PluginManager`, the plugin registered from `target/wasm-plugins`
as *bundled* because the `arcode.` prefix is reserved, `MemoryEditor`,
`ScriptedConsent`, real ureq against a loopback server): lenses, environment
selection, status/headers/body/timing/JSON detection, consent prompts,
history, network log, cancel (returns ~10 ms after `cancel()`), and a denied
host (no connection reaches the server, the denial is remembered).

## Appendix B (continued): Step 11, the DB viewer plugin

> **Added during implementation** of `plugins/db-viewer`
> (`arcode.db-viewer`). Same rule as above: the design text is unchanged;
> this lists what the code does differently or decides where the design was
> silent. Measurements are in `docs/SPIKE.md`, "Step 11 measurements".

**Manifest (differs from the §3 reference):**
- `workspace = "read-write"` instead of `"read"`. The viewer is still
  read-only by default: every file is opened `mode=ro`, and writes need
  **both** the "Allow writes" checkbox in the Query tab and a workspace
  grant with the write box ticked. With a read-only grant, SQLite's
  `mode=rw` open silently falls back to read-only; the plugin detects that
  (`sqlite3_db_readonly`) and says so. This relaxes §15 "DB writes in 0.1"
  on request; the lock problem remains (`nolock=1`), so turning writes on
  shows "The file is not locked: close other programs using it first."
- The `page-size` setting is dropped: the host owns page sizes (200-row
  `fetch-rows`, §7), so a plugin setting could not change them.
  `max-result-rows` (default 100,000, clamped to 1..=1,000,000) stays.

**Opening files (§10):**
- The WAL fallback (`immutable=1`) applies when a `-wal` file exists **or**
  the header says WAL (bytes 18-19 = 2) with no `-wal` file: with
  `nolock=1` SQLite cannot open a WAL database at all (no shared memory),
  it fails with `SQLITE_CANTOPEN`. Under `immutable=1` the pager treats the
  file like a temp file and ignores the WAL entirely, so the view is the
  last checkpoint ("snapshot; uncheckpointed WAL changes not shown"), and
  writes stay off for it.
- Every connection sets `temp_store = MEMORY` (the sandbox has no writable
  temp directory, so big sorts would fail with an I/O error) and
  `query_only = ON` unless writes are on. With writes off, any statement
  for which `sqlite3_stmt_readonly` is false is refused before it runs with
  SQLite's own message plus a hint. That also stops `VACUUM INTO`, which
  creates a file even on a read-only connection.
- `file:` URIs percent-encode `?`, `#` and `%` in paths, so a file name
  cannot add URI parameters.

**Counting and paging (§10 "Data tab"):**
- `count(*)` is answered by one `OP_Count` step that the progress handler
  cannot interrupt. Measured in wasm at about 65 µs per 4 KiB page
  (I/O-bound through WASI), so it is used only for databases of at most
  4096 pages (~270 ms worst case). Larger databases use `count(1)` under
  the 200 ms deadline. A 1M-row table (~22 MB) takes ~350 ms to count in
  wasm, so its count starts **unknown**; the Data tab then shows "rows:
  unknown" and a **Count** button that counts as a cancellable task
  (`begin-task("Counting rows")`).
- Paging is keyset on the rowid, generalised: every fetched page leaves
  *anchors* (row index → rowid); a page is reached from whichever is
  closest of the table start, an anchor below, an anchor above (walking
  backwards) or the end (when the count is known), skipping only the
  distance. When `max(rowid) - min(rowid) + 1 == count` (two separate
  min/max subqueries; a combined `SELECT min(x), max(x)` is a full scan)
  row *i* is rowid `min + i` and pages seek directly. Views, `WITHOUT
  ROWID` tables and sorted grids use `LIMIT/OFFSET`. The rowid alias is
  the first of `rowid`, `_rowid_`, `oid` that no column shadows.
- Interactive statements (open, select, expand, `fetch-rows`) run under a
  progress-handler deadline (500 ms schema, 200 ms count, 700 ms per page)
  so a slow page becomes an inline error, never a trap of the host's 1 s
  budget.
- Text cells are cut to 4 KiB in the plugin, and a page with more than
  3 MiB of text is cut further, so wide rows stay under the host's 4 MiB
  page cap.

**Query tab:**
- **No in-panel Cancel button.** Calls into one instance are serial (§5),
  so a Cancel click would be delivered only after the query it should
  stop. The status line says "Running query… (Esc or [Cancel] in the status
  bar stops it)"; those cancel the call's token, which the progress handler
  polls every 10k VM steps. *Proposal for API 0.2:* a button flag (e.g.
  `button-props.cancels-task`) whose click the host turns into
  `cancel()` of that panel's running task call instead of queueing it.
- All statements of the text run in order (rusqlite `Batch`); the last
  result set is shown, changed rows are summed ("2 rows changed"), and a
  run that wrote re-reads the schema.
- A "Recent" drop-down offers the last 20 queries (project-scope kv,
  `recent-queries`, 8 KiB per query); choosing one replaces the input text.

**Sidebar and commands:**
- `ui.open-panel` only opens *panel* contributions, so activating a
  database or table in the sidebar calls `editor.open-file(path)` (allowed
  with `editor = "none"`); the app routes the file to the `sqlite` opener
  and the plugin selects the remembered table in the new view.
- `new-query` shows the Query tab of the last-used database view; with
  none open it picks a database (quick pick when there are several) and
  opens it on the Query tab. `refresh` re-reads every open view and the
  sidebar's file list.

**Host change:** the `.cwasm` cache named its temp file `*.tmp<pid>`, so
two runtimes in one process compiling the same component raced on the
rename ("cannot write …: No such file or directory"). Temp names now carry
a per-process counter (`cache.rs`); `tests/db_viewer` has a regression test.

**Tests:** 28 unit + 22 native integration tests in `plugins/db-viewer`
(SDK fakes; a property test checks every paging strategy against plain
`LIMIT/OFFSET` on real SQLite; the 1M-row database is generated natively
from `tests/fixtures/big.sql`; an ignored test regenerates the
checked-in fixtures), and 7 end-to-end tests in
`crates/arcode-plugin-host/tests/db_viewer/` (the real wasm under a
headless `PluginManager`, registered as *bundled* from
`target/wasm-plugins`): tree, rows, query and inline errors on the checked-in
64 KB `sample.db`; the write toggle with read-only and read-write grants;
the WAL snapshot (`wal.db` + `wal.db-wal`); the sidebar; and a 1M-row
database that the plugin generates itself with writes on, then pages
(< 1 s per page) and cancels during a `GROUP BY`.

## Appendix B (continued): review fixes and Step 12

> **Added during implementation**, after the security and correctness
> review (`crates/arcode-plugin-host/tests/review_*.rs`,
> `plugins/*/…/review*.rs`). Same rule as above: the design text is
> unchanged; this lists what the code now does differently or decides
> where the design was silent. It supersedes the "still to do" notes on
> `plugins.toml`, `install.rs` and Step 12 in the sections above.
> Numbers are in `docs/SPIKE.md`, "Plugin engine, review fixes and Step 12".

**Budgets and cancellation (§5):**
- Host imports are charged to the CPU budget through a wasmtime call hook
  (the `call-hook` feature): each import costs its wall time minus the
  time it spent waiting for the user, the network or the UI thread
  (consent prompts, quick picks, `http.send`, `apply-edits`, `open-file`,
  `open-scratch`). §5 said "time spent in host imports does not count";
  only *waits* are free now, so looping over an expensive import
  (`find-files`, `apply-patches` on a big panel, WASI file I/O) can no
  longer burn host CPU for free.
- `wasi:clocks/monotonic-clock` `subscribe-*` are replaced (linker
  shadowing) by pollables that also wake when the current call is
  cancelled or its budget runs out. A guest sleeping in WASI is therefore
  stoppable, and the sleep counts against the budget.
- The hard-cancel grace runs from the later of the cancel and the start of
  the call: a call cancelled while still queued gets its full grace once it
  runs instead of being trapped at its first tick.

**Resource caps (§5 host caps):**
- The 8 MiB UI cap applies to the panel's whole tree after every change
  (which also bounds each `render`/`apply-patches`), and it counts the
  in-memory size of every list element (tab title, option, key-value pair,
  column, tree item) on top of their text.
- Row pages: 4 MiB counting 32 bytes per cell plus its text
  (`rows::CELL_BYTES`). The DB viewer cuts text to fit that budget; a table
  wider than ~650 columns cannot be paged at 200 rows (the Data tab shows
  the host's error). A per-table page size is the fix if a real database
  needs it.
- kv: each entry costs 64 bytes on top of key and value
  (`KV_ENTRY_BYTES`). Plugin log: each line costs its in-memory size on top
  of its text, so empty-line floods are bounded.
- `set-status` is coalesced: at most one `Status` output per 100 ms, the
  latest one when the call returns. Other outputs are not coalesced yet
  (`PanelView` floods are bounded by the per-change tree validation cost
  only; merging `Changes` is the next step if that shows up).
- Open files: the store's WASI resource table is capped at an eighth of
  `RLIMIT_NOFILE` (clamped to 256..=4096; 4096 on Windows), and
  `descriptor.open-at` is replaced so that a nearly full table answers
  `insufficient-memory` instead of trapping.

**Files and editor access (§3, §4):**
- `plugin.wasm` must be a regular file (a symlink to one is fine); it is
  opened non-blocking and read with a bound, so a FIFO or `/dev/zero`
  cannot hang or exhaust the editor.
- `.cwasm` cache: the directory must belong to the user; one found open to
  others is tightened to 0700 **and emptied**. Entries are owner-only files
  with a trailing seal (sha256 over the entry name, which holds the source
  hash and engine hash, and the artifact) and are deserialized from the
  checked bytes (`Component::deserialize`, not `deserialize_file`), so a
  swapped artifact is recompiled. This costs one sha256 of the artifact per
  activation (activation stays at 2-11 ms). `create_private_dir` tightens
  every existing private dir it is given (grants, kv).
- `editor = "context"`: lens languages no longer widen it, and a language
  the app has built in never counts as "a language the plugin contributes"
  (`RuntimeConfig::builtin_languages`, default: common language ids; the
  app should pass its syntax registry's ids).
- `editor.open-file` walks the path inside the project with
  `symlink_metadata` and refuses symlinks and non-regular files.

**HTTP (§4 prompts):**
- The request method must be an RFC 9110 token of at most 32 characters,
  checked before any prompt, so a prompt never shows plugin text as the
  method.
- `HttpTransport::send`'s `allow_redirect` now receives the hop's method
  and absolute URL (was `(host, port)`); the host parses scheme, host and
  port itself, so a redirect prompt shows the hop that would be sent
  (`GET https://…` after a 303 from `POST http://…`).
- `Location` resolution removes dot segments (RFC 3986 §5.2.4).

**UI tree and rows (§7):**
- A `TableChange` takes the row count from the final node, so
  `rows-changed` followed by an `upsert` of the same table agrees with the
  snapshot.
- `RowCache::set_row_count` forgets an end learned from an `at-end` page,
  drops rows past a new count and refetches the old partial last page.

**Reference plugins:**
- HTTP client: "Open in editor" re-indents JSON with a streaming
  re-indenter (validated through `IgnoredAny`, written into an exactly
  sized buffer) instead of `serde_json::Value`; bodies whose pretty form
  would exceed 24 MiB open as received. JetBrains `>> file` / `>>! file`
  response redirects and `< {% … %}` / `< script.js` pre-request scripts
  are skipped. Multipart bodies use CRLF line breaks (included files are
  sent byte for byte).
- DB viewer: views and `WITHOUT ROWID` tables sort with `ORDER BY` (ties
  keep SQLite's order).

**Step 12:**
- `install.rs`: validates a folder (manifest, non-reserved id, regular
  `plugin.toml` and entry ≤ 32 MiB; `README.md`/`LICENSE` copied only if
  regular and ≤ 1 MiB; nothing else is read), stages the copy next to
  `<data>/arcode/plugins/<id>/` and swaps it in by rename, then
  `precompile` fills the `.cwasm` cache. The functions block; the app runs
  them on a background task and calls `PluginManager::finish_install`.
- `plugin_state.rs`: `plugins.toml` (`[[plugin]] id, enabled, sha256`).
  Installed plugins start disabled; enabling approves the current
  `plugin.wasm` (sha256 stored); every activation checks it, and a
  mismatch refuses activation and sends `PluginOutput::NeedsApproval`
  (the app asks, then `set_enabled`). An update with different
  capabilities is disabled until enabled again; one with the same
  capabilities stays enabled with the new hash. Bundled plugins are
  enabled unless disabled (no hash: they ship with the app); dev plugins
  are session-only. The capability summary shown when enabling is app UI
  (step 9).
- `examples/plugin_bench.rs`, `crates/arcode-plugin-api/README.md` (author
  guide). Packaging: `ARCODE_WASM_PLUGINS=<prefix>/share/arcode/plugins
  scripts/build-plugins.sh`; wiring that into the release workflow belongs
  to the app's packaging.

## Appendix B (continued): Step 9, app integration (runtime wiring)

> **Added during implementation** of `crates/arcode/src/plugins/` and
> `crates/arcode/src/workspace/plugins*.rs` (behind the `plugins` cargo
> feature, on by default). Same rule as above: the design text is
> unchanged; this lists what the app does differently or decides where the
> design was silent. The consent dialog with capability summaries and the
> Plugins view (list, enable, grants, network log) are a separate step.

**Loading and cost:**
- Each workspace (window) owns one `PluginManager`, created **250 ms after
  the window opens** (or earlier when something needs it: opening a file,
  a plugin command). Creation reads `plugin.toml` files only; the engine
  starts on the first activation, exactly as in §2. Measured on a debug
  build, same tree built with and without the feature (`ready in`, and
  RSS 8 s after start with a project open and no plugin activated):
  `ready in` 110-191 ms with, 109-133 ms without (noise of the software
  renderer; the first frame does no plugin work); threads 23 both; RSS
  +7 MB, of which **+1.2 MB anonymous** (the manager: manifests, globs,
  registry) and +6.4 MB *clean, file-backed* pages of the larger binary
  (wasmtime, cranelift, ureq and rustls are linked in). Idle with the
  HTTP client active: its thread and the epoch ticker use no CPU. The
  HTTP transport (TLS config, root certificates) is built on the first
  request (`bridge::LazyHttp`). Measured inside the app (glibc
  `mallinfo2` and `VmRSS` around `PluginManager::new`, real bundled
  plugins): the manager itself holds **+11 KB of heap**; RSS grows by
  0.7-0.9 MB, all of it code pages of the binary it runs for the first
  time. Of the debug binary's ~152 MB RSS, ~110 MB are clean file-backed
  pages (`Private_Clean`) and ~37 MB anonymous, so the remaining idle
  difference is the size of the binary with wasmtime, cranelift and rustls
  linked in, not plugin state; removing it would take an out-of-process
  host. Release (LTO) numbers are not measured.
- Discovery: bundled roots are `<exe>/../share/arcode/plugins`, then (debug
  builds) this checkout's `target/wasm-plugins`; installed plugins in
  `<data>/plugins`; dev folders from `ARCODE_PLUGIN_DEV` (path list) and
  "Plugins: Load Unpacked…" (session only). `<config>` and `<data>` are
  Arcode's own folders (`ARCODE_CONFIG_DIR`/`ARCODE_DATA_DIR`), so a test
  run with those set leaves nothing in the user's folders; compiled
  components then go to `<data>/cache/plugins`.
- `runtime.builtin_languages` is set from `arcode_syntax::all_languages()`.

**Editor bridge:**
- Snapshots are published for every open editor after each edit (rope
  clone), and for the active editor before every command or lens call, so
  selections are current without work on each cursor move.
- A document's language for plugins is the editor's language, unless that
  is plain text and a plugin contributes a language for the file's
  extension (`.http` → `http`). Plain text's own extensions (`.txt`) and
  files that usually hold secrets or private notes (`.key`, `.pem`,
  `.env`, `.log`, ...; `snapshot::NEVER_CLAIMED`) are never claimed, and
  lens providers are never asked about plain-text documents, so
  `editor = "context"` cannot be widened to them without a prompt.
- Writes (`apply-edits`, `open-file`, `open-scratch`) are requests to the
  workspace task; the plugin thread waits at most 1 s (a timeout answers
  `denied`). `apply-edits` is one undo step; `open-scratch` opens an
  untitled, writable tab. A request the plugin stopped waiting for has no
  effect: `open-file` and `open-scratch` answer before they open anything
  and open nothing when the answer cannot be delivered, and an
  `apply-edits` whose answer cannot be delivered is undone.
- `workspace.find-files` uses the finder's file list once it is loaded,
  and walks the project (`WalkIndex`) before that.

**Contributions:**
- Palette: every command is a `plugins::RunPluginCommand { id }` action
  listed under its manifest title (titles without `Category:` are listed
  under "Plugins"); palette runs pass `null` args. `when = "language ==
  x"` hides the command unless the active document has that language
  (the palette reads a global, `commands::CommandContext`, because it is
  built while the workspace is being updated). A title never replaces a
  command it does not own: when another command (built in or another
  plugin's) already has the label, the plugin id is appended ("File: Save
  (acme.tools)"); a reload replaces a plugin's own entries and removes
  commands that are gone. A contributed `key` is bound in the `Editor`
  context (an invalid one is ignored) unless a binding already active in
  an editor of a pane uses it, a prefix of it, or a chord starting with it
  (`commands::key_taken`: Ctrl+S, Ctrl+C, Ctrl+K, another plugin's key);
  pressed outside the command's `when` it does nothing (the action
  propagates).
- Lenses: one editor decoration set `"plugins"`, above-line rows; lenses
  on one line share a row. Asked for when an editor of a lens language
  becomes visible and 300 ms after the last edit; answers for an older
  buffer version are dropped. A click runs the command as a gesture about
  that document. An edit drops the click targets at once (the rows move
  with their lines, but a target's args are for the old text); a lens
  clicked before the new answer is remembered by line and label, lenses
  are asked for at once, and the click runs if a lens with that label is
  still on that line.
- Openers: `Workspace::open_file` asks the registry first; a match opens
  the plugin view as a tab named after the file (opening it again focuses
  that tab). Only project files are offered to openers.
- Panels: `editor` panels (and `bottom` ones, until the bottom panel has
  plugin tabs) open as tabs of the active pane; `editor-beside` ones in the
  pane to the right (split off if needed) while the editor keeps focus;
  `sidebar` ones get an activity-bar entry after the built-in ones
  (`Activity::Extension`). Manifest icon names map onto the host icon set
  (`commands::icon_for`), unknown names show the plugin icon.
- Closing a tab drops the view; its release sends `view-closed` only if
  the instance still has that panel open, so closing never starts a
  stopped plugin.
- "Plugins: Reload" and a project switch stop every instance, and panel
  ids start at 1 again. The shown panels are retired first (a retired view
  sends nothing, not even `view-closed`, so it cannot reach a new panel
  with its id): Reload closes their tabs, a project switch has already
  reset the panes. Page keys carry a generation, so a new panel never
  lands on an old tab.

**Panel rendering (`plugins/panel/`):**
- Every widget kind is mapped. `text` without `wrap` is clipped at the
  panel edge, not ellipsized (GPUI measures ellipsized text at its
  min-content width inside flex columns). Horizontal stacks wrap. Plugin
  text is selectable only through right-click → copy (`text`, `badge`,
  `key-values`, table cells).
- `input`: single-line and multi-line `TextField`s; a multi-line input
  with a `language` is a highlighted `Editor`. Enter submits a
  single-line input with `submit-on-enter`, Ctrl/Cmd+Enter a multi-line
  one. An input counts as edited while its text differs from the
  plugin's last `value`.
- `table`: `uniform_list` over `rows::RowCache`; a row-number column; a
  sortable header click sends `sort` (toggling direction); double-click
  or Enter sends `row-activate`; arrows, Page Up/Down and Home/End move
  the selection (so a 100k-row table is crossed in one key press). A
  failed page shows an error line and is not refetched until the table
  changes. A reset (new generation) scrolls back to the top.
- `json` is pretty-printed on the background executor and shown in a
  read-only editor (no folding yet); invalid or oversized JSON is shown as
  received with a note. `code` is a read-only editor.
- After a crash the panel shows "<Plugin> stopped: <reason>", with
  [Restart] once the plugin is disabled for the session.

**Status, tasks, notices:**
- Status items and task chips live in the status bar after the problem
  counts; clicking a chip or pressing Esc anywhere in the window cancels
  the newest running task (Esc is consumed only then). Esc is taken by a
  keystroke interceptor, installed when the first task starts, so it acts
  before any key binding (the editor's Escape would otherwise clear a
  selection first); while a consent prompt or quick pick is open Esc is
  theirs. Tasks also have "Plugins: Cancel Task".
- `notify`, crashes, disables and "needs approval" become toasts and lines
  of "Plugins: Show Log", which also shows every plugin's log (reopen to
  refresh).
- Consent (interim): a host prompt (the themed in-window dialog) built
  from the host-derived request, with Allow Once / This Project / Always /
  Deny for hosts and a read / read-and-write choice for workspace access.
  `ARCODE_PLUGIN_CONSENT` (`allow-once`, `allow-project`, `allow-always`,
  optional `+write`, or `deny`) answers every prompt without showing it,
  for automated runs; answers are written to the plugin log. Only debug
  builds read it; a release build always asks.
- Quick picks use the status-bar list picker overlay.
