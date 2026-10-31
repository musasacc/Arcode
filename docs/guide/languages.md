# Languages

Arcode has syntax highlighting built in for the languages below. Smarter
features (completion, go to definition, diagnostics, rename, formatting,
...) come from **language servers**, separate programs that you install
and Arcode starts for you. Arcode never downloads or installs them.

## Built-in languages

| Language | Id | Files |
|---|---|---|
| Rust | `rust` | `.rs` |
| Python | `python` | `.py`, `.pyi`, `.pyw`, `SConstruct`, `SConscript` |
| JavaScript | `javascript` | `.js`, `.mjs`, `.cjs` |
| JavaScript React | `javascriptreact` | `.jsx` |
| TypeScript | `typescript` | `.ts`, `.mts`, `.cts` |
| TypeScript React | `typescriptreact` | `.tsx` |
| Go | `go` | `.go` |
| C | `c` | `.c`, `.h` |
| C++ | `cpp` | `.cc`, `.cpp`, `.cxx`, `.c++`, `.hh`, `.hpp`, `.hxx`, `.h++`, `.ipp`, `.inl`, `.tpp` |
| Java | `java` | `.java` |
| Ruby | `ruby` | `.rb`, `.rake`, `.gemspec`, `.ru`, `.rbw`, `Gemfile`, `Rakefile`, `Guardfile`, `Vagrantfile`, `Podfile`, `Brewfile`, `Fastfile` |
| PHP | `php` | `.php`, `.phtml`, `.php3`, `.php4`, `.php5`, `.phps` |
| Lua | `lua` | `.lua`, `.luacheckrc` |
| Shell Script | `shellscript` | `.sh`, `.bash`, `.zsh`, `.ksh`, `.command`, `.bashrc`, `.bash_profile`, `.bash_login`, `.bash_logout`, `.profile`, `.zshrc`, `.zshenv`, `.zprofile`, `.zlogin`, `PKGBUILD`, `APKBUILD` |
| HTML | `html` | `.html`, `.htm`, `.xhtml` |
| CSS | `css` | `.css` |
| JSON | `json` | `.json`, `flake.lock` |
| YAML | `yaml` | `.yaml`, `.yml`, `.clang-format`, `.clang-tidy`, `.clangd` |
| TOML | `toml` | `.toml`, `Cargo.lock`, `Pipfile`, `poetry.lock`, `uv.lock` |
| Markdown | `markdown` | `.md`, `.markdown`, `.mkd`, `.mdown` |
| Plain Text | `plaintext` | `.txt`, `.text` and anything not listed above |

The status bar shows the language of the current file. Click it, or press
Ctrl+K M (Cmd+K M on macOS), to pick another; the picker can also
remember the choice for every file with that pattern, by adding an entry
to `[files] associations` in `config.toml`:

```toml
[files]
associations = { "*.mdx" = "markdown", "Jenkinsfile" = "shellscript" }
```

Plugins can register more languages (the HTTP Client adds `http` for
`.http` and `.rest` files); see [Plugins](plugins.md).

Without a language server you still get highlighting, an outline for
**Go to Symbol in File** (Ctrl+Shift+O), completion from words in open
files and from file paths, run buttons and snippets.

## Language servers

| Language | Default server | Starts in the folder containing |
|---|---|---|
| Rust | `rust-analyzer` | `Cargo.toml` |
| Python | `pyright-langserver` (from pyright) | `pyproject.toml`, `setup.py`, `requirements.txt` |
| JavaScript, TypeScript (and React variants) | `typescript-language-server` | `package.json`, `tsconfig.json` |
| Go | `gopls` | `go.mod` |
| C, C++ | `clangd` | `compile_commands.json`, `CMakeLists.txt` |
| Java | `jdtls` | `pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle` |
| Ruby | `ruby-lsp` | `Gemfile` |
| PHP | `intelephense` | `composer.json` |
| Lua | `lua-language-server` | `.luarc.json` |
| Shell Script | `bash-language-server` | |
| HTML, CSS, JSON | `vscode-html-language-server`, `vscode-css-language-server`, `vscode-json-language-server` | |
| YAML | `yaml-language-server` | |
| TOML | `taplo` | |
| Markdown | `marksman` | `.marksman.toml` |

