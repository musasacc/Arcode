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
| `arcode-git`           | no  | local git (branch, status, diff, blame, stage/unstage hunks, commit, conflicts) via the `git` CLI / `.git` files; never fetches |
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

## Editor extension points

Features built on the editor (git gutter and blame, run buttons,
breakpoints, merge conflicts, search, inlay hints, completion, signature
help, hover, code actions, rename, Vim, plugins) never patch the renderer.
They describe what they need with plain data from `arcode_editor::ext` and
hand it to the `Editor` entity. Full API docs live in the
`arcode-editor` crate docs; `cargo run -p arcode-editor --example
extension_points` shows all of them in one window.

- **Sets.** Every `set_*` call takes a set key chosen by the owner (`"git"`,
  `"debugger"`, `"search"`, ...) and replaces only that owner's items.
  `clear_decorations(set)` removes everything of a set. Positions (buffer
  lines / `Point`s) are mapped through every edit until the owner replaces
  them; items inside deleted text are dropped (a whole-line highlight
  keeps its surviving lines and is dropped when all of them are deleted;
  a text range is dropped when an edit replaces text across one of its
  ends). Text typed exactly at a range's start or end stays outside it, so
  a search match on `foo` does not grow into `foox`.
- **Colors** are theme keys (`ColorKey::new("diff_added").opacity(0.2)`),
  resolved against the active theme at paint time, never literal colors.
- **Gutter lanes** (`set_gutter_decorations`, `set_gutter_lane_visible`),
  left to right: `Breakpoints`, `Runnables`, the built-in diagnostic dot,
  line numbers, `Changes`. A lane takes space only while it has marks or
  was made visible. Marks are change bars (added / modified / deleted
  wedge), icons or dots (filled / ring / conditional). Clicking a mark
  emits `EditorEvent::GutterClicked { line, lane, tag, modifiers }`; the
  breakpoint lane also emits it for empty slots (`tag: None`).
