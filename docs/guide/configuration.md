# Configuration

All of Arcode's settings live in one file, `config.toml`. You can change
them in three ways:

- the **Settings page** (Ctrl+, / Cmd+,), which writes `config.toml` for
  you and keeps your comments;
- **Preferences: Open Settings File (config.toml)**, which opens the file
  in the editor;
- any other editor.

Saving the file applies it at once, in every window. Language server
changes restart only the servers whose settings changed.

Every setting on this page has a row on the Settings page, except the
ones that are tables rather than single values: `[files] associations`
(the language picker in the status bar adds entries), `[debug] adapters`
and `adapter_paths`, and the per-language `[language.<id>]` and
per-server `[lsp.<name>]` tables (the Languages section turns language
packs on and off). `editor.vim_mode` is the Vim choice of the Keymap
row. Each row shows its key in small type after its label, so you can
find it in the file. Type a key into **Filter settings** (`tab_size`,
`editor.minimap`) to find its row; the filter also knows a few other
words for some settings (`caret`, `zoom`, `color scheme`), and typing
one of the table keys above shows where it is set, with a button that
opens `config.toml` (so does the button next to the filter). A row whose
value is not the default has a reset button that writes the default
back, keeping the comments above the line. Lists (fallback fonts, search
excludes, TODO tags) are typed separated by commas; a comma inside a
glob's `{…}` or `[…]` stays in the glob (`*.{min.js,map}`). Terminal
arguments are separated by spaces: put one that holds spaces in double
or single quotes (`-c "echo hi"`, `-c 'echo hi'`), and write a double
quote inside double quotes as `\"`. A field is written when you press
Enter or leave it (Tab moves to the next field or reset button), and
Escape puts the setting back.

