# Permissions

A plugin can only do what its manifest declares **and** the user allows.
The manifest says what a plugin may ask for; the user decides, through
prompts that Arcode draws itself. Arcode enforces every rule in the host,
so a plugin cannot skip a prompt or fake one.

This page describes what you declare, when the user is asked, what the
user sees, and how grants are stored and revoked.

## Capabilities at a glance

| Capability | Manifest | Default | When the user is asked |
|---|---|---|---|
| Log, notifications, status item, quick pick, panels, kv storage, own settings | nothing to declare | allowed | never |
| Read the document a call is about | `editor = "context"` | allowed | never |
| Read any open document | `editor = "all-open"` | not available yet | see below |
| Edit documents | `editor-write = true` | ask | first edit in each project |
| Read project files | `workspace = "read"` | ask | first start of the plugin in each project |
| Change project files | `workspace = "read-write"` | ask | same prompt, separate checkbox |
| Network, during user actions | `network = { hosts = [...] }` | ask | first request to each host |
| Network, in the background | `network = { ..., background = true }` | denied | cannot be granted yet |
| Processes, environment, sockets, clipboard, files outside the project | not offered | | |

## User actions

Some things are only allowed while your plugin handles a **user action**
(Arcode calls it a gesture): a call that directly follows something the
user did.

These calls are user actions:

- `command`, whether run from the palette, a key binding, a code lens or
  the status item;
- `ui_event` for `Click`, `Submit`, `Select`, `Tab`, `Sort`, `RowActivate`
  and `TreeActivate`.

These are not:

- `activate`, `deactivate`, `code_lenses`, `open_view`, `fetch_rows`,
  `view_closed`;
- `ui_event` for `Change`, `Toggle` and `TreeToggle`.

Only user actions may send HTTP requests (otherwise
`HttpError::NoUserGesture`) and start a task with `host::begin_task`
(otherwise it returns `false`). One user action may send at most 64
requests. So "the plugin only sends when you click" is a promise the host
keeps, not one the plugin has to.

## Documents

- `editor = "none"`: no document can be read. `editor::open_file` and
  `editor::open_scratch` still work.
- `editor = "context"`: the plugin may read the document its current call
  is about: the document a code lens is shown for (in `code_lenses` and in
  the command the lens runs), or the active document when a command runs
  from the palette or a key. It may also read documents of a language it
  registers itself under `[[contributes.languages]]`, as long as Arcode
  does not have that language built in. No prompt.
- `editor = "all-open"`: meant for reading any open document after the
  user agreed. The current version of Arcode does not offer that grant,
  so the plugin is treated as `"context"`.

`editor-write = true` lets the plugin call `editor::apply_edits`. The
first edit in a project shows "Allow editing documents?". Edits are
checked before the prompt (document version, ranges), so a prompt never
appears for an edit that would fail anyway. Each `apply_edits` call is
one undo step for the user.

## Project files

With `workspace = "read"` or `"read-write"`, the first time your plugin
starts in a project Arcode asks before creating the plugin instance:

> **Allow access to project files?**
> "Your Plugin" (`acme.tool`, installed by you)
> Asks to: Read the files of project `my-app`
> Nothing outside the project is reachable.

For `"read-write"` the prompt has an extra checkbox, **Also allow
changing files**, unticked by default. When the plugin also has a
`network` capability, the prompt adds "It can also send data to hosts you
approve (each one is asked for)."

The answer decides how `/project` is mounted for the plugin: not at all,
read-only, or read-write. It is fixed for the life of the instance;
revoking it restarts the plugin before any more of its code runs. A user
who answers with the checkbox unticked has granted read only; writes then
fail with a permission error.

`workspace::find_files` also needs this grant.

## Network

`network.hosts` lists the hosts you may **ask** for. Each one still needs
the user's approval. The first request to a host shows:

