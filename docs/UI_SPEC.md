# UI specification (Milestone 1)

Shared reference for the GPUI crates. Read `docs/ARCHITECTURE.md` first.

## Look and feel

- Flat, soft rounded corners (4-8px), 1px borders in `colors.border`, no
  heavy shadows except popovers (`shadow_md`).
- One accent (`colors.accent`) for focus, selection markers, primary buttons;
  `accent_alt` sparingly (e.g. modified indicators).
- UI font: system UI font (or `settings.ui.font_family`) at
  `settings.ui.font_size` (13px). Editor/terminal font:
  `settings.editor.font_family` (bundled "JetBrains Mono") at
  `settings.editor.font_size`, line height `font_size * line_height`.
- Animations 100-150 ms via `arcode_ui_kit::animation_duration(cx)`; when it
  returns `None` (low-end mode or disabled) do not animate at all. The
  sidebar and bottom panel slide, popups, dialogs and toasts fade in.
- Popup shadows go through `arcode_ui_kit::Elevated` (`.elevated(cx)`), which
  draws none in low-end mode.
- Never schedule frames continuously. No cursor blink unless
  `settings.editor.cursor_blink` is on (then use a timer that toggles and
  notifies, paused when the window is not focused).

## Shared crate: `arcode-ui-kit` (already written)

- `theme(cx) -> &UiTheme`: `colors: ThemeColors` (Hsla fields named like the
  theme TOML `[ui]` keys), `terminal: TerminalColors`, `syntax(capture) ->
  HighlightStyle` with dotted-prefix fallback, `is_dark`.
- `settings(cx) -> &arcode_config::Config`, `animation_duration(cx)`.
- `Assets` (AssetSource with icons), `load_fonts(cx)`, `BUNDLED_MONO_FONT`.
- `icon(IconName::Folder)`, `button(id, label)`, `icon_button(id, icon)`,
  `Tooltip::text(text, cx)`, `kbd("Ctrl+P")`, `divider()`.

## Layout (app crate `arcode`)

```
+--------------------------------------------------------------------------+
| [project v]  [branch]        [ Search files  Ctrl+P ]      [Run] [gear]  |  title bar (36px)
+--+-----------+-----------------------------------------------------------+
|A | Explorer  | tab | tab | tab                                 [split]  |
|c |  file     +-----------------------------------------------------------+
|t |  tree     |                                                           |
|i |           |   editor (split panes)                                    |
|v |           |                                                           |
|i |           +-----------------------------------------------------------+
|t |           | Terminal | Problems | Todo | Debug                    [x] |
|y |           |   panel content                                           |
+--+-----------+-----------------------------------------------------------+
| branch | 2 errors | Ln 12, Col 4 | Spaces: 4 | UTF-8 | LF | Rust | rust-analyzer: ready |  status bar (24px)
+--------------------------------------------------------------------------+
```

- Activity bar (44px): Files, Search, Git, Debug, Plugins, Todo, Packages. The
  Plugins view lists the language packs ("Languages"): per pack its
  languages and extensions, snippets, each language server as installed
  or not installed (with the install command as text and a copy button;
  nothing is downloaded), an enable switch, and on click the comment
  tokens, run commands and file associations.
- Packages activity (package icon, after Todo): a filter box and one
  section per manifest (nested projects too) with its manager badge
  (cargo, npm, pnpm, yarn, bun, uv, poetry, pipenv, pip, go, composer,
  bundler) and, on hover, Add…, Update all, Check for updates and Refresh.
  Rows show name, requested → locked version, kind (dev, build, optional,
  peer, indirect) and source (git, path, workspace) badges, and
  "newer: x.y.z" after a check. Clicking a row opens the manifest at its
  line; hover buttons and the context menu offer Update, Remove… (confirmed
  in the row) and Open Documentation. Add… opens a small form above the
  list (name, optional version, dev switch; Enter adds, Escape closes).
  A Cargo virtual workspace root has no Add…, and its
  `[workspace.dependencies]` rows no Remove.
- Status bar, right side: cursor, indentation, encoding, line ending,
  language, language servers. Clicking the indentation, line ending or
  language opens a small filterable picker (Ctrl+K M also picks the
  language; the language picker can add a `[files] associations` entry).
