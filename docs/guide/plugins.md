# Plugins

Plugins add commands, panels, code lenses and file viewers to Arcode.
They are small WebAssembly programs that run in a sandbox: a plugin can
only read your project, edit documents or use the network after you
allow it, and Arcode enforces that, not the plugin.

There is no plugin store. Plugins come with Arcode, or you install them
from a folder.

## The bundled plugins

- **HTTP Client**: send requests from `.http` and `.rest` files. A
  "▶ Send request" lens sits above each request; the response opens
  beside the editor with its status, headers, timing and body. Supports
  variables, environment files (`http-client.env.json` and
  `http-client.private.env.json`), and a history per project. Ctrl+Enter
  (Cmd+Enter) sends the request at the cursor. See
  [`plugins/http-client/README.md`](../../plugins/http-client/README.md).
- **Database Viewer**: browse and query SQLite databases in your project.
  Opening a `.sqlite`, `.sqlite3` or `.db` file shows its tables, a data
  grid that pages through millions of rows, and a query tab. It opens
  databases read-only unless you tick "Allow writes" and gave it write
  access. A **Databases** entry in the activity bar lists every database
  in the project.

## The Plugins view

Click the puzzle icon in the activity bar (or run **Plugins: Show
Installed**). The **Installed** tab has a card per plugin with:

- its name, version, description and where it comes from (bundled with
  Arcode, installed by you, or loaded unpacked);
- its state: inactive (not running, costs nothing), active, crashed or
  disabled;
- an **enable switch**;
- when expanded, what the plugin's manifest lets it ask for ("Read
  project files (asks first)", "Send requests when you click, to any host
  (asks per host)", ...);
- **Permissions**: every answer you gave it, each with a **Revoke**
  button;
- **Log**: the plugin's own log and its network log;
- **Restart** after a crash, and **Uninstall** for plugins you
  installed.

The **Languages** tab is the language pack page described in
[Languages](languages.md#language-packs).

A plugin that is not running costs no memory and no CPU. Plugins start
when you use them (run one of their commands, open one of their panels or
files) and stop after ten idle minutes.

## Installing a plugin

1. Get the plugin folder (it contains `plugin.toml` and `plugin.wasm`)
   and unpack it anywhere.
2. In the Plugins view, click the folder button on the Installed tab, or
   run **Plugins: Install from Folder…**, and pick the folder.
3. The plugin is copied into your data folder and listed **disabled**.
4. Turn it on. Arcode first shows what the plugin may ask for. Enabling
   it approves that exact version of the plugin.

If the plugin's files change later without being installed again, it
does not run until you approve it again. An update that asks for
different permissions is disabled until you enable it again.

To try a plugin without installing it, run **Plugins: Load Unpacked…**
and pick its folder; it stays loaded until Arcode quits.

## Permissions

Plugins ask before they:

- **read your project files** (asked once per project, the first time the
  plugin starts there; a separate checkbox allows changing files);
- **edit your documents** (asked the first time per project);
- **contact a host on the network** (asked the first time for each host).

Each prompt offers **Allow once**, **Always for this project**,
**Always** and **Deny**. Esc closes it without deciding (you will be asked
again). A denial is remembered for the project until you revoke it.

Some rules hold no matter what you allow:

- A plugin can use the network only while handling something you did
  (a click, a command), never in the background.
- A plugin never sees files outside the project folder, environment
  variables, or other programs.
- Nothing inside a project can install, enable or grant permissions to a
  plugin; your answers are stored in your own config and data folders.
- `[plugins] network = false` in `config.toml` (or the switch in
  Settings → Plugins) turns off all plugin networking.

Revoke any answer in the plugin's Permissions section; the plugin asks
again next time.

## When a plugin misbehaves

Arcode stops a plugin that uses too much CPU or memory, and shows
"<Plugin> stopped: <reason>" in its panels. The plugin restarts on the
next action; after three crashes in ten minutes it stays off for the
session (**Restart** on its card brings it back). Running plugin tasks
show in the status bar with a Cancel button, and Esc cancels the newest
one.

**Plugins: Show Log** shows what every plugin logged, and lists plugin
folders Arcode skipped and why.

## Writing plugins

See the [plugin author guide](../plugins/README.md).