### How servers are found

Arcode starts the command configured for the language and looks it up on
your `PATH`. On Windows it also tries the extensions in `PATHEXT`, so the
`.cmd` and `.bat` files npm installs are found by their bare name.

The "starts in" column lists each server's root markers: Arcode looks
upwards from the file for the nearest folder containing one of them and
starts the server there, so one project can hold several workspaces.
Without a marker, the project folder is used.

If a server is missing, the status bar shows it as not installed and a
message names the command, once. Nothing else happens; install the
server and Arcode uses it the next time it needs it.

To use a server that is not on `PATH`, give its path:

```toml
[lsp.rust-analyzer]
command = "/opt/rust-analyzer/bin/rust-analyzer"
```

### Several servers for one language

A language can use more than one server, for example a type checker plus
a linter. List them in priority order:

```toml
[language.python]
servers = ["pyright-langserver", "ruff"]

[lsp.ruff]
command = "ruff"
args = ["server"]
features = { hover = false, completion = false }
```

Diagnostics, completion and code actions are collected from every server.
Hover, go to definition, rename, formatting and signature help ask the
servers in order until one answers. `features` turns individual features
off for one server. Every key is described in
[Configuration](configuration.md#lspname).

### Server settings

`initialization_options` is sent to the server when it starts, and `env`
sets environment variables for it:

```toml
[lsp.rust-analyzer]
initialization_options = { check = { command = "clippy" } }
env = { CARGO_TARGET_DIR = "target/ra" }
```

Saving `config.toml` restarts only the servers whose settings changed.

## Language packs

A language pack is everything Arcode knows about a language: its grammar,
file types, comment tokens, snippets, run commands, and the tools that
make it work well: language servers, debug adapters, formatters and
linters.

The **Languages** page shows every pack: open the Plugins view (puzzle
icon in the activity bar) and choose the Languages tab, or run
**Languages: Show Language Packs**. For each pack it shows which tools are
installed, checked offline by running them with `--version` and similar,
and the command to install a missing one on your system, ready to copy.
Nothing is downloaded or installed from this page.

| Pack | Languages | Tools it lists |
|---|---|---|
| Rust | `rust` | rust-analyzer, lldb-dap, CodeLLDB, rustfmt, clippy |
| Python | `python` | pyright, python-lsp-server (optional), debugpy, ruff, black (optional) |
| TypeScript / JavaScript | `typescript`, `javascript`, `typescriptreact`, `javascriptreact` | typescript-language-server, js-debug, prettier, eslint language server |
| Go | `go` | gopls, Delve (`dlv`), gofmt |
| C / C++ | `c`, `cpp` | clangd, lldb-dap, CodeLLDB, clang-format |
| Web | `html`, `css`, `json` | vscode-langservers-extracted, prettier |
| TOML | `toml` | taplo |
| Shell Script | `shellscript` | bash-language-server, shellcheck (optional) |
| YAML | `yaml` | yaml-language-server |
| Markdown | `markdown` | marksman |
| Java | `java` | jdtls |
| Lua | `lua` | lua-language-server |
| Ruby | `ruby` | ruby-lsp |
| PHP | `php` | intelephense |

Each pack has an enable switch. Turning a pack off (or setting
`enabled = false` under `[language.<id>]`) starts no language servers and
shows no run buttons for that language; highlighting stays.

```toml
[language.java]
enabled = false
```

## Per-language editor settings

`tab_size` and `insert_spaces` can be set per language:

```toml
[language.go]
tab_size = 8
insert_spaces = false
```

With `editor.detect_indentation` on (the default), Arcode also guesses
the indentation of each file from its content. The status bar shows what
the current file uses; click it to change it, or to convert the file
between tabs and spaces.