Arcode is forgiving with this file. Unknown keys and values of the wrong
type are reported as warnings in Arcode's log (see
[Troubleshooting](troubleshooting.md#logs)) and the affected settings
keep their defaults; Arcode always starts. A file with a TOML syntax
error is ignored as a whole (running language servers are kept as they
are) until you fix it.

The first-run dialog writes a new `config.toml` (both "Save and
Continue" and "Skip" do). It lists every option
with its default and a comment, so the file is its own reference. The
template is
[`crates/arcode-config/assets/default-config.toml`](../../crates/arcode-config/assets/default-config.toml).
Delete a line to go back to the default.

## Where files live

Arcode uses two per-user folders: a **config folder** for things you
edit, and a **data folder** for things Arcode keeps for you.

| | Linux | macOS | Windows |
|---|---|---|---|
| Config folder | `~/.config/arcode` (or `$XDG_CONFIG_HOME/arcode`) | `~/Library/Application Support/arcode` | `%APPDATA%\arcode` |
| Data folder | `~/.local/share/arcode` (or `$XDG_DATA_HOME/arcode`) | `~/Library/Application Support/arcode` | `%APPDATA%\arcode` |
| Cache | `~/.cache/arcode` | `~/Library/Caches/arcode` | `%LOCALAPPDATA%\arcode` |

Set `ARCODE_CONFIG_DIR` or `ARCODE_DATA_DIR` to use other folders, for
example for a portable setup or to try something without touching your
real settings. The variables name the folder itself (no `arcode` is
appended). With `ARCODE_DATA_DIR` set, compiled plugins are cached under
it too (in `<data folder>/cache/plugins/`), so nothing is written to the
cache folder.

What is in them:

| File | Folder | What it holds |
|---|---|---|
| `config.toml` | config | All settings (this page). |
| `snippets.toml` | config | Your own snippets. |
| `themes/*.toml` | config | Your own themes ([Themes](themes.md)). |
| `plugins.toml` | config | Which plugins are enabled, and the approved hash of each installed plugin. |
| `plugin-grants.toml` | config | Plugin permissions you granted for every project. |
| `state.toml` | data | Window and session state: open projects, tabs, splits, cursors, the Run command per project, breakpoints. |
| `plugins/<id>/` | data | Plugins you installed. |
| `projects/<hash>/plugin-grants.toml` | data | Plugin permissions granted for one project. |
| `plugin-data/<id>/` | data | Data plugins store (their key-value store). |
| `plugins/*.cwasm` | cache | Compiled plugins, rebuilt when missing. |

Arcode keeps none of its own state in your project folder. The only file
it creates there for itself is `.arcode/launch.toml`, and only when you
ask for it (**Debug: Open launch.toml**).

## `[editor]`

| Key | Default | Meaning |
|---|---|---|
| `font_family` | `"JetBrains Mono"` | Code font. JetBrains Mono ships with Arcode. |
| `font_fallbacks` | `["Noto Sans CJK SC", "Noto Color Emoji", "Segoe UI Emoji", "Apple Color Emoji"]` | Fonts tried, in order, for characters missing from `font_family` (CJK, emoji, ...). |
| `font_size` | `14.0` | Font size in points, 4 to 200. |
| `font_weight` | `400` | Font weight, 100 to 900: 400 is regular, 700 bold. A font without that weight uses its nearest one; the bundled JetBrains Mono has 400 and 700, so try a value like 500 with a font that has more weights. |
| `line_height` | `1.5` | Line height as a multiple of the font size, 0.5 to 4.0. |
| `ligatures` | `true` | Programming ligatures such as `->` and `!=`. |
| `tab_size` | `4` | Width of a tab stop in columns, 1 to 32. |
| `insert_spaces` | `true` | Insert spaces when pressing Tab. |
| `detect_indentation` | `true` | Guess tabs or spaces and the indent size of each file from its content. The status bar shows what a file uses and lets you change it. |
| `show_line_numbers` | `true` | Line numbers in the gutter. |
| `highlight_current_line` | `true` | A background on the cursor line. |
| `cursor_blink` | `false` | Blinking cursor. Always off in low-end mode. |
| `word_wrap` | `false` | Soft-wrap long lines at the edge of the editor. |
| `keymap` | `"default"` | `"default"` or `"vim"`. `"vim"` turns Vim mode on. |
| `vim_mode` | `false` | Modal Vim editing: Normal, Insert, Visual and Replace modes, operators, text objects, registers (`"+` is the system clipboard), dot repeat, `/` and `?` search, `:w`, `:q`, `:wq`, `:e`, `:s`. **Editor: Toggle Vim Mode** switches it for the session. |
| `auto_close_brackets` | `true` | Insert the closing bracket or quote automatically. |
| `format_on_save` | `false` | Format the file with its language server when saving. |
| `format_on_save_timeout_ms` | `1500` | Saving never waits longer than this for the formatter; after that it saves unformatted. |
| `large_file_bytes` | `4194304` | Files at least this large (bytes) keep no full syntax tree; only the visible part is parsed for highlighting. |
| `large_file_lines` | `50000` | The same, by line count. |
| `hover` | `true` | Show documentation when the mouse rests on a symbol. Ctrl+K Ctrl+I shows it at the cursor either way. |
| `hover_delay_ms` | `300` | How long the mouse must rest before the hover appears. |
| `occurrence_highlight` | `true` | Highlight the other occurrences of the symbol under the cursor. |
| `inlay_hints` | `true` | Type and parameter hints from the language server inside the code. **Editor: Toggle Inlay Hints** switches them for the session. Off in low-end mode unless set here. |
| `code_action_lightbulb` | `true` | A light bulb in the gutter when code actions exist on the cursor line. |
| `inline_diagnostics` | `"errors"` | Show diagnostics after the end of their line: `"errors"`, `"all"` or `"off"`. |
| `fade_unused` | `true` | Draw code the language server reports as unused (imports, variables, unreachable code) faded. Deprecated code is struck through either way. See [How diagnostics look](languages.md#how-diagnostics-look). |
| `minimap` | `true` | A minimap at the right edge of file editors: the file scaled down in its syntax colours, with the visible part, selections, search matches, git changes, errors and warnings marked. Click a line to center it, drag to scroll. It is hidden when it would leave the text less than three times its width, in diffs, and always in low-end mode. |
| `minimap_width` | `96` | Width of the minimap in pixels, 40 to 240 (one pixel per column, so this is about the number of columns shown). |

### `[editor.completion]`

| Key | Default | Meaning |
|---|---|---|
| `auto_trigger` | `true` | Open the completion list while typing. When `false`, only Ctrl+Space opens it. |
| `delay_ms` | `50` | Wait this long after a keystroke before asking for completions. |
| `snippets` | `true` | Offer built-in snippets and your own from `snippets.toml` in the list. |
| `signature_help` | `true` | Show parameter hints after typing `(` or `,` in a call. |

## `[ui]`

| Key | Default | Meaning |
|---|---|---|
| `font_family` | unset | UI font. Unset uses the bundled Inter (or the system UI font if Inter cannot be loaded). `".SystemUIFont"` picks the platform UI font. |
| `font_size` | `13.0` | UI font size. |
| `theme` | `"system"` | `"system"` (follow the OS), `"dark"`, `"light"`, or the name of a theme. |
| `dark_theme` | `"Arcode Ember"` | The theme used for `"system"` in dark mode and for `"dark"`. |
| `light_theme` | `"Arcode Chalk"` | The theme used for `"system"` in light mode and for `"light"`. |
| `low_end_mode` | `false` | Reduce visual effects and background work on slow machines. See [Low-end mode](low-end-mode.md). |
| `animations` | `true` | Animations. Always off in low-end mode. |
| `animation_ms` | `120` | Animation duration in milliseconds. |
| `show_perf_stats` | `true` | Show frame time and memory use in the status bar. |
| `menu_bar` | `true` | Show the menus (File, Edit, ...) in the title bar. When `false`, and in a narrow window, one menu button holds them; Alt opens them either way. No effect on macOS, which uses its system menu bar. |
| `text_gamma` | `1.8` | How text edges are blended, 1.0 to 2.2 in steps of 0.1. Higher draws light text on dark themes heavier (and dark text on light themes thinner). Linux only; takes effect after a restart. See [Text rendering](themes.md#text-rendering). |
| `text_contrast` | `1.0` | Extra contrast for text edges, 0 to 8. Higher thickens dark and mid-tone text: light themes, muted text, syntax colours. Light text on dark themes is not affected. Linux only; takes effect after a restart. |

Built-in themes: **Arcode Ember** (dark) and **Arcode Chalk** (light),
plus the classic **Arcode Dark**, **Arcode Light** and **Arcode Midnight**.
See [Themes](themes.md).

## `[terminal]`

| Key | Default | Meaning |
|---|---|---|
| `shell` | unset | Shell program. Unset: `$SHELL` on Linux and macOS, PowerShell on Windows. |
| `args` | `[]` | Arguments passed to the shell. |
| `font_family` | unset | Terminal font. Unset: the editor font. |
| `font_size` | unset | Terminal font size. Unset: the editor font size. |
| `scrollback` | `10000` | Lines of scrollback history. |

```toml
[terminal]
shell = "/bin/zsh"
args = ["-l"]
```

## `[git]`

| Key | Default | Meaning |
|---|---|---|
| `gutter` | `true` | Added, modified and deleted bars in the editor gutter. |
| `inline_blame` | `true` | Author, date and summary of the cursor line, shown at its end. Off in low-end mode unless set here. |
| `inline_blame_delay_ms` | `500` | How long the cursor rests on a line before its blame appears. |

## `[search]`

| Key | Default | Meaning |
|---|---|---|
| `max_results` | `2000` | A project search stops after this many matches; "Show more" raises it. |
| `debounce_ms` | `250` | Wait after the last keystroke before searching. |
| `respect_gitignore` | `true` | Skip what `.gitignore` and `.ignore` list. |
| `include_hidden` | `false` | Also search hidden files and folders (dot files). |
| `exclude` | `[]` | Globs always excluded from project search, for example `["*.min.js", "dist"]`. |

## `[todo]`

| Key | Default | Meaning |
|---|---|---|
| `tags` | `["TODO", "FIXME", "HACK", "XXX", "BUG", "NOTE"]` | Comment tags listed in the TODO view. Case-sensitive, whole words. |

## `[workspace]`

| Key | Default | Meaning |
|---|---|---|
| `restore_session` | `true` | Reopen a project's tabs, splits, cursors and panels when it is opened. |
| `reopen_projects` | `true` | When Arcode starts without a path, reopen the projects that were open at quit, one window each. |
| `confirm_delete` | `true` | Ask before deleting files in the file tree. |
| `watch_files` | `true` | Show changes made outside Arcode in the file tree, using the operating system's file notifications (no polling). |
| `projects_folder` | unset | Folder offered for new projects and clones. Unset: next to the current project (a clone with no project yet: `~/Projects` if it exists). Example: `"~/Projects"`. |
| `package_banner` | `true` | Show a banner when a project's packages are not installed (for example `package.json` without `node_modules`). Nothing is installed unless you click "Install packages". |

## `[files]`

| Key | Default | Meaning |
|---|---|---|
| `associations` | `{}` | File pattern to language id (or name). |

A pattern without `/` matches the file name, one with `/` the path; `*`
and `?` are wildcards. The most specific matching pattern wins. The
language picker in the status bar (Ctrl+K M) can add entries for you.

```toml
[files]
associations = { "*.mdx" = "markdown", "Dockerfile.*" = "shellscript" }
```

## `[debug]`

| Key | Default | Meaning |
|---|---|---|
| `run_buttons` | `true` | Run and Debug buttons next to `main` functions and tests in the gutter. |
| `open_panel_on_start` | `true` | Show the Debug panel when a debug session starts. |
| `adapters` | `{}` | Preferred debug adapter per language: `codelldb`, `lldb-dap`, `gdb` (14 or newer), `debugpy` or `delve`. Unset: the first one installed. Example: `{ rust = "gdb" }`. |
| `adapter_paths` | `{}` | Adapter executables that are not on `PATH`, by adapter name. Example: `{ "lldb-dap" = "/usr/bin/lldb-dap-18" }`. |

Debug adapters are never downloaded; install them yourself. See
[Run and debug](run-and-debug.md).

## `[plugins]`

| Key | Default | Meaning |
|---|---|---|
| `network` | `true` | Master switch for plugin network access. Off: every plugin request is refused. On: a plugin sends only while handling your click, and asks before it contacts a host for the first time. |

See [Plugins](plugins.md) and [Privacy](privacy.md).

## `[language.<id>]`

Per-language settings, keyed by language id (`rust`, `python`,
`typescript`, ...; the ids are listed in [Languages](languages.md)).

| Key | Meaning |
|---|---|
| `enabled` | `false` turns the language pack off: no language servers and no run buttons for that language. Highlighting stays. |
| `servers` | Language servers to start, in priority order: names of `[lsp.<name>]` tables. |
| `formatter` | An external formatter command line. The current version reads this key but does not run it yet: formatting always goes through a language server. |
| `tab_size` | Overrides `editor.tab_size` for this language. |
| `insert_spaces` | Overrides `editor.insert_spaces` for this language. |

Built-in assignments:

| Language | Server |
|---|---|
| `rust` | `rust-analyzer` |
| `typescript`, `javascript`, `typescriptreact`, `javascriptreact` | `typescript-language-server` |
| `python` | `pyright-langserver` |
| `go` | `gopls` |
| `c`, `cpp` | `clangd` |
| `toml` | `taplo` |
| `json`, `html`, `css` | `vscode-json-language-server`, `vscode-html-language-server`, `vscode-css-language-server` |
| `shellscript` | `bash-language-server` |
| `yaml` | `yaml-language-server` |
| `markdown` | `marksman` |
| `java` | `jdtls` |
| `lua` | `lua-language-server` |
| `ruby` | `ruby-lsp` |
| `php` | `intelephense` |

Any key you set replaces the built-in value for that language only.

When several servers serve one file, diagnostics, completion and code
actions come from all of them; hover, definition, rename, formatting and
signature help ask them in `servers` order until one answers.

```toml
[language.python]
servers = ["pyright-langserver", "ruff"]
tab_size = 4
```

## `[lsp.<name>]`

How to start a language server. The built-in servers are already
defined; a table with a new name defines a new server, and keys in a
built-in table override just those keys.

| Key | Meaning |
|---|---|
| `command` | Executable name or path. Defaults to the table name. Names are looked up on `PATH`, then in the [bundled language servers](languages.md#bundled-language-servers); a path is used as given. |
| `args` | Command-line arguments. `${workspaceFolder}` is replaced by the folder the server starts in, `${workspaceFolderHash}` by a short hash of it. |
| `env` | Extra environment variables for the server. |
| `root_markers` | Files whose presence marks a workspace root, searched upwards from the file. |
| `initialization_options` | Passed to the server when it starts (`initializationOptions`). The built-in YAML, TOML, JSON and TypeScript servers have values that turn off their schema and type downloads; setting this replaces them. |
| `features` | Features to use from this server, all on by default: `diagnostics`, `completion`, `hover`, `signature_help`, `definition`, `references`, `symbols`, `highlights`, `rename`, `formatting`, `code_actions`, `inlay_hints`. |

```toml
[lsp.ruff]
command = "ruff"
args = ["server"]
env = { RUFF_CACHE_DIR = "/tmp/ruff" }
root_markers = ["pyproject.toml", "ruff.toml"]
initialization_options = { settings = { lineLength = 100 } }
features = { hover = false, completion = false }

[lsp.rust-analyzer]
command = "/opt/rust-analyzer/bin/rust-analyzer"
```

A feature that is off is never asked from that server, and
`diagnostics = false` hides what it reports. An unknown key in `features`
makes the whole table invalid (with a warning). A server that is not
installed is reported once; nothing is ever downloaded or installed.

## Snippets

Your own snippets go in `snippets.toml` in the config folder, next to
`config.toml`:

```toml
[[snippet]]
prefix = "fn"
body = "fn ${1:name}(${2}) {\n\t$0\n}"
description = "function"
languages = ["rust"]          # optional; empty or missing = every language

[[snippet]]
name = "todo"
prefix = "todo"
body = ["// TODO($1):", "// $0"]   # a list of lines works too
```

The body uses the usual snippet syntax: `$1`, `$2`, ... are tab stops,
`${1:text}` a placeholder, `$0` the final cursor position. Snippets show
up in the completion list when `editor.completion.snippets` is on.

## Command-line and environment

| | Meaning |
|---|---|
| `arcode [PATH]` | Open a folder as the project, or a file with its project. |
| `ARCODE_CONFIG_DIR` | Use this folder instead of the config folder. |
| `ARCODE_DATA_DIR` | Use this folder instead of the data folder (and for the plugin cache). |
| `ARCODE_PLUGIN_DEV` | Plugin folders to load for this session, separated like `PATH` (for plugin development). |
| `ARCODE_SKIP_FIRST_RUN` | Set to a non-empty value other than `0` to skip the first-run dialog (for automation). |
| `ZED_FONTS_GAMMA`, `ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST` | Read by the renderer on Linux. `ui.text_gamma` and `ui.text_contrast` set them when they differ from their defaults; otherwise a value set in the environment applies, and Settings > Fonts says so. Programs started from Arcode inherit them; an Arcode started from Arcode's terminal uses its own settings, not those of the first one. |
| `ARCODE_TEXT_RENDERING_OUTER` | Set by Arcode for the programs it starts: which of the two variables above it set, and what they were before. Not meant to be set by hand. |
| `RUST_LOG` | Log level of Arcode's log on stderr, for example `RUST_LOG=info`. Default: warnings and errors. |
