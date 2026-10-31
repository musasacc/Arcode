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
  signals "wakeup" (redraw needed) over a channel. Closing a terminal
  hangs up its child and, on Linux, what the child started in its process
  group (`hangup.rs`: stopped first so nothing forks past the hangup), as
  a real terminal hangup would; a child still running after a second is
  killed (`reaper.rs`).

## Diagnostics

Several servers may serve one file. Diagnostics are stored per
`(uri, server name)` and merged for display; each carries its source
(`diagnostic.source` or the server name). The editor draws a wavy underline
in the severity color, and hovering shows the message. The Problems panel
lists all of them; clicking one opens the file and moves the cursor there.

## Several language servers per file

`[language.<id>].servers` lists servers in priority order (e.g.
`["rust-analyzer", "typos"]`); each `[lsp.<name>]` has its command, args,
env, root markers, `initialization_options` and `features`, per-feature
switches (`diagnostics`, `completion`, `hover`, `signature_help`,
`definition`, `references`, `symbols`, `highlights`, `rename`,
`formatting`, `code_actions`, `inlay_hints`; all on by default).

- **Feature switches** (`arcode_lsp::Features`) are applied when a server
  is initialized: a feature that is off is removed from its capabilities,
  so every capability-routed request skips it, and its published
  diagnostics are dropped when `diagnostics` is off.
- A server listed twice for one language is used once (the first,
  higher-priority place; a warning is logged).
- **Merged**: diagnostics (labelled by source in hover, Problems panel and
  status bar counts), completion (`request_all`; the ranker drops an item
  of another server with the same label and inserted text, whatever kind
  it claims, and keeps all items of one server), code actions, references
  (a place two servers report is listed once) and workspace symbols.
  Completion opened by a trigger character only asks the servers that
  registered that character (a spell checker is not asked after `.`), and
  asking an incomplete list again goes to the same servers.
- **Single answers** (hover, definition / type definition /
  implementation, document symbols, rename, formatting, signature help,
  inlay hints, highlights) go through `LspManager::request_with`: the
  highest-priority server whose capabilities qualify is asked; when it
  answers "nothing" or fails, the next one is asked, and so on (a fallback
  server is only sent its request after the previous answer). "Nothing"
  (`Answer::is_null`) is `null`, and also an empty list where servers use
  it for "not found" (definition, symbols, inlay hints, highlights, no
  signatures); `[]` from a formatter means "already formatted" and is an
  answer. E.g. rust-analyzer answers hover on code, a spell checker on a
  word inside a comment.
- **Document sync**: each edit is converted once per position encoding the
  file's servers use (`change_document_multi`), so a UTF-8 and a UTF-16
  server both get incremental changes.
- **Live reload**: the bridge observes the settings global. Saving
  `config.toml` calls `LspManager::reconfigure`, which shuts down only the
  servers whose `ServerConfig` changed (any `[lsp.<name>]` key, feature
  switches included) or that no language uses any more, and makes each
  open document's servers match its language's list again (new servers
  get `didOpen` with the document's latest version and text, removed ones
  `didClose`). Workspace files are re-attached before library files that
  joined the workspace instance, so those stay on it. A replacement
  instance waits (up to 8 s) for the old one's shutdown before it starts,
  so their status events never interleave. Editors whose language gained
  or lost all servers are registered again. A `config.toml` that is not
  valid TOML keeps the current `[language.*]` / `[lsp.*]` settings
  (`Config::is_syntax_warning`, checked in `app::reload_settings`), so a
  typo does not restart every server.
- **Crashes**: when a server instance fails or stops, its diagnostics are
  removed (all of that server's when no other instance of it runs, else
  those of files under its root) instead of going stale.
- **Server processes**: on Unix each server leads its own process group,
  killed once the server exits or is killed and when its client or the
  manager is dropped (`client/group.rs`), so wrapper scripts and launchers
  never leave their real server or helpers behind. On quit the app also
  kills every group still listed (`arcode_lsp::kill_all_servers`), since
  servers stopping on a background thread would be cut off mid-shutdown.
  A signal that ends the app (Ctrl+C or a hangup of the terminal it was
  started from, `SIGTERM`) runs no quit callback and, with servers in
  their own groups, no longer reaches them, so the app installs a handler
  that kills the groups first (`arcode_lsp::kill_servers_on_signals`).
- **Status bar**: one entry per started server of the active file's
  language with its state (`lsp_bridge::status_view`). Clicking one opens
  a menu with the configured command, Restart Server
  (`LspManager::restart_server`; the file is reopened on the new
  instance) and Show Server Log, a read-only tab with the last 2000 lines
  of its stderr / `window/logMessage` and status changes; lines logged
  while it is open are appended (`LogFeed`, notified once per event
  batch). Clicking the entry again, Escape or a click elsewhere closes the
  menu. The palette has "LSP: Restart Language Servers" and "LSP: Show
  Language Server Log" for the active file.
- **Missing servers**: a server whose executable is not found shows "not
  installed" and a one-time toast with its configured command. Nothing is
  downloaded or installed.

`cargo build -p arcode-lsp --example fake_spell_server` builds a tiny
second server (flags the word `teh`, answers hover on it, offers a quick
fix and one completion item) to try this next to rust-analyzer.

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
  breakpoint lane also emits it for empty slots (`tag: None`). A right
  click emits `EditorEvent::GutterContextClicked { line, lane, tag }`
  under the same rules.
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
an editor). Everything is ranked with `arcode_completion::Ranker`, which
drops duplicates (same label and kind) except items of one server: those
can differ only in what they import, so all of them stay.

- **Never blocking.** Typing schedules a request after
  `editor.completion.delay_ms` from a task, so the bridge has already
  queued the keystroke's `didChange`. Each request has a sequence number;
  a newer one replaces the task (dropping the older one) and answers for
  anything but the latest are ignored. While a newer list loads, the old
  one stays shown and is re-filtered locally, so the popup does not
  flicker. Complete lists are only filtered as you type; incomplete ones
  are asked again (`TriggerForIncompleteCompletions`), remembering whether
  the list opened after a trigger character (then only server items are
  shown). An incomplete list filtered to nothing stays open but hidden
  (no box, keys go to the editor) until the next answer. A row the user
  picked with the arrows stays selected when a newer answer arrives. A
  complete answer with nothing for a word is not asked again while that
  word grows; a different word at the same place is.
