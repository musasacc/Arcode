# API reference

Everything your plugin can call, and everything Arcode calls in your
plugin, for plugin API 0.1. Names are the Rust names of the
`arcode_plugin` crate (the `arcode-plugin-api` package). The WIT names in
[`arcode-plugin.wit`](../../crates/arcode-plugin-api/wit/arcode-plugin.wit)
are the same with `-` instead of `_` (`begin-task` is `begin_task`).
The exports have shorter names on the `Plugin` trait: `handle-command` is
`command`, `provide-code-lenses` is `code_lenses` and `handle-ui-event` is
`ui_event`.

Most plugins start with:

```rust
use arcode_plugin::prelude::*;
```

The prelude brings in the `Plugin` trait, `Json`, `Result`, `Error`,
`Activation`, `CodeLens`, `TablePager`, `editor::{self, DocInfo}`, the
`host`, `http`, `kv` and `workspace` modules, and from `ui`: `Panel`,
`PanelId`, `NodeId`, `Event`, `EventKind`, `RowPage`. Widget builders
come from `arcode_plugin::ui` (see [Building UI](ui.md)).

- [The `Plugin` trait](#the-plugin-trait): what Arcode calls
- [Commands](#commands), [code lenses](#code-lenses),
  [panels and openers](#panels-and-openers)
- [`host`](#host-notifications-status-quick-pick-settings-tasks): log,
  notifications, status item, quick pick, settings, tasks, cancellation
- [`editor`](#editor-documents): documents, selections, edits
- [`workspace` and files](#workspace-and-project-files)
- [`kv`](#kv-storage): key-value storage
- [`http`](#http): network requests
- [Errors](#errors), [`Json`](#json-arguments)

## The `Plugin` trait

```rust
pub trait Plugin: Default + 'static {
    fn activate(&mut self, reason: Activation) -> Result<()>;
    fn deactivate(&mut self);
    fn command(&mut self, id: &str, args: Json) -> Result<()>;
    fn code_lenses(&mut self, doc: &DocInfo) -> Vec<CodeLens>;
    fn open_view(&mut self, panel: PanelId, contribution: &str, args: Json) -> Result<()>;
    fn ui_event(&mut self, panel: PanelId, event: Event) -> Result<()>;
    fn fetch_rows(&mut self, panel: PanelId, table: NodeId, start: u64, count: u32) -> Result<RowPage>;
    fn view_closed(&mut self, panel: PanelId);
}
```

Every method has a default, so implement only what your manifest
contributes. Register the type once:

```rust
#[cfg(target_arch = "wasm32")]
arcode_plugin::export_plugin!(MyPlugin);
```

The `cfg` keeps native builds (your tests) linkable.

| Method | Called when | Default | Budget | User action? |
|---|---|---|---|---|
| `activate(reason)` | The instance was created, before the call that caused it. | `Ok(())` | 1 s | no |
| `deactivate()` | Before the instance is dropped (idle unload, disable, reload, project switch). | nothing | 100 ms | no |
| `command(id, args)` | One of your commands ran. | error "unknown command" | 2 s | yes |
| `code_lenses(doc)` | A document of a lens language is visible, and again 300 ms after edits. | no lenses | 100 ms | no |
| `open_view(panel, contribution, args)` | Arcode shows one of your panels or openers. | error "no view for" | 1 s | no |
| `ui_event(panel, event)` | A widget in one of your panels sent an event. | `Ok(())` | 1 s | depends on the event |
| `fetch_rows(panel, table, start, count)` | A table needs rows. | empty page at the end | 1 s | no |
| `view_closed(panel)` | The user closed one of your panels. | nothing | 1 s | no |

"User action" is what Arcode calls a *gesture*: only these calls may send
HTTP requests or start a task. See [Permissions](permissions.md#user-actions).

Errors returned from `command`, `open_view` and `ui_event` are shown to
the user as a notification ("<Plugin name>: <message>"), so write them as
plain sentences.

### `Activation`

```rust
pub enum Activation {
    Command(String),   // a command id
    Language(String),  // a lens language id
    Opener(String),    // an opener id
    Panel(String),     // a panel id
    Restore,           // after an idle unload or a crash
}
```

You rarely need the reason. Use `activate` for cheap setup such as
setting the status item from stored data. Returning an error prevents the
plugin from starting and shows the message.

### State

Your plugin value lives from `activate` until `deactivate`. Calls are
serial, so `&mut self` is all the synchronization you need. When the
plugin crashes, its state is gone; when it comes back, `activate(Restore)`
runs on a fresh `Default` value, then `open_view` runs again for every
panel that is still open. Keep anything that must survive in `kv`.

## Commands

```toml
[[contributes.commands]]
id = "insert-date"
title = "Dates: Insert Today"
```

```rust
fn command(&mut self, id: &str, args: Json) -> Result<()> {
    match id {
        "insert-date" => self.insert_date(),
        "open-item" => self.open_item(args.parse()?),
        other => Err(Error::msg(format!("unknown command `{other}`"))),
    }
}
```

Commands run from the palette, from their key binding, from a code lens
and from the status item. From the palette and keys `args` is `null`;
from a lens it is whatever the lens carries. The document the command is
"about" is the lens's document, or the active document otherwise, and is
what `editor::current_document()` returns.

## Code lenses

A lens is a small clickable label drawn above a line. Clicking it runs
one of your commands with arguments you choose.

```toml
[[contributes.commands]]
id = "run-test"
title = "Tests: Run Test at Cursor"

[[contributes.code-lenses]]
languages = ["python"]
```

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct RunArgs {
    line: u32,
    name: String,
}

fn code_lenses(&mut self, doc: &DocInfo) -> Vec<CodeLens> {
    let Ok(text) = editor::text(doc.id, None) else {
        return Vec::new();
    };
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let name = line.strip_prefix("def test_")?.split('(').next()?;
            let args = RunArgs { line: i as u32, name: format!("test_{name}") };
            Some(CodeLens::new(i as u32, "Run test", "run-test", &args))
        })
        .collect()
}
```

`CodeLens::new(line, title, command, &args)`:

| Field | Type | Meaning |
|---|---|---|
| `line` | `u32` | Zero-based line the lens sits above. |
| `title` | `String` | The label. Lenses on the same line share a row. |
| `command` | `String` | A command id from your manifest. Lenses naming an unknown command are dropped. |
| `args` | `String` | JSON text; `CodeLens::new` serializes any `serde::Serialize` value. |

Rules:

- The budget is 100 ms. Cache parsed results per `(doc.id, doc.version)`
  and skip very large documents (`doc.line_count`).
- Answers for an older version of the document are dropped.
- With `editor = "context"`, `code_lenses` may read the document it is
  asked about.

## Panels and openers

See [Building UI](ui.md) for the widgets. The lifecycle:

```rust
fn open_view(&mut self, panel: PanelId, contribution: &str, args: Json) -> Result<()> {
    match contribution {
        "results" => {
            // Shown from the activity bar, or restored after a restart.
            self.panel = Some(Panel::attach(panel));
            self.redraw()
        }
        "csv" => {
            // An opener: args is {"path": "data/table.csv"}.
            #[derive(serde::Deserialize)]
            struct Open { path: String }
            let open: Open = args.parse()?;
            self.show_file(panel, &open.path)
        }
        other => Err(Error::msg(format!("no view for `{other}`"))),
    }
}

fn view_closed(&mut self, panel: PanelId) {
    if self.panel.as_ref().map(Panel::id) == Some(panel) {
        self.panel = None;
    }
}
```

To open a panel yourself, for example from a command:

```rust
let mut panel = Panel::open("results", "Results", None)?;   // None: manifest location
panel.rerender(&ui::text("Working…"))?;
```

`Panel::open(contribution, title, location)` opens the panel, or focuses
it when it is already open (one panel per contribution), and returns
immediately. If it was already open, it returns the same id; keep your
`Panel` value in that case so its diff state stays valid.

`Panel` methods:

| Method | What it does |
|---|---|
| `Panel::open(contribution, title, location)` | Opens or focuses a panel of a manifest contribution. `location: Option<ui::Location>`. |
| `Panel::attach(id)` | Wraps a panel Arcode created (the `panel` of `open_view`). |
| `id()` | The panel id. |
| `rerender(&root)` | Sends the tree: a full render the first time, then only the changes. |
| `node_id(key)` | The node id of the element with explicit key `key` in the last tree sent. |
| `rows_changed(table, row_count)` | Drops a table's cached rows, optionally with a new row count. |
| `invalidate()` | Makes the next `rerender` a full render. |
| `set_title(title)` | Changes the tab or panel title (at most 256 bytes are kept). |
| `close(self)` | Closes the panel. |

## `host`: notifications, status, quick pick, settings, tasks

```rust
use arcode_plugin::host::{self, Level};
```

| Function | Signature | Notes |
|---|---|---|
| `api_version` | `fn() -> String` | `"0.1.0"`. |
| `log` | `fn(level: Level, message: &str)` | A line in your plugin's log. Lines over 16 KiB are cut. |
| `notify` | `fn(level: Level, message: &str)` | A notification. Plain text, at most 1 KiB, 5 per minute (more are dropped and logged). |
| `set_status` | `fn(text: Option<&str>, command: Option<&str>)` | Your plugin's one status-bar item. Clicking it runs `command` (a command id from your manifest) as a user action. `None` text removes it. |
| `quick_pick` | `fn(title: &str, items: &[String]) -> Option<u32>` | Shows a list for the user to pick from and waits. `None` when the user pressed Escape. At most 10,000 items of 512 bytes. |
| `settings` | `fn() -> String` | Your settings as a JSON object (manifest defaults merged with the user's values). |
| `settings_as` | `fn<T: DeserializeOwned>() -> Result<T>` | The same, deserialized. |
| `cancelled` | `fn() -> bool` | Whether the user cancelled the current call. Cheap; poll it in loops. |
| `begin_task` | `fn(title: &str) -> bool` | Turns the current call into a task: "title… [Cancel]" in the status bar and the CPU budget rises to `task-seconds`. Returns `false` (and does nothing) outside a user action. |

`Level` is `Trace`, `Debug`, `Info`, `Warn` or `Error`.

```rust
fn select_profile(&mut self) -> Result<()> {
    let names = vec!["dev".to_owned(), "staging".to_owned()];
    if let Some(i) = host::quick_pick("Select a profile", &names) {
        let name = &names[i as usize];
        kv::set(kv::Scope::Project, "profile", name.as_bytes())?;
        host::set_status(Some(&format!("Profile: {name}")), Some("select-profile"));
    }
    Ok(())
}
```

Waiting in `quick_pick` does not count against your CPU budget.

A long task:

```rust
fn scan(&mut self) -> Result<()> {
    host::begin_task("Scanning for TODOs");
    for path in workspace::find_files("**/*.rs", 10_000)? {
        if host::cancelled() {
            host::notify(Level::Info, "Scan cancelled.");
            return Ok(());
        }
        let text = std::fs::read_to_string(format!("/project/{path}")).unwrap_or_default();
        self.collect(&path, &text);
    }
    self.redraw()
}
```

The user cancels with the status bar's Cancel button, with Escape, or by
closing the panel the call is about. After a cancel, blocking host calls
return early (HTTP returns `HttpError::Cancelled`, `quick_pick` returns
`None`) and your call has 2 seconds to return before Arcode stops it.

Status updates are coalesced: at most one per 100 ms is shown, and the
latest one when the call returns.

## `editor`: documents

```rust
use arcode_plugin::editor::{self, DocInfo, Position, Range, TextEdit};
```

Types:

```rust
pub type DocId = u64;
pub struct Position { pub line: u32, pub column: u32 }   // zero-based; column counts chars
pub struct Range { pub start: Position, pub end: Position }
pub struct DocInfo {
    pub id: DocId,
    pub path: Option<String>,     // project-relative, `/`-separated; None for untitled buffers
    pub language: String,
    pub version: u64,             // grows with every edit
    pub line_count: u32,
}
pub struct TextEdit { pub range: Range, pub text: String }
```

| Function | Signature | Notes |
|---|---|---|
| `current_document` | `fn() -> Option<DocInfo>` | The document this call is about (lens or command), else the focused one, if the plugin may read it. |
| `document_info` | `fn(doc: DocId) -> Result<DocInfo, EditorError>` | |
| `text` | `fn(doc: DocId, range: Option<Range>) -> Result<String, EditorError>` | The whole text, or a range. Reads a snapshot; never waits for the UI. |
| `selections` | `fn(doc: DocId) -> Result<Vec<Range>, EditorError>` | One range per cursor; an empty range is a plain cursor. |
| `apply_edits` | `fn(doc: DocId, expected_version: u64, edits: &[TextEdit]) -> Result<u64, EditorError>` | Applies all edits as one undo step and returns the new version. Needs `editor-write`. |
| `open_file` | `fn(path: &str, at: Option<Position>) -> Result<(), EditorError>` | Opens a project file (relative path with `/`, no `.` or `..`, a regular file reached without symbolic links). |
| `open_scratch` | `fn(title: &str, language: &str, text: &str) -> Result<DocId, EditorError>` | Opens an untitled, unsaved buffer. Text up to 64 MiB. |

Which documents you can read depends on `[capabilities] editor`; see
[Permissions](permissions.md#documents). `open_file` and `open_scratch`
need no editor capability; `open_file` needs an open project.

Editing:

```rust
fn insert_date(&mut self) -> Result<()> {
    let Some(doc) = editor::current_document() else { return Ok(()) };
    let at = editor::selections(doc.id)
        .ok()
        .and_then(|s| s.first().map(|r| r.start))
        .unwrap_or(Position { line: 0, column: 0 });
    let edit = TextEdit { range: Range { start: at, end: at }, text: today() };
    match editor::apply_edits(doc.id, doc.version, &[edit]) {
        Ok(_new_version) => Ok(()),
        Err(editor::EditorError::VersionMismatch(_)) => {
            Err(Error::msg("The document changed; try again."))
        }
        Err(e) => Err(Error::msg(format!("cannot edit: {e:?}"))),
    }
}
```

`apply_edits` rules: `expected_version` must equal the document's current
version (otherwise `VersionMismatch(current)`); every range must lie
inside the document and ranges must not overlap (otherwise
`InvalidRange`); ranges refer to the text before any of the edits; at
most 64 MiB of new text. The user is asked for permission the first time
in each project. The call waits up to 1 second for the editor.

`EditorError` is `Denied`, `NoSuchDocument`, `VersionMismatch(u64)`,
`InvalidRange` or `InvalidPath`.

## `workspace` and project files

There is no file API of its own: you read and write project files with
`std::fs`, under the path `/project` (also available as
`workspace::PROJECT_DIR`). That works once the user allowed project access
for your plugin (`[capabilities] workspace`); without it, `/project` does
not exist.

```rust
use std::path::Path;

let config = std::fs::read_to_string(Path::new(workspace::PROJECT_DIR).join("app.toml"))?;
```

| Function | Signature | Notes |
|---|---|---|
| `root_name` | `fn() -> Option<String>` | The project's display name (never its absolute path). `None` without a project. |
| `find_files` | `fn(glob: &str, limit: u32) -> Result<Vec<String>, FsError>` | Project-relative paths with `/` that match `glob`, sorted, honouring `.gitignore`. `limit` is capped at 10,000. |

`find_files` details:

- The glob uses the usual syntax (`*`, `**`, `?`, `[abc]`, `{a,b}`); `*`
  does not cross `/`, so use `**/*.rs` for every Rust file.
- Files ignored by `.gitignore`, `.ignore` and `.git/info/exclude` are
  left out. Dotfiles are included; the `.git` folder is not. Symbolic
  links are not followed.
- It needs the workspace capability and an open project, otherwise it
  returns `FsError::Denied`. A bad glob returns `FsError::InvalidGlob(msg)`.

File access rules: paths that leave `/project` (`..`, absolute paths,
symbolic links pointing outside) fail. Writing needs the user's write
grant; without it writes fail with a permission error. The number of open
files per plugin is limited (an eighth of the editor's limit, 256 to
4096), after which opening a file fails with an error.

`FsError` is `Denied`, `InvalidGlob(String)` or `Io(String)`.

## `kv`: storage

A small persistent key-value store per plugin, in two scopes:

- `kv::Scope::Global`: shared by every project;
- `kv::Scope::Project`: one store per project (session-only when no
  project is open).

| Function | Signature | Notes |
|---|---|---|
| `get` | `fn(scope: Scope, key: &str) -> Option<Vec<u8>>` | |
| `set` | `fn(scope: Scope, key: &str, value: &[u8]) -> Result<(), KvError>` | |
| `delete` | `fn(scope: Scope, key: &str)` | |
| `keys` | `fn(scope: Scope, prefix: &str) -> Vec<String>` | Keys starting with `prefix`, sorted. |

- Keys are 1 to 128 bytes of printable ASCII (space to `~`); others give
  `KvError::InvalidKey`.
- Each scope holds at most 1 MiB per plugin, counting keys, values and 64
  bytes per entry; past that `set` returns `KvError::QuotaExceeded`.
- Values are bytes. Store text with `as_bytes()`, structured data with
  `serde_json::to_vec`.
- Changes are written to disk when the call returns, in one atomic write.
- Data stays when the plugin is disabled or uninstalled.

```rust
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct History { recent: Vec<String> }

fn load() -> History {
    kv::get(kv::Scope::Project, "history")
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(history: &History) {
    let bytes = serde_json::to_vec(history).unwrap_or_default();
    if let Err(e) = kv::set(kv::Scope::Project, "history", &bytes) {
        host::log(host::Level::Warn, &format!("history not saved: {e:?}"));
    }
}
```

## `http`

```rust
use arcode_plugin::http::{self, HttpError, Request, Response};
```

```rust
pub struct Request {
    pub method: String,                    // "GET", "POST", ...
    pub url: String,                       // http:// or https://
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timeout_ms: u32,                   // 0 means 30 s; at most 600 s
    pub follow_redirects: bool,
    pub max_response_bytes: u64,           // 0 or above 50 MiB means 50 MiB
}

pub struct Response {
    pub status: u16,
    pub reason: String,                    // canonical reason phrase
    pub version: String,                   // "HTTP/1.1", ...
    pub final_url: String,                 // after redirects
    pub remote_addr: Option<String>,       // always None in 0.1
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub truncated: bool,                   // body hit max_response_bytes
    pub timing: Timing,                    // headers_ms, total_ms
}
```

`http::send(&request) -> Result<Response, HttpError>` blocks your plugin's
thread (never the editor) until the response arrives, the timeout passes
or the user cancels. Waiting does not count against your CPU budget.

```rust
fn fetch_status(&mut self) -> Result<()> {
    host::begin_task("Checking status");
    let request = Request {
        method: "GET".into(),
        url: "https://status.example.com/api/v1/status".into(),
        headers: vec![("Accept".into(), "application/json".into())],
        body: Vec::new(),
        timeout_ms: 10_000,
        follow_redirects: true,
        max_response_bytes: 1024 * 1024,
    };
    match http::send(&request) {
        Ok(response) => self.show(response.status, &response.body),
        Err(HttpError::Denied) => self.show_message("You did not allow this host."),
        Err(HttpError::Cancelled) => Ok(()),
        Err(e) => self.show_message(&format!("Request failed: {e:?}")),
    }
}
```

What Arcode checks, in order, before anything leaves the machine:

1. Plugin networking is on (`[plugins] network` in the user's config).
2. The call is a user action (otherwise `NoUserGesture`), and this action
   has made fewer than 64 requests.
3. The method is a valid HTTP token of at most 32 characters, the URL is
   `http` or `https` and has no user name or password in it (otherwise
   `InvalidRequest`).
4. The host (and port) matches `[capabilities] network.hosts`.
5. The user allowed this host: the first request to each host shows a
   prompt (Allow once, Always for this project, Always, Deny).

Redirects are followed by Arcode itself (at most 10), and steps 4 and 5
run again for every hop; `Authorization`, `Cookie` and
`Proxy-Authorization` headers are dropped when a redirect changes the
origin. 4xx and 5xx statuses are ordinary responses, not errors. The
standard proxy environment variables (`HTTPS_PROXY`, `NO_PROXY`, ...) are
honoured.

`HttpError` is `Denied`, `NoUserGesture`, `Cancelled`, `Timeout`,
`InvalidRequest(String)`, `Dns(String)`, `Connect(String)`, `Tls(String)`,
`Protocol(String)` or `Io(String)`.

Every request, refused ones included, appears in the plugin's network log
(time, method, host, port, status or reason). Bodies, paths and headers
are never logged.

## Errors

`arcode_plugin::Error` is a plain message. Any type that implements
`Display` converts into it with `?`, like `anyhow::Error`. That includes
the host error enums (`EditorError`, `HttpError`, `KvError`, `FsError`,
`UiError`), whose `Display` text is their variant name (`Denied`,
`VersionMismatch(7)`, ...):

```rust
let n: u32 = text.trim().parse()?;                       // ParseIntError -> Error
let text = editor::text(doc.id, None)?;                  // EditorError -> Error ("Denied")
let text = editor::text(doc.id, None)
    .map_err(|e| Error::msg(format!("cannot read the document: {e}")))?;   // a friendlier message
```

`Error::msg(text)` creates one, `error.message()` reads it. Because
errors from `command`, `open_view` and `ui_event` are shown to the user,
add context with `map_err` where a bare variant name would confuse.

`arcode_plugin::Result<T>` is `std::result::Result<T, arcode_plugin::Error>`.

## `Json` arguments

Commands, lenses, openers and settings carry JSON text. `Json` wraps it:

| Method | What it does |
|---|---|
| `Json::null()` | The `null` literal. |
| `Json::from_value(&value)` | Serializes any `serde::Serialize` value. |
| `json.parse::<T>()` | Deserializes; empty text counts as `null`. Fails with "invalid arguments: ...". |
| `json.as_str()`, `json.into_string()` | The raw text. |

Parse into `Option<T>` when a command can run both with arguments (from a
lens) and without (from the palette).

## Not in API 0.1

There is no API for processes, environment variables, raw sockets, the
clipboard, files outside the project, timers, background work, images,
markdown rendering or secrets storage. The background-network capability
exists in the manifest but cannot be granted yet.
