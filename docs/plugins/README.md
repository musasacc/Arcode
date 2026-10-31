# Writing Arcode plugins

Arcode plugins are small WebAssembly programs written in Rust. A plugin can
add commands to the command palette, show code lenses above lines, open
panels with buttons, inputs, tables and trees, read and edit documents,
read project files and send HTTP requests. Arcode runs every plugin in a
sandbox and draws all of its UI itself, so a plugin cannot slow the editor
down or do anything the user did not allow.

This guide is for plugin authors. If you only want to use plugins, see
the Plugins section of the [user guide](../guide/README.md).

## Contents

| Page | What it covers |
|---|---|
| [Getting started](getting-started.md) | From an empty folder to a plugin running in Arcode |
| [The manifest](manifest.md) | Every `plugin.toml` field |
| [API reference](api.md) | Every host function and every callback your plugin implements |
| [Building UI](ui.md) | Panels, widgets, events, incremental updates, tables |
| [Permissions](permissions.md) | Capabilities, consent prompts, what users see, grants |
| [Testing and debugging](testing.md) | Native unit tests, end-to-end tests, logs |
| [Publishing](publishing.md) | Folder layout, versioning, how users install a plugin |

The fastest way in is the example plugin in
[`plugins/hello-world`](../../plugins/hello-world): a palette command, a
code lens and a small panel, with comments on every line that matters.
Copy it and change it. The two plugins that ship with Arcode,
[`plugins/http-client`](../../plugins/http-client) and
[`plugins/db-viewer`](../../plugins/db-viewer), are larger, real examples.
The full design of the plugin system, with the reasoning behind each
rule, is in [docs/PLUGINS.md](../PLUGINS.md).

## How a plugin is put together

A plugin is a folder with two files:

```
my-plugin/
  plugin.toml    the manifest: id, name, what the plugin contributes,
                 what it may ask for
  plugin.wasm    the code: a WebAssembly component built for wasm32-wasip2
```

You write the code in Rust against the `arcode-plugin-api` crate (its
library name is `arcode_plugin`). The crate gives you:

- the `Plugin` trait, whose methods Arcode calls (a command ran, a panel
  was shown, a button was clicked, ...);
- plain functions for everything you can ask Arcode to do (`host::notify`,
  `editor::text`, `kv::set`, `http::send`, ...);
- builders for panel UI (`vstack`, `text`, `button`, `table`, ...) and a
  `Panel` type that sends only what changed;
- a `testing` module that fakes Arcode, so your plugin logic runs under a
  plain `cargo test` without any WebAssembly.

Under the hood the boundary between Arcode and your plugin is defined in
WIT, the WebAssembly interface language, in
[`crates/arcode-plugin-api/wit/arcode-plugin.wit`](../../crates/arcode-plugin-api/wit/arcode-plugin.wit).
You do not need to read it to write a plugin; the [API reference](api.md)
covers the same ground with Rust names.

## What plugins can do

- **Commands** in the command palette, optionally with a default key
  binding and a `when` clause that limits them to one language.
- **Code lenses**: clickable labels above lines of documents in the
  languages you list.
- **Panels** in the sidebar (with their own activity-bar icon), as editor
  tabs, or beside the editor. Panels are built from host widgets: text,
  badges, buttons, inputs (including a highlighted code input),
  drop-downs, checkboxes, key-value lists, virtual tables, trees, code and
  JSON viewers, tabs, splits and stacks.
- **Openers**: show your own view instead of the text editor for files
  that match a pattern (the Database Viewer opens `*.sqlite` this way).
- **Languages**: register a language id for file extensions, so commands,
  lenses and `when` clauses can target it.
- **Documents**: read the text and selections of the document a command or
  lens is about, apply edits as one undo step, open files, open untitled
  scratch buffers.
- **Project files**: read (and, if allowed, write) files under the
  project folder with ordinary `std::fs`, and search file names with a
  glob that honours `.gitignore`.
- **Network**: send HTTP and HTTPS requests to hosts the user approves,
  while handling a click.
- **Storage**: a small key-value store per plugin, global or per project.
- **Feedback**: notifications, a status-bar item, a quick pick, long
  running tasks with a Cancel button, and a log.

## What plugins cannot do

These are not missing features you can work around; the sandbox does not
offer them at all.

- Start processes, read environment variables, or open raw sockets.
- See files outside the project folder. The project is the only folder
  the plugin can reach, at `/project`, and only after the user allowed it.
- Read any open document. A plugin reads the document a command or lens
  is about, and documents of a language it registers itself.
- Use the network on its own. Requests work only while handling a user
  action (a command, a button click, ...), only to hosts the manifest
  lists, and only after the user approved each host.
- Draw anything. There is no canvas, HTML or web view; Arcode draws every
  widget, so plugin UI follows the user's theme, font and low-end mode.
- Ship images or icons. Panels and buttons name icons from Arcode's own
  icon set.
