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
  returns `None` (low-end mode or disabled) do not animate at all.
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
| branch | Rust | rust-analyzer: ready | Ln 12, Col 4 | 2 errors | 41ms 180MB |  status bar (24px)
+--------------------------------------------------------------------------+
```

- Activity bar (44px): Files, Search, Git, Debug, Plugins, Todo. Only Files
  is functional in Milestone 1; the others show a short "coming soon" view.
- Sidebar: file tree only (resizable, default 240px). No projects here.
- Bottom panel: tabs Terminal, Problems, Todo, Debug (Todo/Debug are
  placeholders in M1). Toggle with Ctrl+J (Cmd+J on macOS).
- Welcome screen when no project is open: logo/name, "Open Folder",
  "New Project…" (creates a folder), recent projects list (click to open,
  x to remove), key hints.
- Title bar project dropdown: recent projects, Open…, New…, Close Project.
  On Linux use client-side decorations if available, otherwise the system
  title bar plus our bar below it.

## Keyboard

Use `secondary` (Cmd on macOS, Ctrl elsewhere) for app shortcuts:
open file finder (Ctrl+P, M1: simple fuzzy list of project files using a
substring/fuzzy match), save (Ctrl+S), close tab (Ctrl+W), toggle panel
(Ctrl+J), toggle sidebar (Ctrl+B), new terminal (Ctrl+Shift+`), focus
terminal (Ctrl+`), split right (Ctrl+\), undo/redo, copy/cut/paste,
select all, Ctrl+D (select next occurrence).

Git: Source Control view (Ctrl+Shift+G), commit from the message box
(Ctrl+Enter), toggle the blame column (Ctrl+Alt+B), next / previous merge
conflict (Alt+] / Alt+[). Clicking the branch in the title bar or status
bar opens the branch picker. The command names are listed in
`arcode::actions::commands()` for a command palette.

## Milestone 1 acceptance

1. Open a folder containing a Cargo project. Open `src/main.rs` which has a
   type error. rust-analyzer starts lazily; within its normal startup time
   the error range gets a red wavy underline. Hovering shows the message
   with its source (`rust-analyzer` / `rustc`). The Problems panel lists it;
   clicking it jumps to the location. Status bar shows the error count.
2. The Terminal tab runs a real shell in the project root. Typing
   `cargo run` and Enter runs it and shows colored output.