> **Allow a network request?**
> "Your Plugin" (`acme.tool`, installed by you)
> Asks to: Send a POST request to `https://api.example.com:443`
> Your answer covers every request to api.example.com.
> Requests are only sent when you click; you are asked once per host.

Everything in the prompt except the quoted plugin name comes from Arcode,
not from your plugin: method, scheme, host and port are taken from the
parsed URL. The plugin name is shown quoted, on its own line, and cut to
80 characters, so do not try to put instructions in it.

Answers to network prompts are per host (lower-case, without the port).
A host pattern with a port in the manifest (`localhost:8080`) still
limits which ports the plugin may use. Redirects are checked again at
every hop, so a redirect to a new host shows a new prompt (or is refused).

The user's master switch, `[plugins] network = false` in `config.toml`,
refuses every plugin request without asking.

## Prompt answers and scopes

Every prompt offers the same answers:

| Button | Effect |
|---|---|
| **Allow once** | Allowed for this request (network, edits) or this run of the plugin (project files). Not stored. |
| **Always for this project** ("For this session" when no project is open) | Stored for this project. |
| **Always** | Stored for every project. Replaces narrower answers. |
| **Deny** | Refused, and remembered for this project: the plugin is not asked again until the user revokes the denial. |

Closing the dialog with Esc refuses this time only; the plugin will ask
again next time. The buttons ignore clicks and keys for the first half
second, so a click or key press meant for something else cannot land on
them. A prompt that belongs to a cancelled call closes by itself.

While a prompt is open, your plugin's call waits. The wait does not count
against your CPU budget, and the editor does not wait for your plugin.

## What users see before enabling a plugin

Plugins installed from a folder start **disabled**. Turning one on in the
Plugins view first shows the capability summary from its manifest, for
example:

- Read project files (asks first)
- Read and change project files (asks first)
- Read the document a command is run on
- Edit documents (asks first)
- Send requests when you click, to api.example.com (asks per host)
- Use the network in the background (off unless you allow it)

or "No access to files, documents or the network". Enabling approves that
exact `plugin.wasm` (Arcode stores its SHA-256). If the file changes later,
the plugin does not run until the user approves it again, and an update
that asks for different capabilities is disabled until the user enables it
again. Plugins bundled with Arcode and plugins loaded unpacked are
enabled right away.

Each plugin card in the Plugins view also lists, when expanded, what the
manifest lets the plugin ask for.

## Where grants live, and revoking them

- Global answers: `<config folder>/plugin-grants.toml`.
- Per-project answers: `<data folder>/projects/<hash of the project path>/plugin-grants.toml`.
- Enabled state and approved hashes: `<config folder>/plugins.toml`.

These are the user's Arcode folders (see the user guide's
[configuration page](../guide/configuration.md#where-files-live)), never
the project folder. Nothing inside a repository can install, enable or
authorize a plugin; a `plugin-grants.toml` committed to a project is
ignored.

In the Plugins view each plugin card has a **Permissions** section listing
every stored answer with a **Revoke** button. Revoking means the plugin
asks again next time. Revoking project file access restarts the plugin.
**Uninstall** removes the plugin's grants in every project, so a plugin
installed later with the same id starts from scratch.

## The network log

Every request a plugin makes, and every one Arcode refused (no user
action, denied host, master switch off), is recorded in an in-memory
network log of the last 500 entries per plugin: time, method, host, port,
and the status or the reason. Bodies, paths and headers are never
recorded. The **Log** button on the plugin's card shows it after the
plugin's own log.

## Designing for permissions

- Ask for the smallest capability that works. A plugin that only needs
  the current document should not ask for `workspace`.
- List specific hosts instead of `"*"` when you can; users see the list.
- Expect "no". Handle `HttpError::Denied`, `EditorError::Denied`,
  `FsError::Denied` and a missing `/project` with a clear message in your
  panel instead of an error.
- Do network work in commands and button clicks, and say what will happen
  on the button ("Send", "Fetch latest").
- Never store secrets from responses or headers in `kv`; it is a plain
  file in the user's data folder.