- **Background highlights** (`set_background_highlights`): whole lines
  (full width, e.g. conflict regions, the debugger's current line) or text
  ranges (behind the glyphs, e.g. search matches).
- **Virtual text** (`set_virtual_text`): inline before a char (inlay
  hints) or after the end of a line (inline blame). It is not part of the
  buffer: the cursor skips it, it cannot be selected and all reported
  columns are buffer columns (`DisplayLine` keeps caret and glyph offsets
  apart).
- **Code lenses** (`set_code_lenses`): clickable labels joined by `|`,
  either on a block row above a line or at its end. Clicks emit
  `EditorEvent::CodeLensClicked { set, line, id }`. Block rows shift the
  display: `BlockMap` converts between buffer lines and display rows for
  layout, hit testing, scrolling and the scrollbar.
- **Popovers** (`show_popover(key, EditorPopover)`): a caller-built
  `AnyView` anchored below or above a buffer position, drawn on top via
  `defer_draw`. The editor keeps focus, so typing continues; the popover's
  `on_key` handler sees keys first (a list takes up/down/enter/escape).
  A popover whose anchor line is scrolled out of view is not drawn, and
  its `on_key` gets no keys until it is back in view.
- **Key interception** (`add_key_interceptor(priority, f)`): GPUI runs
  bound actions before element key listeners, so the editor installs one
  app-wide `intercept_keystrokes` hook in `arcode_editor::init`. For the
  focused editor it offers each keystroke to popover handlers (newest
  first), then to interceptors by priority; returning `true` consumes the
  key (no action, no text input). Skipped during IME composition.
- **Cursor shape** (`set_cursor_shape`): bar, block, underline, hollow.
  Block-like cursors stay visible (hollow) when the editor is unfocused.
- **Programmatic APIs**: `scroll_to_line(line, Top|Center|Fit)`,
  `set_selections` / `select_ranges`, `apply_edits(Vec<(Range<Point>,
  String)>)` (one undo step, cursors mapped), `insert_snippet` (LSP
  snippet syntax via `arcode-completion`: tab stops, mirrors, Tab /
  Shift+Tab / Escape; undoing the insertion ends the session),
  `text_in_range`, `word_at`, `point_to_lsp` /
  `lsp_to_point`, `point_to_offset` / `offset_to_point` and
  `position_for_point` (window position, e.g. to place custom UI).

## Completion and signature help

`arcode_editor::completion` owns the popup; the editor knows nothing about
LSP. A `CompletionProvider` (the app's `lsp_bridge::completion`) answers
completions, `completionItem/resolve` and signature help with plain editor
types (`CompletionItem` in buffer `Point`s). Local sources are merged in:
words of every open buffer (only when the provider has no items or there
is none; each editor keeps an incremental `WordIndex`, built lazily and not
at all above 2 MB), file names inside string literals that start like a
path (listed on the background executor) and snippets (built-in plus the
user's `snippets.toml`, loaded in the background and reloaded when saved in
an editor). Everything is ranked with `arcode_completion::Ranker`.

- **Never blocking.** Typing schedules a request after
  `editor.completion.delay_ms` from a task, so the bridge has already
  queued the keystroke's `didChange`. Each request has a sequence number;
  a newer one replaces the task (dropping the older one) and answers for
  anything but the latest are ignored. While a newer list loads, the old
  one stays shown and is re-filtered locally, so the popup does not
  flicker. Complete lists are only filtered as you type; incomplete ones
  are asked again (`TriggerForIncompleteCompletions`).
- **Session.** A list is anchored at the start of the completed word (or
  file name, or snippet placeholder for choices). Any cursor move or edit
  re-filters it; leaving the word closes it. Several cursors or a
  selection disable it.
- **Accepting** applies the item's `textEdit` (insert or, with
  Shift+Enter, replace range, extended by what was typed since the answer)
  plus `additionalTextEdits` as one undo step; snippets go through the
  snippet session. Items the server must resolve first get their
  additional edits applied when the resolve answer arrives, if they lie
  above the accepted line. `editor.action.triggerParameterHints` and
  `editor.action.triggerSuggest` commands are run by the editor.
- **Keys** come through the popover's `on_key`: up/down/page, Enter/Tab,
  Escape, and commit characters (accept, then type the character).
- **Popovers flip.** The editor measures each popover and opens it on the
  other side of its line when it does not fit (completion below, signature
  help above), and shifts it left to stay inside the window.

## Git

`arcode-git` wraps the `git` CLI with network access disabled; the app
never fetches, pulls or pushes. In the app (`crates/arcode/src/git/`):

- `GitStore` (one per workspace) finds the repositories of the project:
  the one containing the root plus repositories in sub-folders up to three
  levels deep (`discover_repos`). It keeps the status of the active one
  (the Source Control view follows the active editor's repository) and
  runs every operation on the background executor (`GitStore::run`).
  Nothing is created on open: a folder without git shows an
  "Initialize Repository" button that runs `git init` only when clicked.
- Watching is event driven (`notify`: inotify, FSEvents,
  ReadDirectoryChangesW). On Linux each non-ignored folder gets its own
  watch so `target/` and `node_modules/` cost nothing; elsewhere the root
  is watched recursively. Changes under a git directory (index, `HEAD`,
  refs, merge state) re-run status and make editors recompute their
  gutter and blame; worktree changes re-run status. Events are coalesced
  for 250 ms. Our own reads never write the index (`GIT_OPTIONAL_LOCKS=0`),
  so they cannot trigger themselves.
- `GitEditors` decorates editors through the extension points: change bars
  (set `"git"`, index against the buffer plus staged hunks mapped into
  buffer lines by `gutter_hunks`), recomputed 300 ms after the last edit;
  a popover on click with the old text and Stage / Unstage / Revert hunk
  (revert is an editor edit, so it is undoable); inline blame of the
  cursor line after `git.inline_blame_delay_ms` (set `"git-blame"`); the
  blame column toggled by "Git: Toggle Blame" (inline virtual text at
  column 0, set `"git-blame-column"`); merge conflict regions (set
  `"git-conflict"`) with Accept Current | Accept Incoming | Accept Both |
  Compare lenses, a counter in the status bar and next / previous
  conflict commands. Blame is cached per buffer version.
- Diff tabs are read-only editors without a path (so they can never be
  saved over a file) showing an inline diff (`inline_diff`), with a custom
  tab title kept by the pane.

## Large files

Above `editor.large_file_bytes` (default 4 MiB) or `editor.large_file_lines`
(default 50 000) the editor keeps no tree-sitter tree. Each viewport is
highlighted on the background executor from a parse of a small excerpt that
starts at a nearby top-level line (`arcode_syntax::highlight_excerpt`), and
bracket matching falls back to a bounded textual scan. An excerpt that would
start inside a block comment is found by scanning back for the nearest
comment delimiter and is parsed from the comment's opener (or with a
synthetic opener line in front when that is more than 2 MB back).

The language server path keeps no copy either: the bridge opens documents
with `LspManager::open_document_lazy` and a rope snapshot, so the text is
built only while `didOpen` (or a `didSave` with text) is sent. The few
short-lived multi-megabyte copies made while sending would stay resident in
glibc's thread heaps after its dynamic mmap threshold grows, so creating an
`LspManager` pins that threshold to 1 MiB (Linux/glibc only).

Measured RSS on a 100 000-line, 8.8 MB Rust file: 420 MB before; after,
178 MB with rust-analyzer attached and 178 MB with no language server
(plain `.txt`: 173 MB).

## Text rendering fallbacks

GPUI 0.2.2 on Linux ignores `Font::fallbacks`, cosmic-text only falls back
to faces of the run's exact style, and GPUI merges adjacent runs with equal
colors without comparing fonts. `arcode_ui_kit::apply_glyph_fallbacks`
splits runs so chars missing from the run's font use the first font of
`editor.font_fallbacks` that has them (or an upright style so the system
fallback can find a face), and keeps such runs distinct. The editor and the
terminal call it for every shaped line; ASCII lines skip it entirely.
`arcode_ui_kit::truncated` is a label element that truncates with an
ellipsis to its laid-out width (GPUI caches the first unconstrained text
measurement, so `text_ellipsis` does not work for shrinking labels).
