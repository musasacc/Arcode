# The manifest: `plugin.toml`

Every plugin folder has a `plugin.toml`. Arcode reads it when a window
opens, long before it loads any of your code, and registers everything
the plugin contributes from it. The manifest is also what users see when
they decide whether to enable your plugin.

The rules on this page come from the host's parser
(`crates/arcode-plugin-host/src/manifest/`).

- **Unknown keys are errors.** A typo like `discription` makes Arcode skip
  the plugin, with the reason in **Plugins: Show Log**.
- The file must be at most 256 KiB.
- For installed (and bundled) plugins, `plugin.toml` must be a regular
  file (not a symbolic link) in a folder named exactly like the plugin id.
- User-visible text (names, titles, descriptions) must not be empty, must
  not contain control characters, and has a length limit given below.

A complete example, using every section:

```toml
[plugin]
id = "acme.todo"
name = "TODO Tools"
version = "1.2.0"
api = "0.1"
entry = "plugin.wasm"
description = "Lists and explains TODO comments"

[capabilities]
workspace = "read"
editor = "context"
editor-write = false
network = { hosts = ["api.example.com"] }

[limits]
memory-mb = 64
task-seconds = 60

[[contributes.languages]]
id = "todo-list"
name = "TODO List"
extensions = ["todo"]
line-comment = "#"

[[contributes.commands]]
id = "scan"
title = "TODO: Scan Project"

[[contributes.commands]]
id = "explain"
title = "TODO: Explain Line"
when = "language == rust"
key = "secondary-k t"

[[contributes.code-lenses]]
languages = ["rust", "python"]

[[contributes.panels]]
id = "list"
title = "TODOs"
location = "sidebar"
icon = "list"

[[contributes.openers]]
id = "todo-file"
patterns = ["*.todo"]
title = "TODO List"

[[contributes.settings]]
key = "tags"
type = "string"
default = "TODO,FIXME"
description = "Comma-separated tags to look for"
```

## `[plugin]`

| Key | Type | Required | Default | Rules |
|---|---|---|---|---|
| `id` | string | yes | | 3 to 64 characters: lowercase letters, digits, `.` and `-`, starting with a letter or digit. Ids starting with `arcode.` are reserved for plugins that ship with Arcode. |
| `name` | string | yes | | Display name, at most 64 characters. |
| `version` | string | yes | | Your plugin's version, at most 32 characters. Shown to users, not interpreted. |
| `api` | string | yes | | The plugin API version, `major.minor`. Arcode currently supports `"0.1"`. |
| `entry` | string | no | `"plugin.wasm"` | The component file in the plugin folder. A plain file name ending in `.wasm`: at most 64 characters of letters, digits, `.`, `_` and `-`, not starting with `.`. |
| `description` | string | no | none | One line shown in the Plugins view, at most 512 characters. |

Pick an id you will keep: grants, stored data and the user's
enabled/disabled choice are all keyed by it. A good pattern is
`<your-name-or-org>.<plugin>`.

Arcode checks `api` before it compiles anything. A plugin asking for a
version Arcode does not support is refused with "plugin requires plugin
API x; this Arcode supports 0.1".

## `[capabilities]`

What the plugin may ask for. Everything defaults to "nothing". Users see
this list in the Plugins view before they enable a plugin, so ask only
for what you use. [Permissions](permissions.md) explains when the user is
asked and what happens on each answer.

| Key | Type | Default | Values |
|---|---|---|---|
| `workspace` | string | `"none"` | `"none"`, `"read"`, `"read-write"`. Access to the project folder through `/project`. |
| `editor` | string | `"none"` | `"none"`, `"context"`, `"all-open"`. Which documents `editor::*` functions may read. |
| `editor-write` | bool | `false` | Whether the plugin may apply edits to documents. |
| `network` | table | none | `{ hosts = [...], background = false }`. Hosts the plugin may ask to contact. |

### `workspace`

- `"none"`: no project files. `workspace::find_files` returns
  `FsError::Denied`.
- `"read"`: after the user allows it, the project folder is available
  read-only at `/project`.
- `"read-write"`: the same prompt has an extra "Also allow changing
  files" checkbox. If the user leaves it unticked, the plugin gets read
  access only.

### `editor`

