# Themes

## Built-in themes

| Theme | Appearance | |
|---|---|---|
| **Arcode Ember** | dark | The default dark theme: a warm charcoal canvas with one ember accent. |
| **Arcode Chalk** | light | The default light theme: a chalk-grey canvas, near-white panels and an amber-brown accent. |
| Arcode Dark | dark | Classic: a flat, neutral dark grey with a soft blue accent. |
| Arcode Light | light | Classic light theme. |
| Arcode Midnight | dark | Classic: deep blue-black, high contrast. |

Pick one with **Preferences: Select Theme** (Ctrl+K Ctrl+T, Cmd+K Cmd+T
on macOS; the arrow keys preview each theme), on the Settings page, or in
`config.toml`:

```toml
[ui]
theme = "system"               # "system", "dark", "light" or a theme name
dark_theme = "Arcode Ember"    # used for "system" in dark mode and for "dark"
light_theme = "Arcode Chalk"   # used for "system" in light mode and for "light"
```

With `theme = "system"`, Arcode follows the operating system's light or
dark mode and switches when it changes.

Plugins' panels, the terminal and every built-in view use the active
theme; there is nothing else to configure.

## Text rendering

On Linux, Arcode draws text with grayscale anti-aliasing. Two settings
change how the edges of glyphs are blended, which is what makes text look
thin, heavy or soft. Both are in **Settings > Fonts > Text rendering**,
with a sample of light text on Ember and dark text on Chalk, and in
`config.toml`:

```toml
[ui]
text_gamma = 1.8      # 1.0 - 2.2
text_contrast = 1.0   # 0 - 8
```

- **Gamma** works on all text. Raising it (2.0, 2.2) makes light text on a
  dark theme heavier and dark text on a light theme thinner; lowering it
  does the opposite.
- **Contrast** thickens dark and mid-tone text only: text on a light
  theme, muted labels and syntax colours. Light text on a dark theme does
  not change.

So for thin text on a dark theme, raise the gamma; on a light theme, raise
the contrast (2 to 4). The renderer reads both when Arcode starts, so a
change takes effect after a restart; the Settings page says when one is
needed. While a setting is at its default, `ZED_FONTS_GAMMA` or
`ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST` set in your environment applies
instead, and the Settings page shows it. macOS and Windows draw text with
the system's own settings (on Windows, the ClearType Text Tuner), so the
two settings are not shown there.

The editor font weight is `editor.font_weight` (Settings > Fonts > Weight).

## Writing your own theme

