# Arcode

A fast, native, open-source IDE written in Rust. No account, no telemetry,
offline by default. Built on [GPUI](https://crates.io/crates/gpui).

Status: early development. See `docs/SPIKE.md` for the GUI stack validation
and `docs/ARCHITECTURE.md` for the crate layout.

## Building

Linux needs a few system libraries:

```bash
sudo apt-get install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libx11-xcb-dev libfontconfig-dev libfreetype-dev mesa-vulkan-drivers
```

Then:

```bash
cargo build --release
```

## Completion and snippets

The completion popup opens while you type (or with Ctrl+Space). It shows
what the language server suggests; words from your open files fill in when
no server runs or the server has nothing, file names are offered inside
string literals that start like a path (`"./`, `"../`, `"/`, `"~/`), and
snippets are mixed in. Up/Down/PageUp/PageDown move, Enter or Tab accepts
(Shift+Enter also replaces the rest of the word), Escape closes. Parameter
hints appear after `(` and `,` (or with Ctrl+Shift+Space).

Settings in `config.toml`:

```toml
[editor.completion]
auto_trigger = true     # open while typing; false = only Ctrl+Space
delay_ms = 50           # wait after a keystroke before asking
snippets = true         # offer snippets
signature_help = true   # parameter hints
```

### Your own snippets

Put them in `snippets.toml` next to `config.toml` (on Linux
`~/.config/arcode/snippets.toml`, or `$ARCODE_CONFIG_DIR/snippets.toml`).
Saving the file in Arcode reloads it. A snippet with the same prefix as a
built-in one replaces it for its languages.

```toml
[[snippet]]
prefix = "logd"                       # what you type
description = "debug log"             # shown next to it (optional)
languages = ["rust"]                  # language ids; omit for every language
body = 'log::debug!("${1:message} {:?}", ${2:value});$0'

[[snippet]]
prefix = "fori"
languages = ["javascript", "typescript"]
body = [                              # a list of lines also works
  "for (let ${1:i} = 0; $1 < ${2:n}; $1++) {",
  "\t$0",
  "}",
]
```

Body syntax (as in VS Code / LSP): `$1`, `$2` are tab stops (Tab and
Shift+Tab move between them, Escape leaves), `$0` is where the cursor ends,
`${1:default}` is a placeholder, repeating `$1` mirrors what you type,
`${1|one,two,three|}` offers a list of choices, and variables such as
`$TM_FILENAME`, `$CURRENT_YEAR` or `$UUID` are filled in. A tab character
in the body becomes your indentation unit.

## License

Licensed under either of Apache License 2.0 (`LICENSE-APACHE`) or MIT
(`LICENSE-MIT`) at your option. The bundled JetBrains Mono font is under the
SIL Open Font License (`assets/fonts/jetbrains-mono/OFL.txt`).