- `"none"`: no document access at all (`EditorError::Denied`).
  `editor::open_file` and `editor::open_scratch` still work.
- `"context"`: the document a call is about (the document a code lens was
  shown for, or the active document when a command runs), plus documents
  of a language the plugin registers under `[[contributes.languages]]`
  (only if Arcode does not have that language built in). No prompt.
- `"all-open"`: any open document. This needs a grant that the current
  version of Arcode has no way to give, so such a plugin behaves like
  `"context"`. Do not use it.

### `editor-write`

`true` lets the plugin call `editor::apply_edits`. The user is asked the
first time, per project.

### `network`

```toml
network = { hosts = ["api.example.com", "*.example.org", "localhost:8080"] }
```

| Key | Type | Default | Rules |
|---|---|---|---|
| `hosts` | list of strings | required | At least one, at most 256 entries. Each entry is `*` (any host), a lowercase host name (`example.com`), a wildcard suffix (`*.example.com`), an IPv4 address or an IPv6 address in brackets (`[::1]`), optionally followed by `:port`. |
| `background` | bool | `false` | Requests outside user actions. Never granted by default, and the current version of Arcode offers no way to grant it. |

The list only says which hosts the plugin may **ask** for. Each host is
still approved by the user on first use. A plugin that imports the `http`
interface without a `network` capability is refused at load.

## `[limits]`

| Key | Type | Default | Rules |
|---|---|---|---|
| `memory-mb` | integer | `64` | Linear memory of the plugin in MiB. Clamped to `1..=256`. |
| `task-seconds` | integer | `60` | CPU budget of a call after it called `host::begin_task`. Clamped to `1..=600`. |

Values outside the range are clamped, not rejected.

## `[[contributes.commands]]`

A palette command. Repeat the table for each command.

| Key | Type | Required | Rules |
|---|---|---|---|
| `id` | string | yes | Local id, unique among your commands: lowercase letters, digits, `.`, `_`, `-`, starting with a letter or digit, at most 64 characters. Your code receives this id. |
| `title` | string | yes | Palette title, at most 128 characters. A `Category: Name` title is listed under that category; a title without a category is listed as `Plugins: <title>`. |
| `when` | string | no | Only `language == <id>`: the command is listed (and its key works) only while the active document has that language. |
| `key` | string | no | Default key binding, at most 64 characters. |

Arcode namespaces ids: your command `scan` is `acme.todo/scan` to the rest
of the app. If another command already uses the same title, yours is
listed with the plugin id appended ("File: Save (acme.todo)"), so a
plugin can never take over a built-in command's name.

Key bindings use Arcode's key syntax: modifiers `ctrl`, `alt`, `shift`,
`cmd`, and `secondary` (Cmd on macOS, Ctrl elsewhere), joined to the key
with `-`; a space separates the keys of a chord. Examples:
`"secondary-enter"`, `"alt-shift-t"`, `"secondary-k t"`. The binding works
in editors. It is ignored when it is invalid, or when it (or its first
chord key) is already taken by a built-in binding or another plugin, so
check that your command still has a palette entry.

## `[[contributes.code-lenses]]`

| Key | Type | Required | Rules |
|---|---|---|---|
| `languages` | list of strings | yes | Non-empty list of language ids. |

When a document in one of these languages becomes visible, and shortly
after each edit, Arcode calls your `code_lenses` method. This is also an
activation event: opening such a document starts your plugin. Language
ids are those of Arcode's built-in languages (`rust`, `python`,
`javascript`, `typescript`, `typescriptreact`, `javascriptreact`, `go`,
`c`, `cpp`, `java`, `ruby`, `php`, `lua`, `shellscript`, `html`, `css`,
`json`, `yaml`, `toml`, `markdown`) or one your plugin registers. Plain
text documents never get lenses.

## `[[contributes.panels]]`

| Key | Type | Required | Rules |
|---|---|---|---|
| `id` | string | yes | Local id (same rules as command ids), unique among your panels. |
| `title` | string | yes | At most 128 characters. |
| `location` | string | yes | `"sidebar"`, `"bottom"`, `"editor"` or `"editor-beside"`. |
| `icon` | string | no | An icon name: 1 to 32 characters of lowercase letters, digits and `-`. |

