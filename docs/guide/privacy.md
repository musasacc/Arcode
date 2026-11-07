# Privacy

Arcode does not connect to the internet on its own. It has no telemetry,
no crash reporting, no usage statistics, no update check, no account and
no plugin store. Fonts and icons are bundled; nothing is fetched at
startup. It never downloads language servers, debug adapters or packages
for you; the language servers of a release come inside the package you
installed (see [Languages](languages.md#bundled-language-servers)), and
their built-in settings turn off the schema and type downloads some of
them would otherwise make, including schemas a YAML, JSON or TOML file
links to (see [Server settings](languages.md#server-settings)).

Everything that can use the network does so only as a direct result of
something you do. This page lists all of it. The one thing Arcode can
share without a click, once you turn it on, is your
[Discord presence](#discord-presence): Arcode hands it to the Discord app
on your computer, and the Discord app sends it to Discord's servers so
your Discord friends can see it.

## Things Arcode itself starts, only when you ask

| What | When | What goes over the network |
|---|---|---|
| **Plugin requests** | Only while a plugin handles your click or command, and only to hosts you approved (asked once per host). | Whatever the plugin sends, for example the request in an `.http` file. |
| **Packages: Check for updates** | When you click it. | Your package manager asks its registry for newer versions. |
| **Packages: Add, Remove, Update, Install packages** | When you click them. | Your package manager downloads packages and updates lockfiles. |
| **Package documentation** | When you choose it in a dependency's menu. | Your browser opens the package's registry page. |

### Plugins in detail

- A plugin's manifest lists the hosts it may ask for. The first request
  to each host shows a prompt with the method, scheme, host and port,
  taken from the actual URL. You choose Allow once, Always for this
  project, Always or Deny.
- Requests are only possible while the plugin handles a user action (a
  command, a button click, ...). Background network access is never
  granted.
- Redirects are checked again at every hop; credentials headers are
  dropped when a redirect goes to another origin.
- `[plugins] network = false` in `config.toml`, or the switch in
  Settings → Plugins, turns off plugin networking completely.
- The plugin's network log (Plugins view → the plugin's **Log** button)
  lists every request it made or Arcode refused: time, method, host, port
  and status. Bodies, paths and headers are never logged.
- Plugins honour the standard proxy variables (`HTTPS_PROXY`,
  `NO_PROXY`, ...).

The two bundled plugins: the **HTTP Client** sends only the requests you
click "Send" for. The **Database Viewer** never uses the network.

## Discord presence

Off by default. When you turn it on (`[discord] enabled = true`, the
switch in Settings → Workspace, or **Preferences: Toggle Discord Rich
Presence**), Arcode tells the Discord app running on your computer, over
its local socket or pipe, what you are doing: "Editing a Rust file",
"Browsing files", "Choosing a project" or "Idle", the Arcode logo and the
file's language icon, and how long the project has been open
(`show_elapsed`). The file's name (`show_file_name`) and the project
folder's name (`show_project_name`) are sent only if you turn them on.
Paths, file contents, branch names and other files' names are never
sent. Discord then shows this to your Discord friends. Arcode itself
makes no network connection for it. See [Discord presence](discord.md).

## Programs you run

These are your own tools; Arcode starts them, but what they do is up to
them:

- commands you type in the **terminal**, and commands started by the
  **Run** button, run buttons and the debugger;
- **language servers** and **debug adapters**: separate programs you
  installed. Some of them may go online by themselves, for example a Rust
  or Go language server that runs your build tool, which may fetch
  dependencies;
- **install commands** shown on the Languages page, which you copy and
  run yourself.

Arcode starts language servers only for languages whose pack is enabled;
turn a pack off (`[language.<id>] enabled = false`) or set `servers = []`
to stop a server from starting.

## Git

All git features are local. Arcode never fetches, pulls or pushes.

## What is stored, and where

Arcode stores data only in your user folders (see
[Configuration](configuration.md#where-files-live)):

- `config.toml`, `snippets.toml` and your themes;
- `state.toml`: open projects, tabs, cursors, breakpoints and similar
  session state;
- which plugins are enabled, your permission answers, installed plugins,
  data plugins store, and compiled plugins.

Arcode keeps none of its own state in your projects; the only file it
creates there for itself is `.arcode/launch.toml`, when you ask for it. No file inside a project can change Arcode's settings,
install or enable a plugin, or grant one permissions.