- **Session.** A list is anchored at the start of the completed word (or
  file name, or snippet placeholder for choices). Any cursor move or edit
  re-filters it; leaving the word closes it. Several cursors or a
  selection disable it.
- **Accepting** applies the item's `textEdit` (insert or, with
  Shift+Enter, replace range, extended by what was typed since the answer)
  plus `additionalTextEdits` as one undo step; snippets go through the
  snippet session. The selected item is resolved once for its docs (a
  resolve in flight is not resent while typing). Items the server must
  resolve first get their
  additional edits applied when the resolve answer arrives, if they lie
  above the accepted line. `editor.action.triggerParameterHints` and
  `editor.action.triggerSuggest` commands are run by the editor
  (`editor.completion.signature_help = false` disables the first).
  rust-analyzer attaches its `rust-analyzer.triggerParameterHints` only
  when the client lists it in `experimental.commands`; the bridge maps it
  to the editor's name. Signature help also opens while a snippet
  placeholder (an argument) is selected.
- **Keys** come through the popover's `on_key`: up/down/page, Enter/Tab,
  Escape, and commit characters (accept, then type the character).
- **Popovers flip.** The editor measures each popover and opens it on the
  other side of its line when it does not fit (completion below, signature
  help above), and shifts it left to stay inside the window. A popover can
  have a side panel (the completion docs), placed right of it (left
  without room) and aligned with its edge nearest the line, so a tall
  panel never pushes a list that flipped above away from the cursor.

## Hover and navigation