Where panels go:

- `"sidebar"`: the panel gets its own entry in the activity bar (the icon
  strip on the left), after the built-in ones, and opens in the sidebar.
- `"editor"`: a tab in the active editor pane.
- `"editor-beside"`: a tab in the pane to the right of the active editor
  (split off if needed); the editor keeps the focus.
- `"bottom"`: currently opens as an editor tab too.

Your code can override the location when it opens a panel
(`Panel::open(id, title, Some(Location::Bottom))`).

Icon names map onto Arcode's icon set: `database`, `table`, `rows`,
`globe`, `link`, `external-link`, `play`, `refresh`, `search`, `eye`,
`file`, `file-text`, `folder`, `terminal`, `settings`, `gear`, `history`,
`bug`, `package`, `info`, `warning`, `error`, `check`, `copy`, `save`,
`trash`, `plus`, `add`, `list`, `code`, `git-branch`. Any other valid name
shows a generic plugin icon.

## `[[contributes.openers]]`

An opener shows your view instead of the text editor (or the "binary
file" notice) for matching files.

| Key | Type | Required | Rules |
|---|---|---|---|
| `id` | string | yes | Local id, unique among your openers. |
| `patterns` | list of strings | yes | Non-empty list of glob patterns. |
| `title` | string | yes | At most 128 characters. |

A pattern without `/` matches the file name (`*.sqlite`); a pattern with
`/` matches the path relative to the project (`data/*.csv`), and `*` does
not cross `/` there. Only files inside the project are offered to
openers. When a matching file opens, Arcode opens a tab named after the
file and calls `open_view(panel, "<opener id>", {"path": "<relative path>"})`.

## `[[contributes.languages]]`

Registers a language for file extensions, so documents get a language id
your commands, lenses and `when` clauses can use.

| Key | Type | Required | Rules |
|---|---|---|---|
| `id` | string | yes | Language id: lowercase letters, digits and `_ + # . -`, starting with a letter or digit, at most 32 characters. Unique in your manifest. |
| `name` | string | yes | Display name, at most 64 characters. |
| `extensions` | list of strings | no | Extensions without the dot (`"http"`), at most 32 characters of letters, digits, `_`, `-`, `+`. |
| `line-comment` | string | no | Line comment token, at most 8 characters. |

Language ids are global, not namespaced. A contributed language only
applies to files Arcode would otherwise open as plain text, and never to
files that usually hold secrets or notes (`.txt`, `.env`, `.key`, `.pem`,
`.log`, and similar). Arcode does not highlight contributed languages.

## `[[contributes.settings]]`

User-adjustable values with defaults. Your code reads them with
`host::settings()` or `host::settings_as::<T>()`.

| Key | Type | Required | Rules |
|---|---|---|---|
| `key` | string | yes | Local id, unique among your settings. |
| `type` | string | yes | `"boolean"`, `"integer"`, `"string"` or `"enum"`. |
| `default` | matching type | yes | Must match `type`; for `"enum"` it must be one of `options`. |
| `description` | string | no | At most 512 characters. |
| `options` | list of strings | for `"enum"` only | The allowed values, each at most 64 characters. Not allowed for other types. |

```toml
[[contributes.settings]]
key = "style"
type = "enum"
options = ["short", "long"]
default = "short"
```

The current version of Arcode has no editor for plugin settings yet, so
`host::settings()` returns the manifest defaults.

## Limits on lists

Each `[[contributes.*]]` list holds at most 256 entries.

## How Arcode finds plugins

Arcode reads manifests from these places, in this order. A later one
replaces a plugin with the same id from an earlier one.

1. **Bundled**: `<install prefix>/share/arcode/plugins/<id>/`, next to the
   Arcode binary. A binary built inside a checkout (anything under its
   `target/` folder, debug or release) also reads the checkout's
   `target/wasm-plugins/<id>/`.
2. **Installed**: `<data folder>/plugins/<id>/`, written by "Install from
   Folder…" (see [Publishing](publishing.md)).
3. **Unpacked**: folders from **Plugins: Load Unpacked…** and the
   `ARCODE_PLUGIN_DEV` environment variable, for the current session.

A folder whose manifest is invalid is skipped, and the reason is listed at
the end of **Plugins: Show Log**.