- Package banner: when the project (or an opened file) has a manifest
  whose dependencies are missing, a bar above the editor area says "This
  project has packages that are not installed." with Install packages
  (runs the shown command in a terminal tab), Dismiss and Don't ask again
  for this project.
- Sidebar: file tree only (resizable, default 240px). No projects here.
  The tree header has New File, New Folder, Refresh and Collapse buttons.
  Right click (or Shift+F10 / the menu key) opens its context menu: Open,
  New File…, New Folder… (names are typed inline in the tree), Rename…
  (inline, F2), Delete (to the trash after a confirmation; a permanent
  delete is offered, and confirmed again, only when there is no trash),
  Copy Path (Ctrl+Alt+C), Copy Relative Path (Ctrl+Alt+Shift+C), Reveal
  in File Manager (Finder / File Explorer), Open in Terminal. Arrows move,
  Right / Left expand / collapse (or step into / out of a folder), Home /
  End, Enter or Space opens. Rows can be dragged onto a folder to move
  them. Changes made outside Arcode appear by themselves.
- Bottom panel: tabs Terminal, Problems, Todo, Debug. Toggle with Ctrl+J
  (Cmd+J on macOS). The Debug tab has the debug toolbar, the call stack,
  variables and watches, and the debug console (see `docs/DEBUGGING.md`).
- Run and debug: run buttons in the gutter next to mains and tests (Run /
  Debug menu), the breakpoint lane left of them, the Run and Debug view
  (launch configurations, breakpoints list) and the title bar Run button
  with a dropdown of detected commands. Details in `docs/DEBUGGING.md`.
- Welcome screen when no project is open: logo/name, "Open Folder",
  "New Project…", recent projects list (click to open, a button to open
  in a new window, x to remove), key hints.
- Settings (gear in the title bar, Ctrl+, / Cmd+,): a "Settings" tab with
  the sections Appearance (theme cards that preview on hover, follow
  system / dark / light, dark and light themes, UI size, animations,
  low-end mode, perf stats), Editor, Fonts (editor and UI font pickers over
  the system fonts plus the bundled JetBrains Mono, size, line height,
  ligatures, a preview with ligature, CJK and emoji samples), Terminal,
  Languages (enable switches), Keymap (read-only, filterable) and Advanced
  (workspace, search, debug and large-file options, Reload, Start Tour,
  Welcome Setup). Every control edits `config.toml` in place, keeping its
  comments; "Open config.toml" opens the file. "Preferences: Select Theme"
  (Ctrl+K Ctrl+T) previews themes as the selection moves; Escape restores.
  Themes from the config folder's `themes/*.toml` are listed too. In a
  narrow (split) pane the section list shows only icons and controls wrap
  below their labels.