`arcode_editor::hover` owns the hover popover; a `HoverProvider` (the
app's `lsp_bridge::hover`) answers `textDocument/hover` as plain
`HoverInfo` (documentation sections plus the symbol range).

- **Hover.** Diagnostics under the mouse show at once; when the mouse
  rests on an identifier for `editor.hover_delay_ms` (300 ms) the
  provider is asked and its documentation follows the diagnostics in the
  same popover (`editor.hover = false` turns the request off). One timer
  per rest, no polling: moving inside the same word keeps the request,
  leaving it cancels it (a sequence number drops late answers). The
  popover stays while the mouse is inside the symbol's range or inside
  the popover itself (its bounds are recorded in prepaint), so long
  documentation scrolls. It opens above the line in the lower part of the
  window. Ctrl+K Ctrl+I shows it for the cursor; that one stays until the
  cursor moves or Escape, and mouse motion never cancels its request.
  The popover is built and placed in the editor element's prepaint from
  that frame's layout (`window.defer_draw`, priority 1), so it follows
  its line: a mouse hover closes when the view scrolls, any hover when
  its line leaves the view, and when the window's focus moves to
  something else (a picker, the finder, the terminal, another editor),
  so it never covers an overlay. Moving inside one diagnostic does not
  redraw. Markdown is rendered by the completion docs
  renderer; fenced code is colored with tree-sitter by its fence tag (an
  untagged block uses the document's language), cached per block.
- **Definition link.** While Ctrl (Cmd) is held over an identifier the
  provider is asked for the definition and, when there is one, its origin
  range is underlined in the accent color with a pointer cursor.
  Secondary-click moves the cursor there and dispatches `GoToDefinition`.
- **Navigation commands** (`crates/arcode/src/navigation/`):
  `GoToDefinition` (F12), `GoToTypeDefinition`, `GoToImplementation`
  (Ctrl+F12) and `FindAllReferences` (Shift+F12) are editor actions the
  workspace handles. One target jumps; several open a picker grouped by
  file with preview lines (read on the background executor; open buffers
  win over the disk because the server answers for their unsaved text).
  Document symbols (Ctrl+Shift+O) use `textDocument/documentSymbol`,
  flattened with nesting depth, and fall back to the tree-sitter outline;
  workspace symbols (Ctrl+T) send the query to the server (debounced) and
  fall back to the outlines of the open files. Without a server, Go to
  Definition looks for an outline symbol named like the word.
- **Opening targets.** A target opens in the active pane, or activates
  the pane where it is open, and is centered. LSP ranges are converted
  against the buffer once it is open. A file outside the project (e.g.
  the Rust standard library) opens read-only: the editor then refuses
  every edit, completion and `save`. Before opening it the jump calls
  `LspManager::attach_to_workspace`, so the file joins the server already
  running (and not failed) for the workspace instead of starting one for
  its own root, and rust-analyzer keeps answering inside library
  sources. Other files outside the workspace (another project opened
  with File > Open) still get their own server.
- **History.** Every jump and every switch to another file records the
  previous cursor place; Alt+Left / Alt+Right (macOS Ctrl+- /
  Ctrl+Shift+-) walk it. Places a few lines apart in one file merge, and
  stepping through the history does not record new places.
- **Occurrences.** `textDocument/documentHighlight` is asked 250 ms after
  the cursor stops on an identifier (`editor.occurrence_highlight`) and
  drawn as background highlights (set `"occurrences"`, writes stronger).

## Code actions, rename, formatting and inlay hints

`arcode_editor::lang` owns the UI; a `LanguageServices` provider (the
app's `lsp_bridge::services`) answers in buffer `Point`s and applies what
may touch other files itself.

- **Inlay hints** are asked for the visible lines plus a page above and
  below and drawn as inline virtual text (set `"inlay-hints"`). They follow
  edits until the answer for the new text arrives (no flicker), and are
  asked again 400 ms after the last edit, when scrolling leaves the fetched
  lines, and at the next layout after `workspace/inlayHint/refresh` or a
  server becoming idle (it may have answered nothing while indexing); a
  refresh only marks the hints stale, so background tabs ask when they are
  shown again. Nothing is asked
  while `editor.inlay_hints` is off or "Editor: Toggle Inlay Hints" turned
  them off for the session (a global every editor checks on render).
- **Code actions.** 250 ms after the cursor rests the actions for the
  selection are asked (with the server's own diagnostics touching it as
  context) and a light bulb replaces the diagnostic marker of the cursor
  line (warning color when a quick fix is among them;
  `editor.code_action_lightbulb`). The answer is cached per buffer version
  and range, so Ctrl+. (Cmd+.) or a click on the bulb opens the menu at
  once; "Refactor..." (Ctrl+Shift+R, context menu) lists only `refactor.*`.
  The menu is a popover grouped by kind (Quick Fix, Extract, Inline,
  Rewrite, Refactor, Source Action), preferred fixes first, disabled
  actions greyed with their reason. Picking one resolves it
  (`codeAction/resolve`) when it has no edit, applies its edit, then runs
  its command (`workspace/executeCommand`). Menus close on a click, an
  edit, a key that is not theirs (it reaches the editor) and focus loss.
- **Rename** (F2) asks `prepareRename`, shows an inline field at the
  symbol with the name selected (keys go to the field through the
  popover's `on_key`; the editor keeps focus) and sends `rename` on Enter.
  Errors (e.g. "No references found") show as a notice at the cursor.
- **Workspace edits** (code actions, renames, the server's
  `workspace/applyEdit`, which is answered only after the workspace
  applied it) are planned by `lsp_bridge::workspace_edit` and applied by
  `crate::refactor`: one undo step per file, even when several
  `TextDocumentEdit`s touch it (`Editor::begin_edit_group` /
  `end_edit_group`); files that are not open are opened in background
  tabs and left unsaved (LSP has no "save" request in an edit, so nothing
  is written behind the user's back); the active tab stays active and the
  focus stays where it was. Create / rename / delete run on disk and open
  editors follow a rename (`Editor::set_path`,
  `LspBridge::editor_path_changed`); a case-only rename (`foo.rs` ->
  `Foo.rs`) works on case-insensitive filesystems. Since the disk then
  already shows part of the edit, an edit with resource operations also
  saves the files it changed that had no unsaved changes before (like VS
  Code's `files.refactoring.autoSave`); undo reverts text, not the
  resource operations. A create that overwrites an open file empties its
  editor too. A document whose version differs from the edit's, or a
  delete of a file with unsaved changes, refuses the whole edit before
  anything changes; later failures stop the rest (`abort`). Failures of
  the editor's own code actions and renames show as the editor's notice;
  a server's `workspace/applyEdit` failure shows as a toast.
- **Formatting**: Shift+Alt+F (document), Ctrl+K Ctrl+F (selection or the
  cursor line). Edits are trimmed to the text that really changes and
  applied as one undo step; a cursor inside a rewritten range keeps its
  line and column. Format on save (`editor.format_on_save`) waits at most
  `editor.format_on_save_timeout_ms` (1500 ms) and then saves whatever the
  buffer holds, so saving never hangs; edits arriving for an older version
  are dropped. The pending save has its own task, so formatting by hand
  meanwhile does not cancel it.
- **Context menu** (right click, Shift+F10): Go to Definition, Find All
  References, Rename Symbol, Refactor..., Format Document / Selection,
  Cut, Copy, Paste, with their key bindings; entries dispatch the same
  actions as the keys.

## Vim mode and multiple cursors

`arcode_editor::vim` connects the `arcode-vim` state machine to the editor
through the key-interceptor extension point; the editor's own bindings are
unchanged.

- **Switching**: `editor.vim_mode` (or the older `editor.keymap = "vim"`),
  and "Editor: Toggle Vim Mode" in the palette, which overrides the
  setting for the session (`VimModeToggle`). The toggle is an app-wide
  action (`cx.on_action` in `arcode_editor::init`), so it works whatever
  has focus. Every editor follows on its next render: it installs (or
  removes) one interceptor at priority 0.
- **Key routing**: popover handlers run first, so the completion list,
  signature help, the rename field and menus keep their keys; Escape
  closes the completion list before it leaves Insert mode. Keys Vim never
  sees: Cmd (macOS), Ctrl+Shift combinations, function keys, Ctrl+P (the
  file finder) and the second key of a pending chord (Ctrl+K Ctrl+I).
  Other Ctrl keys are Vim's in Normal mode (Ctrl+D/U/F/B scroll, Ctrl+V
  block, Ctrl+R redo); a key Vim passes through runs the editor binding,
  except that plain keys never type text outside Insert mode. In Insert
  mode Vim inserts typed text itself (so `.` and counts repeat it) and the
  editor then runs its completion trigger; Tab goes to an active snippet.
- **Edits**: Vim edits the buffer directly and logs each edit
  (`Vim::take_edits`); the editor forwards them like its own (syntax,
  diagnostics, decorations, LSP `didChange`). Undo and redo (`u`,
  `Ctrl-R`, `.`) use the buffer's history, one step per command or per
  Insert session. Edits made outside Vim (completion, formatting) keep
  Vim's marks in place (`on_external_edits`). In a read-only editor
  (`Vim::set_read_only`, synced before each key) Vim refuses every change
  before it touches the buffer: operators other than `y`, Insert and
  Replace mode, `x`, `p`, `u`, `Ctrl-R`, `.`, `:s`, and each change inside
  a replayed macro give "E21: Cannot make changes, the buffer is
  read-only". Motions, yanks, searches and Visual mode work.
- **Shared state**: registers, macros and the last search live in one
  `VimShared` global, swapped into the focused editor's `Vim` around each
  key, so a yank in one file pastes in another and `n` repeats a search
  started elsewhere. `"+` / `"*` read and write the system clipboard.
- **Look**: block cursor in Normal and Visual modes (drawn on Vim's
  cursor char, not the selection end), bar in Insert, underline in
  Replace and operator-pending. The status bar shows a mode pill (accent
  NORMAL, green INSERT, amber VISUAL, red REPLACE), the pending keys
  (`2d`), the command line being typed (`:%s/a/b`, `/foo`), `recording
  @q` and Vim's last message (errors in red). The editor emits
  `EditorEvent::VimStatusChanged` only when that changes.
- **Search**: `/ ? * # n N` matches are highlighted (set `"vim-search"`)
  until `:noh`. Only the lines around the viewport are searched (200
  lines above and below, at most 1 M chars), again when the view scrolls
  out of that window (checked after each layout), the search changes
  (also in another editor: the search is shared) or Vim is back in Normal
  mode after edits.
- **Ex commands**: `:w` (format on save applies; the message `"x"
  written` or `E212: ...` appears when the save has finished), `:{n}`,
  `:s` / `:%s`
  and `:noh` run in the editor. `:q` (refused with E37 while modified),
  `:q!`, `:wq` / `:x` (save, then close) and `:e <file>` (relative to the
  project root, `~/` is home) go to the application's
  `VimRequestHandler` (`crate::vim_glue` in the app), which closes the
  tab (`Pane::close_discarding` for `:q!`) or opens the file.

Multi-cursor commands outside Vim (`commands/multi_cursor.rs`):

- Ctrl+D adds the next occurrence; Ctrl+Shift+L selects every occurrence
  of the selection (or of the word at the cursor, whole words only).
- Ctrl+Alt+Up / Down (Cmd+Alt on macOS; Alt+Shift+Up / Down also) add a
  cursor above / below; Alt+click adds a cursor, or removes the one
  clicked; Escape goes back to one cursor.
- Alt+Shift+drag or a middle-button drag makes a box selection, one
  selection per line between the two x positions of the box, mapped to a
  char on each line through its shaped text (so wide CJK glyphs, tabs and
  inlay hints line up with what is drawn; lines out of view use the
  char-width grid). Lines that end left of the box are skipped unless the
  box has no width; then every line gets a cursor.
- Ctrl+U (Cmd+U) goes back to the cursors before the last cursor change
  (up to 64 states); an edit clears that history.
- Typing, deleting and pasting act at every cursor; a paste with as many
  lines as cursors gives each cursor one line, also for whole lines
  copied from empty selections (cursors sharing a line get their lines
  above it, in cursor order).

## Git

`arcode-git` wraps the `git` CLI with network access disabled; the app
never fetches, pulls or pushes. In the app (`crates/arcode/src/git/`):

- `GitStore` (one per workspace) finds the repositories of the project:
  the one containing the root plus repositories in sub-folders up to three
  levels deep (`discover_repos`). It keeps the status of the active one
  (the Source Control view follows the active editor's repository) and
  runs every operation on the background executor (`GitStore::run`).
  Nothing is created on open: a folder without git shows an
  "Initialize Repository" button that runs `git init` only when clicked,
  and only once discovery has finished (never inside an existing
  repository). A status refresh never publishes the result for a
  repository that is no longer the active one. When git cannot read the
  status (git missing, a broken repository) the view shows the reason
  instead of "No changes". Group "Stage all" stages exactly the listed
  files (`stage_paths`), never `git add -A`/`-u`, so conflicts stay
  unresolved; commits are guarded against a double Ctrl+Enter.
- Watching is event driven (`notify`: inotify, FSEvents,
  ReadDirectoryChangesW). On Linux each non-ignored folder gets its own
  watch so `target/` and `node_modules/` cost nothing; elsewhere the root
  is watched recursively. The folder walk runs on the watcher's own
  thread, never on the main thread. Changes under a git directory (index,
  `HEAD`, refs, merge state) re-run status and make editors recompute
  their gutter and blame; worktree changes re-run status; a `.git`
  appearing or vanishing (`git init`/`git clone` in a terminal)
  re-discovers the repositories. Without any repository only the top
  three folder levels are watched, for that purpose. At most one message
  per kind is queued, so worktree churn cannot crowd out an index change,
  and events are coalesced for 250 ms. Our own reads never write the
  index (`GIT_OPTIONAL_LOCKS=0`), so they cannot trigger themselves.
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
  conflict commands. Conflict actions check the buffer version: a lens
  clicked before the debounced re-parse acts on the conflict that starts
  on that line now, and lenses are re-parsed right after a resolution.
  Blame is cached per buffer version; a dirty buffer is blamed with its
  own line ending (CRLF files stay attributed). The blame column pads by
  the glyphs' real advances (`arcode_ui_kit::glyph_advance`, CJK in
  fallback fonts), so the code lines up for any author name.
- Diff tabs are read-only editors without a path (so they can never be
  saved over a file) showing an inline diff (`inline_diff`), with a custom
  tab label kept by the pane (`TabLabel`: title, key, tooltip). File diffs
  are keyed by path, so same-named files get separate tabs.

## Search, command palette and TODO list

All searching goes through `arcode-search` on the background executor; the
UI never walks the project or scans a file on the main thread.

- **Find bar** (`crates/arcode/src/find_bar/`, Ctrl+F / Ctrl+H, Cmd+Alt+F
  on macOS): owned by the pane and shown above its active editor, following
  tab switches. Each search snapshots the rope and runs
  `arcode_search::search_text` in the background (at most 20 000 matches);
  matches are drawn through the background-highlight extension point (sets
  `"find"` and `"find-current"`) and searched again 150 ms after an edit.
  Replace and Replace All are `apply_edits` calls (Replace All is one undo
  step); regex groups expand per match with `arcode_search::LineReplacer`
  (the query compiled once). Replace All (`find_bar::bulk`) hands the editor
  at most 256 edits: beyond that, neighbouring matched lines are merged into
  whole-line edits, so 200 000 matches stay one fast edit batch. Next and
  previous only step through matches of the current text: when they are
  stale (edit, other tab, F3 on a closed bar) the bar searches first and
  steps when the result is in. A hidden bar keeps its query (F3 reopens it)
  but neither searches nor highlights.
- **Search view** (`crates/arcode/src/search_view/`, Ctrl+Shift+F,
  Ctrl+Shift+H for replace): typing re-runs the search after
  `search.debounce_ms`; a new run cancels the old one through its
  `CancelToken`. Results stream over a channel and are added in batches
  (40 ms), sorted by path. The run stops at `search.max_results` ("Show
  more" raises the limit). Include and exclude globs come from the view,
  `search.exclude` and the folders the file tree skips (`target`,
  `node_modules`). A file open with unsaved changes is shown with the
  matches of its buffer; after the disk search, unsaved buffers it did not
  list are searched too (`SearchQuery::includes_path` applies the globs).
  Replace All first counts every match (not only those listed) and asks for
  confirmation with the counts; exactly the counted query and replacement
  are applied, whatever is typed meanwhile. Open files are changed in their
  editors (undoable, left unsaved), others are rewritten with
  `replace_in_file`, which skips non-UTF-8 and binary files; the reasons are
  shown in the view. Enter in the replace or glob fields searches again
  (it does not open a result); Ctrl+Alt+Enter in the replace field is
  Replace All.
- **Command palette** (`crates/arcode/src/commands/`, Ctrl+Shift+P, F1, or
  `>` in the file finder; Backspace on an empty query goes back to files):
  `CommandRegistry` is a global of commands (category, name, action), built
  from `actions::commands()` and `actions::editor_commands()`; features add
  theirs with `commands::register`. When the palette opens it lists only the
  commands available at the current focus (`Window::is_action_available`)
  with the binding that applies there, and the chosen command is dispatched
  back on that focus. Ranking uses nucleo; recently used commands
  (`recent_commands` in `state.toml`) come first when they match well.
- **TODO list** (`crates/arcode/src/todo/`): one `TodoStore` per workspace
  (tags from `todo.tags`) feeds the TODO activity and the Todo tab of the
  bottom panel. The project is scanned once; after that only changed paths
  are scanned again (`scan_todos_in_paths`, same ignore rules): files saved
  in Arcode, and paths the project watcher reports. The git watcher records
  changed working-tree paths in a shared set and `GitStore` emits them as
  `GitEvent::FilesChanged` after its 250 ms debounce (more than 1000 paths,
  or 500 pending, fall back to a full rescan). Gitignored paths (root and
  repository `.gitignore`, `info/exclude`) are left out of that set, so a
  build writing to `dist/` does not cause rescans where the root is watched
  recursively. Changes that arrive while a full scan runs are held and
  scanned after its result is applied, so a stale full-scan read never
  overwrites a newer incremental one.
- **Workspace subscriptions** (`workspace/subs.rs`): overlays (finder,
  palette, prompts) keep one subscription each, replaced when they open
  again; editor and pane subscriptions are dropped with their entity. The
  finder and the palette never stack: Ctrl+P in the palette switches to
  the finder.

## Windows, sessions and the file tree

- **Windows** (`crates/arcode/src/windows/`): every window has its own
  `Workspace`, so its own project, `LspBridge` (language servers),
  terminals, git store, TODO store and session. `open_window` places a new
  window where its project's window was last (else where the last window
  was), shifted when another window is at the same spot. Opening a project
  that another window shows brings that window forward instead. Actions
  that open windows or quit run deferred (`cx.defer`): an action runs
  while its window is updated, and an updated window cannot be read.
  A saved placement is kept on the display that holds its top-left corner
  (else the primary display). On X11 (`windows/x11.rs`) the new window is
  found by our process id and its title (`_NET_WM_NAME`, else `WM_NAME`);
  when several of our windows share the title, the newest X id wins.
- **Sessions** (`workspace/session/`, stored in `state.toml` as
  `AppState::sessions`, see `arcode_config::ProjectSession`): panes with
  their files and active tab, the active pane, selections (as line /
  column points, primary first) and the first visible line per file, the
  sidebar view and width, the bottom panel (shown, height, tab), the
  expanded tree folders and the window placement. A session is captured
  when the project is switched away from or closed, when its window
  closes, on quit, and 2 s after a file is opened or a file is renamed or
  deleted (one timer, replaced on each change; nothing polls). Only files
  inside the project are stored (diff tabs and library sources are not),
  never unsaved text. Restoring skips files that no longer exist; a tab
  that was not painted keeps its restored scroll line for the next
  capture. A file keeps its cursor and scroll after its tab closes
  (`PaneEvent::ItemClosed`): the state is applied when the file is opened
  again, in this run or after a restart, follows renames and is dropped
  when the file is deleted. Sessions of projects that are neither recent
  nor open are dropped beyond 40; file states beyond 200 per project, the
  least recently open first (`FileSession::seen`). The delayed save keeps
  the project's stored window placement (only quitting or closing the
  window records a new one). `open_projects` lists the projects of the
  windows open at quit (closing the last window counts as quitting) and
  they are reopened at start when no path is given. The list is reset at
  start and a project is added as soon as a window opens it, so after a
  crash the projects of that run are reopened.
- **Writing `state.toml`**: `app_state::save` writes synchronously (quit,
  window close, project close); frequent saves (the delayed session save,
  recent projects and commands) use `app_state::save_in_background`, which
  serializes on the main thread and writes and fsyncs on the background
  executor. Writes carry a generation number and are serialized by a
  mutex, so an older text never replaces a newer one.
- **Closing guard** (`workspace/close_guard.rs`): switching or closing the
  project, closing the window and quitting end the terminals, so besides
  unsaved editors they ask when a terminal runs a command.
  `arcode_project::procs::is_running_command` checks each shell on the
  background executor: on Linux the terminal's foreground process group
  in `/proc/<pid>/stat` differs from the shell's, on other Unix systems
  the same through `ps`, on Windows the shell has child processes
  (PowerShell, unverified).
- **File tree** (`sidebar/file_tree/`): the root and each expanded folder
  get a non-recursive `notify` watch (collapsed folders cost nothing);
  events are coalesced for 150 ms and only the affected cached listings
  are read again, on the background executor, redrawing only when one
  changed. Only the root and expanded folders keep a cached listing, so a
  folder expanded again is read afresh and shows changes made while it
  was collapsed. The expanded folders of a restored session are read on
  the background executor and shown together. Before a folder is renamed,
  moved or deleted its watches (and those inside it) are dropped, since
  an open watch handle can block that on Windows. Paths handed to other
  programs (Explorer, PowerShell, the terminal's working directory) go
  through `files::external_path`, which drops the Windows `\\?\` prefix
  of canonical paths. A rename that only changes letter case is allowed
  only when the folder has no entry spelled exactly like the new name (so
  on a case-sensitive file system `A.txt` is never replaced by `a.txt`). File operations (`arcode_project::files`) run off the main
  thread where they can block: delete goes to the trash (FreeDesktop home
  trash written directly, else `gio trash`; Finder through `osascript`;
  the Recycle Bin through PowerShell) and only offers a permanent delete,
  confirmed separately, when no trash is available. Renames and moves
  update the paths of open editors; deleting closes the tabs of deleted
  files that have no unsaved changes.
- **New Project** (`new_project.rs`, `arcode_project::templates`): creates
  the folder; a template only yields a command line, which the workspace
  types into the new project's terminal after the user clicked Create.

## Debugger and run buttons

User-facing behaviour and the `.arcode/launch.toml` format are in
`docs/DEBUGGING.md`. The pieces:

- **Libraries.** `arcode_dap::detect_adapter` / `locate_adapter` find an
  installed adapter (versioned `lldb-dap-N` names, GDB 14+ via
  `gdb --version`, `import debugpy`; these spawn short processes, so they
  run on the background executor) and `missing_adapter_message` says
  what to install. `arcode_dap::parse_launch_file` parses launch.toml
  leniently (a broken entry becomes a warning). `arcode_project::debug_target`
  turns a run command into a build (`cargo ... --message-format=json`,
  `test`/`bench` get `--no-run`), a program, a Python module or a Delve
  mode; `executables_from_cargo_json` reads the built executable.
- **`Debugger`** (`crates/arcode/src/debugger/`, one entity per
  workspace) owns the `BreakpointBook`, the launch choices (loaded in the
  background, reloaded when launch.toml or a package file is saved or
  changes on disk, from the project watcher), the
  decorations of every registered editor and at most one `Session`.
  Preparing (adapter lookup, build) runs on the background executor; its
  output streams into the console through a channel. The build runs in
  its own process group (`arcode_project::proc_group`), so Stop, a new
  start or dropping the `Debugger` (closing the window) kills it with
  everything it started. The breakpoints are taken from the book when the
  session starts, after the build. The adapter's
  `EventReceiver` is drained by a GPUI task (all queued events in one
  update); nothing polls.
- **Editors.** Breakpoints are gutter dots in the set
  `debugger-breakpoints` (tag = breakpoint id). They follow edits on
  their own (`debugger/line_follow.rs`), not by reading the dots back:
  the debugger keeps each breakpoint line's text (whitespace ignored) and
  maps lines through every `Edited` edit; a line inside a multi-line
  replacement (a line move, a whole-document format, a paste) goes to the
  closest inserted line with the same text, and a breakpoint is removed
  only when its text is gone. The stopped arrow follows edits the same
  way. The stopped line is the set `debugger-stopped` (an
  arrow in the breakpoint lane replacing a dot on that line, plus a line
  highlight). Run buttons (`debugger-run`) are recomputed 400 ms after
  typing from a rope snapshot parsed on the background executor (not for
  large files). Right clicks in the gutter arrive as
  `EditorEvent::GutterContextClicked`.
- **Sessions.** Each stop bumps a generation; answers (threads, stack,
  scopes, variables, watches) for an older stop are dropped. Variables
  load lazily per reference; open nodes are remembered by name path so
  they reload after a step. Breakpoint changes during a session are sent
  per file at once; adapter breakpoint ids are mapped to ours from the
  `setBreakpoints` answers so later `breakpoint` events update the dots
  (the startup answer is matched with the ids that were sent at start;
  a file sent again since is answered by its own request). Only one
  continue/step request is in flight: further steps are dropped until the
  adapter reports the program running. With GDB, REPL input that GDB
  rejects as a command is evaluated as an expression; GDB gets
  `LC_CTYPE=C.UTF-8` when the environment has no UTF-8 locale.
  Restart stops and starts the configuration again (rebuilding it).
- **Persistence.** `ProjectSession.breakpoints` and `run_command` live in
  `state.toml`; breakpoints are read on project open even when
  `workspace.restore_session` is off.
- **Terminals.** Run commands and `runInTerminal` requests run in
  terminal tabs with a fixed name (`TerminalPanel::run_in_named`); an
  idle tab of the same name is reused. Whether a tab is busy is checked
  on the background executor (it can spawn `ps` on macOS), so the command
  starts a moment later and `runInTerminal` is answered from a callback;
  busy is checked on Unix only (on Windows a new tab is used).

## Language packs, file associations and the package banner

A **language pack** bundles what Arcode knows about a language. The parts
live in the UI-agnostic crates and are joined in `arcode::languages::bundle`:

- grammar, highlight / outline / runnables queries, file extensions and
  names, comment tokens: `arcode-syntax` (`Language`);
- default language servers: `arcode-config` (`builtin_languages`,
  `builtin_servers`; every language with a grammar now has one);
- the tools to install (server, debugger, formatter, linter), how to check
  them and the install steps per OS, the executables each provides
  (`Component::provides`, e.g. `pyright` provides `pyright-langserver`),
  and typical run commands: `arcode-project` (`packs()`);
- built-in snippets: `arcode-completion`.

The Languages page (Plugins activity, or "Languages: Show Language
Packs") shows one card per pack: its languages, extensions, snippets,
each configured server with "installed" / "not installed" (a `PATH`
lookup of the configured command, done in the background when the page
is first shown or Refresh is clicked; nothing is run) and, when missing,
the install command or instructions as text with a copy button. Nothing
is downloaded. The pack's switch writes `[language.<id>] enabled = false`
(or removes the key) for each of its languages with a comment-preserving
`toml_edit` edit (`arcode_config::config::edit`) and reloads the settings.
A disabled language keeps its configured servers in
`LanguageSettings::disabled_servers`, so `servers` is empty and the LSP
bridge's live reload closes its documents and stops servers nothing else
uses; its run buttons are hidden; highlighting stays.

**File associations**: `[files] associations = { "*.mdx" = "markdown" }`
maps file-name (or, with a `/`, path) patterns to a language id or name;
the most specific match wins (`FilesSettings::association_for`). The
workspace applies it when a file opens (before the language servers see
it), after a rename, and to open editors when the settings change. A
language picked by hand in the status bar sticks for that editor (also
across renames) until "Auto Detect" is picked. Changing an editor's
language goes through `Workspace::set_editor_language`: the LSP bridge
re-registers it (`LspBridge::editor_language_changed`: the document is
closed, the diagnostics stored for its path are dropped and the new
language's servers open it) and the debugger finds its run buttons again
(`Debugger::language_changed`). An editor only shows diagnostics from the
servers of its own language (`push_diagnostics`), so a `.rs` file switched
to Plain Text shows no squiggles even when rust-analyzer, which still
compiles it, reports it again; such a report still counts in the status
bar and Problems as a project problem. Run buttons are dropped whenever a
file stops qualifying (language pack off, another language, run buttons
off, outside the project).

**Status bar**: indentation (`Spaces: 4` / `Tab Size: 4`), encoding
(`UTF-8`, `UTF-8 with BOM`; display only), line ending (`LF` / `CRLF`) and
the language. Clicks open a filterable `ListPicker`: the language picker
also offers "Configure File Association for '*.ext'…", which writes the
association to `config.toml`; the line ending picker calls
`Editor::set_line_ending` (dirty until saved); the indentation picker sets
`Editor::set_indent_override` (spaces 2/4/8, tabs 2/4/8), detects it again,
goes back to the language settings, or converts leading whitespace (one
undo step). With `editor.detect_indentation` (default on) each opened file
gets the indentation its first 10 000 lines use when they clearly differ
from the settings (`languages::indent::detect`).

**Package banner**: `arcode_project::missing_packages(root, opened)` looks
at the manifests in the project root and the nearest manifest of each open
file, with file reads and `stat` calls only: `package.json` with
dependencies and no `node_modules` (or Yarn PnP), Python manifests with
dependencies and no usable virtualenv (Poetry / Pipenv projects, whose
environments live elsewhere, are not reported), `go.mod` requirements
missing from the module cache (`GOMODCACHE`, `GOPATH/pkg/mod` or
`~/go/pkg/mod`; a `vendor/modules.txt` counts as installed), a `Gemfile`
without `Gemfile.lock`, `composer.json` without `vendor/`. A Cargo project
is never reported. A `go.mod` `replace` is applied: a module replaced by
another module is looked up under the replacement's path and version, one
replaced by a directory (any target without a version, including Windows
paths) is skipped. The check runs in the background 300 ms after a project
or file opens and after a manifest, lockfile or install folder changes on
disk. Install folders (`node_modules`, `.venv`, `venv`, `env`, `vendor`,
`.bundle`, `.pnp.cjs`, `.pnp.js`: `git::watcher::INSTALL_MARKERS`) are
nearly always gitignored or skipped, so the file watcher reports them as an
exception: the folder appearing or going away is reported (never its
contents), and such a change checks once more 4 s later in case the
folder was not complete yet (a virtualenv without its interpreter). "Install packages" runs each manifest's command (`npm install`,
`pnpm install`, `uv sync`, `bundle install`, `go mod download`, ...; a pip
install without a virtualenv first runs `python3 -m venv .venv` and then
installs into it) in a terminal tab named after it; "Dismiss" hides the
banner for the session; "Don't ask again for this project" is stored in
`state.toml` (`hidden_package_banners`). `workspace.package_banner = false`
turns it off everywhere; "Packages: Check for Missing Packages" checks on
demand; "Packages: Install Missing Packages" works without a banner too
(dismissed, hidden for the project, or turned off): it checks first, in the
background, and installs what is missing.

### Packages view

`arcode_project::deps` reads every manifest under the project (breadth
first, at most 6 levels, skipping `node_modules`, `target`, `vendor`,
virtualenvs and hidden folders) with its lockfile, offline: `Cargo.toml` +
`Cargo.lock` (workspace inheritance, renamed and target-specific
dependencies), `package.json` + `package-lock.json` / `yarn.lock` (classic
and berry) / `pnpm-lock.yaml` (a small YAML subset reader) / `bun.lock`,
`pyproject.toml` (PEP 621, PEP 735 groups, uv and Poetry tables) + `uv.lock`
/ `poetry.lock`, `Pipfile` + `Pipfile.lock`, `requirements*.txt` (versions
from the project virtualenv's `*.dist-info`), `go.mod`, `composer.json` +
`composer.lock`, `Gemfile` + `Gemfile.lock`. Parsers never panic; a
malformed file shows its error under its section. The scan runs in the
background the first time the view is shown (never before) and again 400 ms
after a manifest or lockfile changes on disk.

Add, Remove, Update and Update all build the manager's own command as an
argument list (`cargo add`, `npm install`, `pnpm add`, `yarn add` /
`upgrade` / `up`, `bun add`, `uv add` / `lock --upgrade-package`, `poetry
add`, `pipenv install`, `<venv python> -m pip install`, `go get`, `composer
require`, `bundle add`, ...) after checking names (no leading `-`, only
package-name characters) and versions; on Windows npm, pnpm and Yarn run
through their `.cmd` shims (Composer and Bundler through `.bat`), where
`cmd.exe`-special characters are refused. The command runs in a terminal tab
started directly on the program (`TerminalPanel::run_program`, no shell),
in the manifest's folder; the tab stays after the program exits and the
view is read again then. A missing program gives a toast with how to
install it. "Check for updates" is the only network use and runs only on
click, in the background, cancellable: `cargo update --dry-run --verbose`,
`uv lock --upgrade --dry-run`, `npm outdated --json`, `pnpm outdated
--format json`, `yarn outdated --json`, `pip list --outdated --format json`
(the virtualenv's interpreter; `pipenv run` for Pipenv), `poetry show
--outdated`, `go list -m -u -json all`, `composer outdated --format=json`,
`bundle outdated --parseable`. Output that cannot be read is written to a
temporary file that a terminal tab prints (`cat`, `findstr` on Windows), so
the check does not run twice; a manager without such a command (Bun) runs
its own `outdated` in a terminal tab. A finished command's tab is labelled
"(done)", "(exit N)" or "(stopped)" (`TerminalView::exit_code`).

A Cargo virtual manifest (`[workspace]` without `[package]`) offers no Add…
or Remove (cargo cannot tell which member to change), and
`[workspace.dependencies]` entries cannot be removed with cargo either;
Update works everywhere. Removing a target-specific dependency passes
`--target <cfg>`. The view keys the removal confirmation and the context
menu by (manifest, name, kind, group), so a name declared twice (`tokio` in
`[dependencies]` and `[dev-dependencies]`) removes the right entry, and a
rescan closes a menu whose row now shows another dependency. Python names
from a check match the manifest's PEP 503-normalized. Files are read only
when they are regular files of bounded size (a FIFO or a symlink to a
device in a repository cannot block or exhaust the scan).

## Settings, themes, first run, tour and low-end mode

Settings live only in `config.toml`. The Settings page
(`crates/arcode/src/settings_ui`, a page tab of the pane: `pane/page.rs`
`PageTab`) and every picker write through `prefs::store::write_settings`
(the language picker and the Languages view use `store::edit_config`, and a
save of `config.toml` in an editor `store::reload`). All of them wait in one
FIFO queue drained by a single UI task: each job edits the file in place on
the background executor with `arcode_config::apply_edits` (toml_edit: a
replaced value keeps its trailing comment, a new key is appended to its
table, a missing file starts from the commented template; a symlinked
`config.toml` keeps its link and the target is replaced), loads it again
(`Config::load_from`), and applies the result on the UI thread
(`app::apply_settings`) before the next job starts, so the last change wins
in the file and on screen. A file with a TOML syntax error is left alone and
the error is shown as a toast. Steppers step from `store::pending_value` (the
last value queued for their key), so quick clicks all count. The page
observes the `Settings` global, so it redraws on any change. Below 560 px
(a split pane) the page shows an icon-only section nav and rows wrap their
control below the label; the width is measured by a `canvas` during
prepaint and a change redraws the page once.

Live previews never touch the file. The theme picker (`prefs::pickers`,
"Preferences: Select Theme", Ctrl+K Ctrl+T) re-reads the themes folder in the
background when it opens, previews the highlighted row
(`ListPickerEvent::Highlighted`, emitted on arrows, hover and filtering)
with `theme_sync::preview`, and Escape restores the configured theme with
`theme_sync::apply`. Opening another picker, the palette, the finder, the
first-run dialog or the tour closes the picker and undoes its preview first.
The theme cards of the Appearance section preview on hover the same way;
the page remembers which card started the preview (only that card's
hover-out ends it), and the workspace ends it when the Settings tab stops
being shown (`end_hidden_settings_preview` on `PaneEvent::ActiveItemChanged`). The font pickers list `TextSystem::all_font_names`
(queried on the background executor) through `arcode_ui_kit::font_choices`
(bundled JetBrains Mono first, hidden `.`-names dropped) and preview by
installing a modified copy of the settings; dismissing puts the saved copy
back. The font preview of the Fonts section uses the editor's font family,
size, line height, fallbacks and `calt`/`liga` features
(`settings_ui::fonts::ligature_features`).

"Follow system" uses `ui.dark_theme` / `ui.light_theme` and switches when
GPUI reports a new window appearance (`observe_window_appearance` in the
workspace).

First run: when `config.toml` does not exist at startup (and
`ARCODE_SKIP_FIRST_RUN` is unset or `0`), `app::run` sets the
`FirstRunPending` global and the first workspace shows
`prefs::first_run::FirstRunDialog` (theme cards with live preview, editor
font among the bundled and installed monospace fonts, keymap, performance
mode). "Save and Continue" (Enter) writes the template with
`FirstRunChoices::edits` applied, then offers the tour; "Skip" (Escape)
writes the template unchanged, so the dialog shows once. If the file cannot
be written, Save shows the error in the dialog and Skip still closes it
(with a toast). Without any configuration folder the dialog is not shown.
"Help: Show Welcome Setup" opens it again on an existing file, starting from
the current settings (`FirstRunChoices::from_config`). The dialog and the
tour are modal: while one is up the root view's key context is
`WorkspaceModal` instead of `Workspace`, so no workspace shortcut acts
behind it.

Tour ("Help: Start Tour"): the title bar items, the activity bar, the bottom
panel and the status bar each carry an invisible `canvas` child
(`prefs::tour::Anchors::recorder`) that records their bounds during
prepaint. The overlay (`workspace/tour.rs`) draws four dimmed rectangles
around the target and an accent ring on it, and places the callout with
`tour::callout_origin` (below, else above, else beside, clamped to the
window). A step whose element is not drawn (no folder open) shows a centered
callout. While a tour runs, a change of recorded bounds (a resize) asks for
one more frame, so the callout follows; otherwise recording never asks for
frames. The terminal step shows the bottom panel and the tour hides it again
at the end if it opened it. Arrows / Enter / Escape work in the overlay's
`Tour` key context.

Low-end mode (`ui.low_end_mode`; the Settings switch, "Preferences: Toggle
Low-End Mode" and the first-run choice write `arcode_config::low_end_edits`):
`animation_duration` is `None`, so nothing animates; the cursor never
blinks (forced when the file is loaded); inlay hints and inline blame are off
unless the file sets them; popups use `Elevated::elevated` and draw no
shadow; `arcode_ui_kit::debounce` triples the git debounces (watcher, hunks,
blame column) and the Problems list and status bar counts wait 450 ms
instead of updating on every diagnostics push. Turning it on sets
`editor.inlay_hints` / `git.inline_blame` keys that are `true` to `false`,
tagged with the comment `[low-end mode]` (`arcode_config::Guard`); turning
it off restores only tagged values, so a setting the user turned off stays
off. The Keymap choice writes `keymap_edits`: Default also turns off
`editor.vim_mode`.

Animations (when `animation_duration` is `Some`): the sidebar and the bottom
panel slide open and closed (`workspace/anim.rs`: a clipping wrapper whose
width or height animates; a closing panel stays drawn until a timer of the
same duration drops the wrapper), popups, pickers, dialogs and toasts fade
in (`overlays::fade_in`). GPUI's `with_animation` requests frames only while
it runs.

Idle drawing: the workspace is the root view, so each drawn frame renders it
once; `prefs::frames` counts them and `ARCODE_LOG_FRAMES=1` prints the count
when it changed during the last second. Measured on X11/Xvfb: an idle window
with an open Rust file, rust-analyzer ready and the memory sampler on draws
no frame over 30 s; a sidebar toggle draws about 6 frames with animations
and 1 to 2 in low-end mode. The memory sampler reads `/proc` on the
background executor every 2 s, redraws only when the shown MiB value
changes, and starts or stops when `ui.show_perf_stats` changes.

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
