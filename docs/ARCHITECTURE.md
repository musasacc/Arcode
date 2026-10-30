# Arcode architecture

Arcode is a native IDE in Rust. The UI is GPUI (pinned `=0.2.2`). Everything
that is not drawing lives in small, UI-agnostic crates with their own tests.

## Hard rules

- No telemetry, no accounts, no network access unless the user triggers it.
- Redraw only when something changes. Never run a permanent frame loop.
  Views call `cx.notify()` when their state changes; background work wakes
  the UI through channels, not polling timers.
- Keep files small (aim < 400 lines) and modules separated by concern.
- Every crate below `arcode` (the app) must build and test without GPUI.
- Cross-platform: Linux, macOS, Windows. Anything platform-specific is behind
  `cfg` and has a fallback.

## Crates

| crate                  | depends on GPUI | role |
|------------------------|-----------------|------|
| `arcode-buffer`        | no  | ropey text buffer, edits, selections, movement, undo/redo, LSP/tree-sitter coordinate conversion |
| `arcode-syntax`        | no  | language registry, tree-sitter parse + incremental reparse, highlight spans for a line range |
| `arcode-lsp`           | no  | JSON-RPC over stdio, one tokio task per server, server registry, merged diagnostics |
| `arcode-terminal`      | no  | PTY + VT emulation (alacritty_terminal), grid snapshots, key encoding |
| `arcode-config`        | no  | TOML config, themes (TOML), fonts, persisted state (recent projects, session) |
| `arcode-git`           | no  | local git (branch, later diff/blame/stage) via the `git` CLI / `.git` files; never fetches |
| `arcode-editor`        | yes | `EditorView`: GPUI element that renders an `arcode-buffer` with highlights, diagnostics, cursor |
| `arcode-terminal-view` | yes | `TerminalView`: GPUI view rendering an `arcode-terminal` grid and forwarding input |
| `arcode`               | yes | the app binary: workspace shell (title bar, activity bar, sidebar, panes, panels, status bar), welcome screen, wiring |
| `arcode-spike`         | yes | Step 0 spike: scroll a 100k-line file, frame-time benchmark |

## Coordinates

- Buffer positions are **char indices** into the rope (`usize`).
- `Point { line, column }` is zero-based; `column` counts **chars**.
- tree-sitter uses byte offsets and `(row, byte column)`; `arcode-buffer`
  produces those for every edit (`Edit`).
- LSP positions are `(line, UTF-16 code unit)` by default; the client also
  offers UTF-8 via `general.positionEncodings` and the buffer converts both.

## Threading model

- GPUI owns the main thread and its own executor.
- `arcode-lsp` owns a tokio runtime on background threads. The UI talks to
  it through non-blocking handles; events come back over a channel whose
  receiver is runtime-agnostic, polled by a GPUI task.
- `arcode-terminal` runs alacritty's PTY event loop on its own thread and
  signals "wakeup" (redraw needed) over a channel.

## Diagnostics

Several servers may serve one file. Diagnostics are stored per
`(uri, server name)` and merged for display; each carries its source
(`diagnostic.source` or the server name). The editor draws a wavy underline
in the severity color, and hovering shows the message. The Problems panel
lists all of them; clicking one opens the file and moves the cursor there.