A theme is a TOML file in the `themes` folder next to `config.toml`
(`~/.config/arcode/themes/` on Linux; see
[Configuration](configuration.md#where-files-live) for other systems).
Every key is optional except `appearance` (when the theme does not
`extends` another one): anything you leave out is inherited.

```toml
# ~/.config/arcode/themes/dusk.toml
name = "Dusk"
appearance = "dark"            # "dark" or "light"
extends = "Arcode Midnight"    # optional; default: Arcode Ember for dark, Arcode Chalk for light

[ui]
background = "#1b1d26"
accent = "#e0a458"

[syntax]
keyword = { color = "#c792ea", bold = true }
comment = "#6a7080"            # shorthand for { color = "..." }
"function.method" = "#82aaff"  # dotted names style one kind of a capture

[terminal]
foreground = "#e0e0e0"
bright_red = "#ff8080"
```

The theme appears in the theme picker the next time you open it; no
restart needed. Select it by its `name` (or, without a `name`, by its
file name without `.toml`).

Top-level keys:

| Key | Meaning |
|---|---|
| `name` | The name shown in pickers and used in `config.toml`. Defaults to the file name. A user theme with the same name as an earlier one replaces it. |
| `appearance` | `"dark"` or `"light"`. Required unless inherited through `extends`. |
| `extends` | The theme to inherit from: a built-in or another of your themes, in any file. Default: the built-in default theme of the same appearance. |

Colors are hex strings: `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`. A
translucent color (alpha below `ff`) is drawn over what is beneath it,
which suits `selection` and `hover`.

Problems in a theme file (an unknown key, a bad color) are reported as
warnings in Arcode's log and the affected key is inherited; a file that
is not valid TOML is skipped. See [Troubleshooting](troubleshooting.md#logs).

### `[ui]`

| Key | What it colors |
|---|---|
| `background` | Editor and main background. |
| `surface` | Sidebars and panels. |
| `surface_raised` | Popups, menus, dialogs. |
| `border` | Separators and outlines. |
| `border_focused` | Outline of the focused element. |
| `text` | Primary text. |
| `text_muted` | Secondary text (descriptions, inactive labels). |
| `text_disabled` | Disabled controls. |
| `accent` | Primary accent: buttons, links, focus. |
| `accent_text` | Text drawn on top of `accent`. |
| `accent_alt` | Secondary accent. |
| `selection` | Text selection (usually translucent). |
| `cursor` | The editor caret. |
| `line_highlight` | Background of the cursor line. |
| `gutter_text` | Line numbers. |
| `gutter_text_active` | Line number of the cursor line. |
| `error` | Error diagnostics. |
| `warning` | Warning diagnostics. |
| `info` | Information diagnostics. |
| `hint` | Hint diagnostics. |
| `success` | Success states. |
| `scrollbar_thumb` | Scrollbar thumb. |
| `tab_active` | Active tab background. |
| `tab_inactive` | Inactive tab background. |
| `title_bar` | Window title bar. |
| `status_bar` | Status bar. |
| `activity_bar` | Activity bar (the icon strip on the left). |
| `panel_header` | Panel headers. |
| `hover` | Hover highlight (usually translucent). |
| `diff_added` | Added lines in diffs and the gutter. |
| `diff_modified` | Modified lines. |
| `diff_deleted` | Deleted lines. |
| `canvas` | Window background behind the floating panels; the title bar, activity bar and status bar sit on it. |
| `pill` | Active tab pill, selected tree and list rows, the active activity-bar tile, count badges. |
| `border_strong` | Control outlines (inputs, checkboxes, switches, key chips) and edges of overlays. |
| `selection_inactive` | Text selection in an unfocused editor or terminal. |
| `paused_line` | Debugger: the band behind the paused line, and the Paused chip. |

#### Derived keys

The last five keys were added to the format later. When your theme sets
the keys they are derived from but not the derived key itself, Arcode
computes it from your colors instead of inheriting the base theme's
value, so older themes keep a consistent look:

| Key | Derived from |
|---|---|
| `canvas` | `title_bar` |
| `pill` | `surface_raised` |
| `border_strong` | halfway between `text_muted` and `border` |
| `selection_inactive` | `line_highlight` |
| `paused_line` | 14 % `warning` over `background` |

If your theme sets none of the source keys either, the derived key is
inherited like any other. Setting a derived key yourself always wins.

### `[syntax]`

Keys are syntax highlighting capture names; values are a color string, or
a table with `color`, `bold`, `italic` and `underline`:

```toml
[syntax]
keyword = { color = "#e6a0c8", bold = true }
comment = { color = "#ab9f8e", italic = true }
string = "#cfd98c"
```

A dotted name styles a more specific capture: `function.method` styles
methods, while `function` styles every function that has no more specific
style. Quote dotted names (`"function.method" = ...`) or nest them as
tables.

The captures the built-in themes style:

| Capture | Typical use |
|---|---|
| `keyword` | `fn`, `if`, `return`, ... |
| `function`, `function.method`, `function.builtin`, `function.macro` | Functions, methods, built-ins, macros |
| `type`, `type.builtin`, `constructor` | Types and constructors |
| `string`, `string.special`, `string.escape`, `escape`, `character` | Strings, escapes, characters |
| `number`, `boolean`, `constant`, `constant.builtin` | Literals and constants |
| `comment`, `comment.doc` | Comments and doc comments |
| `variable`, `variable.parameter`, `variable.builtin` | Variables, parameters, `self` / `this` |
| `property` | Fields and properties |
| `attribute`, `tag.attribute` | Attributes and decorators, HTML attributes |
| `label` | Labels and lifetimes |
| `module`, `namespace` | Modules and namespaces |
| `punctuation`, `punctuation.delimiter`, `punctuation.bracket` | Punctuation |
| `operator` | Operators |
| `tag` | HTML and XML tags |

### `[terminal]`

| Key | Meaning |
|---|---|
| `foreground` | Default text color. |
| `background` | Default background. |
| `cursor` | Cursor color. |
| `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white` | ANSI colors 0 to 7. |
| `bright_black`, `bright_red`, `bright_green`, `bright_yellow`, `bright_blue`, `bright_magenta`, `bright_cyan`, `bright_white` | ANSI colors 8 to 15. |

### Starting from a built-in theme

The built-in theme files are a good template; they set every key:
[`crates/arcode-config/themes/`](../../crates/arcode-config/themes/)
(`arcode-ember.toml`, `arcode-chalk.toml`, `arcode-dark.toml`,
`arcode-light.toml`, `arcode-midnight.toml`). Copy one into your `themes`
folder, change `name`, and edit the colors. Or start small with `extends`
and only the colors you want to change.

Keep text readable: aim for a contrast ratio of at least 4.5:1 between
text colors and their backgrounds, and 3:1 for borders and icons. The
built-in Ember and Chalk themes are tested against these numbers.