- First run (no `config.toml`): a modal welcome dialog with theme (live
  preview), editor font, keymap (default or Vim) and performance mode
  (normal or low-end); it writes `config.toml`, then offers the tour. Skip
  always closes it, even when the file cannot be written. While the dialog
  or the tour is up, workspace shortcuts do nothing. The tour ("Help:
  Start Tour") dims the window around the project dropdown, the activity
  bar, the search box, the Run button, the terminal panel, the status bar
  and the settings gear in turn, with a callout and Back / Next / Skip
  (arrows, Enter, Escape).
- "New Project…" opens a dialog: name, location (Browse…), a template
  (empty folder, Rust application / library, Node.js, Python venv, Go
  module) and "Open in a new window". Create makes the folder; the
  template's command (e.g. `cargo init`) runs in the new project's
  terminal only then, never on its own.
- Title bar project dropdown: the current project, projects open in other
  windows (click brings that window forward), recent projects (click
  switches this window, buttons to open in a new window or remove), Open
  Folder…, Open Folder in New Window…, New Project…, New Window, Close
  Project. Every window has its own project, language servers, terminals
  and session.
- Sessions: reopening a project restores its tabs per pane, the split
  layout, the active tab, selections and scroll position per file, the
  sidebar view and width, the bottom panel (shown, height, tab) and the
  expanded folders (`workspace.restore_session`). Projects open at quit
  are reopened at the next start, one window each, where their windows
  were (`workspace.reopen_projects`). A file closed and opened again
  goes back to its cursor and scroll position. Unsaved text is never
  stored: switching or closing the project, closing a window or quitting
  asks first when there are unsaved changes or a terminal is running a
  command (those end the terminals).
  On Linux use client-side decorations if available, otherwise the system
  title bar plus our bar below it.

## Keyboard

Use `secondary` (Cmd on macOS, Ctrl elsewhere) for app shortcuts:
open file finder (Ctrl+P, M1: simple fuzzy list of project files using a
substring/fuzzy match), save (Ctrl+S), close tab (Ctrl+W), toggle panel
(Ctrl+J), toggle sidebar (Ctrl+B), new terminal (Ctrl+Shift+`), focus
terminal (Ctrl+`), split right (Ctrl+\), undo/redo, copy/cut/paste,
select all, Ctrl+D (select next occurrence).

Multiple cursors (in an editor): select all occurrences (Ctrl+Shift+L),
add a cursor above / below (Ctrl+Alt+Up / Down, Cmd+Alt on macOS),
Alt+click adds or removes a cursor, Alt+Shift+drag or a middle-button
drag makes a box selection, cursor undo (Ctrl+U), Escape back to one
cursor. Vim mode (`editor.vim_mode`, or "Editor: Toggle Vim Mode") shows
its mode, pending keys and command line at the left of the status bar.

Code navigation (in an editor): hover at the cursor (Ctrl+K Ctrl+I),
go to definition (F12 or Ctrl+click), go to implementation (Ctrl+F12),
find all references (Shift+F12), go to symbol in file (Ctrl+Shift+O),
go to symbol in workspace (Ctrl+T), back / forward (Alt+Left /
Alt+Right; macOS Ctrl+- / Ctrl+Shift+-).

Code actions and refactoring (in an editor): code actions / quick fix
(Ctrl+., Cmd+. on macOS, or click the light bulb in the gutter),
Refactor... (Ctrl+Shift+R), rename symbol (F2), format document
(Shift+Alt+F), format selection (Ctrl+K Ctrl+F), context menu (right
click or Shift+F10). "Editor: Toggle Inlay Hints" in the palette.

Windows and files: new window (Ctrl+Shift+N), close window
(Ctrl+Shift+W), quit (Ctrl+Q), reveal the active file in the tree
(Ctrl+K Ctrl+R), change the file's language (Ctrl+K M). Explorer commands ("Explorer: New File", ...) are in the
palette; outside the tree they act on the active file.

Git: Source Control view (Ctrl+Shift+G), commit from the message box
(Ctrl+Enter), toggle the blame column (Ctrl+Alt+B), next / previous merge
conflict (Alt+] / Alt+[). Clicking the branch in the title bar or status
bar opens the branch picker. The command names are listed in
`arcode::actions::commands()` for a command palette.

Run and debug: run the project (Ctrl+F5), start debugging / continue
(F5), pause (F6), step over / into / out (F10 / F11 / Shift+F11), restart
(Ctrl+Shift+F5), stop (Shift+F5), toggle breakpoint (F9), Run and Debug
view (Ctrl+Shift+D). Every "Debug: ..." command is in the palette.

Search: find in the active file (Ctrl+F; replace Ctrl+H, Cmd+Alt+F on
macOS; Enter / Shift+Enter or F3 / Shift+F3 for next / previous; Alt+C,
Alt+W, Alt+R toggle match case, whole word, regex; Ctrl+Alt+Enter in the
replace field replaces all), Search view (Ctrl+Shift+F; with the replace
field Ctrl+Shift+H; Enter in the replace or glob fields searches again,
Ctrl+Alt+Enter in the replace field replaces all after a confirmation),
command palette (Ctrl+Shift+P or F1, or type `>` in
the title bar search box). Every command is registered in
`arcode::commands` with a name, a category and its key binding.

## Milestone 1 acceptance

1. Open a folder containing a Cargo project. Open `src/main.rs` which has a
   type error. rust-analyzer starts lazily; within its normal startup time
   the error range gets a red wavy underline. Hovering shows the message
   with its source (`rust-analyzer` / `rustc`). The Problems panel lists it;
   clicking it jumps to the location. Status bar shows the error count.
2. The Terminal tab runs a real shell in the project root. Typing
   `cargo run` and Enter runs it and shows colored output.
