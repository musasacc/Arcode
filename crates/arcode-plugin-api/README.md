# Writing an Arcode plugin (API 0.1)

An Arcode plugin is a sandboxed WebAssembly component (`wasm32-wasip2`)
plus a `plugin.toml`. It describes its UI as a tree the editor draws with
native widgets; it never touches the editor's internals. This crate
(`arcode-plugin-api`, imported as `arcode_plugin`) is the SDK. The design,
with every rule the host enforces, is `docs/PLUGINS.md`; the two bundled
plugins in `plugins/http-client` and `plugins/db-viewer` are complete
examples.

## Quick start

```sh
rustup target add wasm32-wasip2           # once
cargo new --lib acme-todo && cd acme-todo
# Cargo.toml and src/lib.rs as in section 3, plugin.toml as in section 2
cargo test                                # plugin logic, natively (section 7)
cargo build --release --target wasm32-wasip2
mkdir -p dist/acme.todo
cp plugin.toml dist/acme.todo/
cp target/wasm32-wasip2/release/acme_todo.wasm dist/acme.todo/plugin.wasm
```

Then in Arcode: "Plugins: Load Unpacked…" on `dist/acme.todo` to try it
(session only), or Plugins → Installed → the folder button ("Plugins:
Install from Folder…") to install it (section 8). Plugins with C
dependencies (SQLite in the DB viewer) also need clang and the wasi-libc
headers; `scripts/build-plugins.sh` shows the flags.

## 1. The folder

```
my-plugin/
  plugin.toml     manifest (required)
  plugin.wasm     the component (required, at most 32 MiB)
  README.md       optional, copied with the plugin
  LICENSE         optional
```

Nothing else is copied when the user installs the folder ("Plugins:
Install from Folder…"). There are no icons or images: name icons from the
host set (`globe`, `database`, `terminal`, …).

## 2. The manifest

```toml
[plugin]
id = "acme.todo"            # ^[a-z0-9][a-z0-9.-]{2,63}$; `arcode.` is reserved
name = "TODO Finder"
version = "0.1.0"
api = "0.1"
entry = "plugin.wasm"
description = "Lists TODO comments"

[capabilities]               # ask only for what you use
workspace = "read"           # none | read | read-write  -> /project in WASI
editor = "context"           # none | context | all-open
editor-write = false
# network = { hosts = ["api.example.com"] }   # per-host consent, gesture only

[limits]
memory-mb = 64               # default 64, the user caps it (256 by default)
task-seconds = 60            # CPU budget after host::begin_task

[[contributes.commands]]
id = "scan"
title = "TODO: Scan Project"

[[contributes.panels]]
id = "list"
title = "TODOs"
location = "bottom"          # sidebar | bottom | editor | editor-beside
icon = "list"
```

Unknown keys are errors. Contribution ids are local (`scan`); the host
namespaces them as `acme.todo/scan`. Other contributions:
`[[contributes.languages]]`, `[[contributes.code-lenses]]` (activate when a
document of that language is visible), `[[contributes.openers]]` (file
patterns your view opens) and `[[contributes.settings]]` (`type = boolean |
integer | string | enum`, with a `default`).

There is no startup event: the plugin activates when one of its commands
runs, its panel or opener is shown, or a lens language becomes visible. An
idle plugin is unloaded after 10 minutes without a call and an open panel.

## 3. The code

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
arcode-plugin = { package = "arcode-plugin-api", path = "…/crates/arcode-plugin-api" }
serde = { version = "1", features = ["derive"] }
```

```rust
use arcode_plugin::prelude::*;
use arcode_plugin::ui::*;

#[derive(Default)]
struct Todo {
    panel: Option<Panel>,
    found: Vec<String>,
}

impl Plugin for Todo {
    fn command(&mut self, id: &str, _args: Json) -> Result<()> {
        match id {
            "scan" => self.scan(),
            other => Err(Error::msg(format!("unknown command {other}"))),
        }
    }
}

impl Todo {
    fn scan(&mut self) -> Result<()> {
        host::begin_task("Scanning for TODOs"); // status-bar chip with [Cancel]
        self.found.clear();
        for path in workspace::find_files("**/*.rs", 10_000)? {
            if host::cancelled() {
                break;
            }
            let text = std::fs::read_to_string(format!("/project/{path}")).unwrap_or_default();
            self.found.extend(text.lines().filter(|l| l.contains("TODO")).map(str::to_owned));
        }
        let mut panel = match self.panel.take() {
            Some(p) => p,
            None => Panel::open("list", "TODOs", None)?,
        };
        panel.rerender(&vstack(self.found.iter().map(|t| text(t.clone()).mono())))?;
        self.panel = Some(panel);
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
arcode_plugin::export_plugin!(Todo);
```

Build with `cargo build --release --target wasm32-wasip2` and copy
`target/wasm32-wasip2/release/<crate>.wasm` to `plugin.wasm`.

The `#[cfg(target_arch = "wasm32")]` matters: natively the component export
names are not valid linker symbols, and native builds are what your tests
use (section 6).

### The `Plugin` trait

Every method has a default, so implement only what you contribute:
`activate(reason)`, `deactivate()`, `command(id, args)`,
`code_lenses(doc)`, `open_view(panel, contribution, args)`,
`ui_event(panel, event)`, `fetch_rows(panel, table, start, count)` and
`view_closed(panel)`. Calls into one plugin are serial; your type keeps its
state between calls and is dropped on `deactivate`. Arguments arrive as
[`Json`]; `args.parse::<T>()?` deserializes them with serde.

## 4. Imports and what the host checks

| Module | What | Checked by the host |
|---|---|---|
| `host` | `log`, `notify` (5/min), `set_status` (coalesced), `quick_pick`, `settings`, `cancelled`, `begin_task` | `begin_task` only in a user-gesture call |
| `kv` | small key/value store, `Global` or `Project` scope | 1 MiB per scope: keys + values + 64 bytes per entry |
| `workspace` | `root_name`, `find_files(glob, limit)` (honours `.gitignore`) | file contents go through `std::fs` on `/project` |
| `editor` | `current_document`, `text`, `selections`, `apply_edits`, `open_file`, `open_scratch` | `context`: the origin document, or documents of a language you contribute that the editor does not have built in; writes ask once per project; `open_file` opens regular files inside the project only |
| `http` | `send(request)` | only in a gesture call, only to `network.hosts`, per-host consent, re-asked on every redirect, at most 64 requests per gesture, 50 MiB per body |
| `ui` | `Panel`, builders, `TablePager` | see section 5 |

Files: the project is preopened at `/project` (read-only unless the user
also granted write). There is no environment, no arguments, no stdin, no
sockets; stdout and stderr go to the plugin log (1 MiB, newest kept).

## 5. Budgets and limits

- **CPU.** `command` 2 s, `open_view`/`ui_event`/`fetch_rows` 1 s, lenses
  100 ms, `activate` 1 s; after `begin_task` up to `limits.task-seconds`.
  Guest execution counts, and so does time spent inside host functions
  except waiting for the user, the network or the editor (prompts, quick
  picks, `http.send`, editor round trips). A WASI sleep counts and ends
  early when the call is cancelled. Overrunning traps: the instance is
  dropped and restarted on the next event; three traps in ten minutes
  disable the plugin for the session.
- **Cancel.** Poll `host::cancelled()` in loops. After cancel you get 2 s
  (counted from the start of your call if it was cancelled while queued)
  before the host stops you.
- **Memory.** `limits.memory-mb` (default 64). Allocation failures happen
  inside your sandbox; plan for them with large inputs.
- **UI.** 20,000 nodes and depth 64 per panel, 8 MiB per panel of text plus
  list elements (each tree item, option, column or key-value pair costs its
  in-memory size too). Table pages: 200 rows, at most 4 MiB counting 32
  bytes per cell plus its text; text cells are shown cut to 4 KiB.
- **Descriptors.** Open files are bounded per instance (an eighth of the
  editor's descriptor limit, 256-4096); past that `File::open` fails with
  an error instead of opening.

## 6. UI

Build an ordinary tree with string keys and call `panel.rerender(&root)`:
the SDK diffs it against the previous tree inside your plugin and sends
only patches. Inputs are uncontrolled: the editor owns the text the user
types, and every event carries the current values of edited inputs in
`event.values`. Tables are virtual: declare `table(columns, row_count,
generation)` and answer `fetch_rows`; bump `generation` (or call
`panel.rows_changed`) when the rows change. `TablePager` keeps plugin-side
result sets and serves pages from them.

## 7. Testing natively

Natively every import is a fake from `arcode_plugin::testing`, so plugin
logic runs under `cargo test` without wasm:

```rust
#[test]
fn scan_lists_todos() {
    arcode_plugin::testing::reset();
    arcode_plugin::testing::with(|h| {
        h.gesture = true;
        h.files = vec!["a.rs".into()];
    });
    let mut p = Todo::default();
    p.command("scan", Json::from("{}".to_owned())).unwrap();
    arcode_plugin::testing::with(|h| assert!(h.panel("list").is_some()));
}
```

`FakeHost` exposes every call (log, notifications, status, quick picks, kv,
documents, opened files, scratch buffers, HTTP requests, panels) for
arranging and asserting.

## 8. Installing and trust

Users install from a folder; a newly installed plugin starts **disabled**.
Enabling it shows its capabilities and approves that exact `plugin.wasm`
(its sha256 is stored in the user's `plugins.toml`). If the file changes,
the plugin stops running until the user approves it again; an update that
asks for different capabilities is disabled until re-enabled. Grants live
in the user's config and data directories only; nothing in a repository
can install, enable or authorize a plugin.

To install: Plugins activity → Installed → the folder button (or "Plugins:
Install from Folder…") and pick the folder that holds `plugin.toml`. The
host validates it (manifest, an id outside the reserved `arcode.`
namespace, regular files only, `plugin.wasm` at most 32 MiB), copies
`plugin.toml`, the entry, `README.md` and `LICENSE` into the user's data
folder, records the sha256 of the copied `plugin.wasm` and compiles it
once. The card then shows the plugin disabled; its switch shows what the
manifest lets it ask for before enabling it. Installing the same id again
updates it. Uninstall (installed plugins only) removes the folder, its
`plugins.toml` entry and its grants in every project, so a plugin
installed later with the same id is asked again. Its stored data
(`host::kv`) is kept, as for a disabled plugin.

Each card also lists the permissions the user gave (Revoke asks again next
time) and opens the plugin's log: your `host::log` lines, stdout and
stderr, and the network log (time, method, host, port, status; never
bodies or paths). Consent prompts are drawn by the editor from what the
host parsed (method, scheme, host, port) and your plugin's id; your
display name is shown quoted and capped, so do not put instructions in it.

During development use "Plugins: Load Unpacked…" (session only, no copy)
and "Plugins: Reload".
