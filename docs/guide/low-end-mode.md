# Low-end mode

Arcode is built to be light: an idle window draws nothing and uses no
CPU, plugins cost nothing until they are used, and large files are only
parsed where you look. Low-end mode goes further for older laptops,
machines with slow integrated graphics, and remote desktops.

## Turning it on

- in the first-run dialog (performance mode);
- on the Settings page, under Appearance;
- with **Preferences: Toggle Low-End Mode**;
- in `config.toml`:

  ```toml
  [ui]
  low_end_mode = true
  ```

## What it changes

| | Normal | Low-end mode |
|---|---|---|
| Animations | on (`ui.animations`, `ui.animation_ms`) | off |
| Cursor blinking | `editor.cursor_blink` | never blinks |
| Minimap | `editor.minimap` | hidden |
| Popups, menus, dialogs, toasts | soft shadow | flat, with a border |
| Inlay hints | on | off, unless you set `editor.inlay_hints` yourself |
| Inline git blame | on | off, unless you set `git.inline_blame` yourself |
| Background refreshes (diagnostic counts, git status) | short delay | three times the delay |
| Running time of a command in the terminal tab | updates every second for the first minute | updates every minute |
| Rust files outside a project | analysed when their folder holds up to 1,000 Rust files | up to 200 ([more](languages.md#files-outside-a-project)) |

Settings you set explicitly win: with `low_end_mode = true` and
`inlay_hints = true`, inlay hints stay on.

When you turn low-end mode on from the Settings page or the palette,
Arcode switches inlay hints and inline blame off in `config.toml` and
marks the lines with a `[low-end mode]` comment. Turning low-end mode off
turns back on only the lines it marked, so anything you changed yourself
in between is left alone.

## More ways to save resources

These help on any machine:

- `editor.large_file_bytes` and `editor.large_file_lines` (default 4 MiB
  and 50,000 lines): lower them so big files skip the full syntax tree
  sooner.
- `editor.hover = false` and `editor.occurrence_highlight = false` cut
  language server traffic while you move around.
- `editor.completion.auto_trigger = false` opens completion only on
  Ctrl+Space.
- Turn off language servers you do not need for a language
  (`[language.<id>] servers = []`) or the whole language pack
  (`enabled = false`).
- `ui.show_perf_stats` (on by default) shows frame time and memory use in
  the status bar, so you can see the effect of a change.
- `search.max_results` limits how much a project search collects.
- `terminal.scrollback` (default 10,000 lines) sets how much terminal
  history is kept in memory.

## Graphics

Arcode draws with the GPU (Vulkan on Linux and Windows, Metal on macOS).
On Linux without a working GPU driver it can run on Mesa's software
Vulkan driver (lavapipe), which is slower but works; low-end mode helps
most there. See [Troubleshooting](troubleshooting.md#the-window-is-black-or-arcode-does-not-start).