- Run at startup, on a timer, or in the background. A plugin runs only
  when Arcode calls it.
- Install, enable or grant itself anything. Nothing in a project folder
  can install or authorize a plugin.

## The sandbox

Each plugin runs in its own WebAssembly instance, on its own thread, with
its own memory limit. Arcode uses [wasmtime](https://wasmtime.dev) and the
WebAssembly component model.

- **Files** go through WASI. When the user allows project access, the
  project folder is mounted at `/project`, read-only or read-write. Paths
  that try to leave it (`..`, absolute paths, symbolic links pointing
  outside) fail.
- **Network** goes through Arcode's own `http` interface, which checks the
  manifest, the user's answer for that host, and whether the call came
  from a user action, again on every redirect. The standard WASI HTTP and
  socket interfaces are refused.
- **stdout and stderr** go to the plugin's log, so `println!` and
  `eprintln!` work for quick debugging.
- **Time and randomness** (`std::time`, `RandomState`) work as usual.
- **No environment, no arguments, no stdin.**

When Arcode loads a component it checks the list of interfaces the
component imports. A component that imports anything outside Arcode's API
and the allowed WASI interfaces is refused, and so is one that imports
`http` while its manifest declares no `network` capability.

## Lifecycle

Arcode reads every `plugin.toml` when a window opens. That is all it does
at startup: it registers your commands, panels, lenses and openers from
the manifest alone, without loading any WebAssembly.

Your plugin **activates** (Arcode creates the instance and calls
`activate`) the first time one of these happens:

- one of your commands runs (palette, key binding, code lens, status-bar
  item);
- a document of a language listed under `[[contributes.code-lenses]]`
  becomes visible;
- a file matching one of your openers is opened;
- one of your panels is shown.

There is no startup event. `activate` gets the reason
(`Activation::Command("say-hello")`, `Activation::Language("rust")`, ...),
or `Activation::Restore` when the plugin comes back after being unloaded
or after a crash.

After **10 minutes without a call**, and with none of its panels open, an
instance is unloaded: Arcode calls `deactivate`, then drops the instance
and its thread. An idle plugin costs no CPU and no memory. Anything you
want to keep across unloads belongs in the [key-value store](api.md#kv-storage).

Calls into one plugin are **serial**: your plugin handles one call at a
time, on one thread, so your state needs no locking. A plugin that is
busy (for example waiting for an HTTP response) handles the next event
only after the current call returns.

**Disabling** a plugin in the Plugins view stops it at once and removes
everything it shows: panels, sidebar entry, lenses, status item, palette
commands and contributed languages.

## Budgets

Arcode limits how much CPU time each call may use, counting only time
spent running your code and in host functions, not time spent waiting for
the user, the network or the editor.

| Call | CPU budget |
|---|---|
| `activate` | 1 s |
| `command` | 2 s |
| `open_view`, `ui_event`, `fetch_rows` | 1 s |
| `code_lenses` | 100 ms |
| `deactivate` | 100 ms |
| any call after `host::begin_task` | `[limits] task-seconds` (default 60 s, at most 600 s) |

A call that runs over its budget is stopped: the instance is dropped, its
panels show "<Plugin> stopped: <reason>", and the next event starts a new
instance with `activate(Restore)`. Three crashes within ten minutes
disable the plugin for the rest of the session. Lenses are special: a
lens call that runs over its budget only loses its result, and after
three of those, lenses are turned off for that plugin for the session
while its commands keep working.

Long work (a big search, a slow query, a request) should call
`host::begin_task("Searching…")` at the start. That shows the title with a
Cancel button in the status bar, raises the budget to `task-seconds`, and
lets the user stop it. Poll `host::cancelled()` in loops; after a cancel
your call gets 2 seconds to return before Arcode stops it.

Other limits:

| What | Limit |
|---|---|
| Memory | `[limits] memory-mb`, default 64 MiB, at most 256 MiB |
| Notifications | 5 per minute; more are dropped and logged |
| Status item | at most one update per 100 ms is shown |
| Panel | 20,000 widgets, 64 levels deep, 8 MiB of text |
| Table page | 200 rows per request, 4 MiB per page, cells shown up to 4 KiB |
| Key-value store | 1 MiB per plugin and scope |
| HTTP | 50 MiB per response body, 64 requests per user action |
| Log | 1 MiB per plugin (oldest lines dropped), 16 KiB per line |
| Open files | an eighth of the editor's descriptor limit (256 to 4096) |

When memory runs out, the allocation fails inside your plugin (a Rust
plugin usually aborts with "out of memory"), Arcode reports it and
restarts the plugin on the next event. The editor itself is not affected.

## API version

This guide describes plugin API **0.1** (`api = "0.1"` in the manifest).
Arcode checks the version before it loads any code and refuses plugins
that need a version it does not have. Within 0.1, changes are additive:
new functions and interfaces may appear, existing ones keep their shape.
The SDK gives every `Plugin` method a default, so a newer SDK does not
break your source.
