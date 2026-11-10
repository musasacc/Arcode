# Languages

Arcode has syntax highlighting built in for the languages below. Smarter
features (completion, go to definition, diagnostics, rename, formatting,
...) come from **language servers**, separate programs that Arcode starts
for you. Release packages bundle them (see
[Bundled language servers](#bundled-language-servers)); a server you
install yourself is used instead. Arcode never downloads or installs
servers on its own: it runs an install command only when you click
**Install**, in a terminal tab where you see what it does (see
[When a server is missing](#when-a-server-is-missing)).

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

Arcode starts the command configured for the language, looking for it in
this order:

1. **A path you configured.** `command = "/opt/rust-analyzer/bin/rust-analyzer"`
   under `[lsp.<name>]` is run as given.
2. **Your `PATH`.** A server you installed (with rustup, npm, pipx, your
   package manager, ...) is used. On Windows Arcode also tries the
   extensions in `PATHEXT`, so the `.cmd` and `.bat` files npm installs
   are found by their bare name. rustup's placeholder does not count:
   rustup puts a `rust-analyzer` into `~/.cargo/bin` even when the
   rust-analyzer component is not installed, and that file only prints
   `error: Unknown binary 'rust-analyzer' in official toolchain …`.
   Arcode asks rustup (`rustup which rust-analyzer`, in the project
   folder, so `rust-toolchain.toml` counts) before starting it, and
   treats the server as not installed when the component is missing.
3. **The bundled servers.** Otherwise the copy that came with Arcode.

So installing a server yourself, or setting its path, always wins over the
bundled one; remove it from `PATH` (or the path from the configuration)
to go back to the bundled copy.

The "starts in" column lists each server's root markers: Arcode looks
upwards from the file for the nearest folder containing one of them and
starts the server there, so one project can hold several workspaces.
Without a marker, the project folder is used.

### When a server is missing

If a server is missing everywhere, the status bar shows it as not
installed and a message says so, once, with a button:

- **Install** when Arcode knows a command for your machine. It is picked
  by what is on your `PATH`:

  | Server | Command |
  |---|---|
  | rust-analyzer | `rustup component add rust-analyzer` |
  | pyright | `npm install -g pyright`, else `pipx install pyright`, else `python3 -m pip install --user pyright` |
  | typescript-language-server | `npm install -g typescript typescript-language-server` |
  | gopls | `go install golang.org/x/tools/gopls@latest` |
  | vscode-langservers-extracted (HTML, CSS, JSON) | `npm install -g vscode-langservers-extracted` |
  | bash-language-server, yaml-language-server, intelephense | `npm install -g <name>` |
  | taplo | `cargo install taplo-cli --locked --features lsp` |
  | ruby-lsp | `gem install ruby-lsp` |
  | clangd | `sudo apt-get install clangd`, `sudo dnf install clang-tools-extra` or `sudo pacman -S clang` |
  | marksman | `brew install marksman` or `winget install -e --id Artempyanykh.Marksman` |
  | lua-language-server | `sudo pacman -S lua-language-server`, `brew install lua-language-server` or `winget install -e --id LuaLS.lua-language-server` |
  | jdtls | `brew install jdtls` |

  Clicking it opens a new terminal tab that runs the command (no shell
  in between), so you see its output and can answer `sudo` or the
  package manager's questions. When it exits successfully, the servers
  that were not installed start again; no restart is needed.
- **How to Install** when there is no command for your machine (clangd
  on macOS and Windows, whose LLVM packages do not put it on `PATH`,
  marksman on most Linux distributions, a server whose tool, such as npm,
  is not on `PATH`): it opens the server's install page.

The same button is in the server's menu in the status bar (click the
server's name), and on the Languages page. **LSP: Show Missing Language
Servers** in the command palette lists every server that the open
project's files need and that is not installed, bundled or at its
configured path; picking one installs it.

A server Arcode knows no install command for (one you configured
yourself) gets the message without a button: install it, then restart
it from the status bar.

To use a server that is not on `PATH`, give its path:

```toml
[lsp.rust-analyzer]
command = "/opt/rust-analyzer/bin/rust-analyzer"
```

### Files outside a project

A single file with no project around it (`~/scratch/test.rs`, a quick
script you **Run Current File** on) still gets completion, hover and
diagnostics:

- **Rust.** rust-analyzer needs a `Cargo.toml` to find a workspace. For a
  `.rs` file with no `Cargo.toml` (or `rust-project.json`) in any folder
  above it, Arcode starts rust-analyzer in its detached-files mode: the
  file is analysed as a crate of its own with the standard library, and
  `mod helper;` finds `helper.rs` next to it. Other such files in the same
  folder join that server, which reloads to take them in (a second or two)
  rather than restarting; renaming or deleting one restarts it with the
  files that are left. You get rust-analyzer's own diagnostics (type
  errors, unresolved names, unused code), though rust-analyzer 1.97 does
  not always report them for such files, and never for one that joined a
  running server; `cargo check` on save is off for these files, because
  Cargo would need a nightly toolchain to check a file on its own (set
  `checkOnSave` in `initialization_options` to choose yourself). Files
  inside a Cargo project are not affected. For crates from crates.io or
  `cargo check`, make it a project: `cargo init` in the folder.

  rust-analyzer reads every `.rs` file in the file's folder and in all
  folders below it, hidden ones included. For a file directly in your
  home folder that would be `~/.cargo`, `~/.rustup` and every checkout:
  gigabytes of memory and minutes of indexing. Arcode counts first: when
  the folder holds more than 1,000 Rust files (200 in
  [low-end mode](low-end-mode.md)), or too many files to count quickly,
  the file is not analysed and a message says so. Keep single files in a
  folder of their own, such as `~/scratch`.
- **Go, C and C++, Python, TypeScript and JavaScript.** gopls, clangd,
  pyright and typescript-language-server serve a loose file as it is
  (gopls as a package of its own, clangd with its default flags), so
  nothing extra is needed. clangd only knows a project's include paths
  and defines from a `compile_commands.json`.

### How diagnostics look

Errors get a wavy underline, warnings and information a straight one, in
their colour; hints have none. `editor.inline_diagnostics` adds the
message after the end of the line, and the Problems panel lists them all.
Rest the mouse on the code to read the message.

Servers also say when code is **unused** (an import, a variable or a
function nobody uses, unreachable code) or **deprecated**:

- Unused code is drawn faded, in its usual colours. When the server only
  reports it as a hint (pyright's "is not accessed", for example), the
  fading is all you see: no underline, gutter dot or inline message. When
  it is a warning (rust-analyzer's unused variables and imports), you get
  the warning's underline as well. Turn fading off with
  [`editor.fade_unused = false`](configuration.md#editor) or in Settings,
  Editor, Fade unused code.
- Deprecated code (a call to a function marked `#[deprecated]`,
  `@deprecated` in TypeScript) is struck through.

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

`args` may contain `${workspaceFolder}` (the folder the server starts in)
and `${workspaceFolderHash}` (a short, stable hash of it, for servers that
keep state per project).

The built-in settings keep servers from downloading on their own:

- `yaml-language-server` does not fetch the SchemaStore catalog, and a
  schema a YAML file names in a `# yaml-language-server: $schema=<url>`
  comment is not downloaded (it shows as not loaded).
- `vscode-json-language-server` does not download schemas a JSON file
  links to with `"$schema"` (they show as not loaded). Schemas in local
  files work for both.
- `taplo` runs with schemas off, as it can only get them from the network:
  TOML files get formatting, syntax checks and symbols, but no schema
  completion or validation.
- `typescript-language-server` does not install `@types` packages with npm.

Setting `initialization_options` for one of these servers replaces the
built-in value, so you can turn them back on. For example, schema support
for TOML, which downloads the SchemaStore catalog when taplo starts:

```toml
[lsp.taplo]
initialization_options = { evenBetterToml = { schema = { enabled = true } } }
```

## Bundled language servers

Release packages (built with `scripts/package.sh`) include these servers,
so they work offline right after installing. The versions are pinned in
`packaging/language-servers.toml`:

| Language | Server | Version | Runs on |
|---|---|---|---|
| Rust | rust-analyzer | 2026-11-02 | native |
| C, C++, Objective-C | clangd | 23.1.0 | native (no Linux arm64 build upstream) |
| Go | gopls | v0.24.0 | native; needs Go installed to be useful |
| Python | pyright (`pyright-langserver`) | 1.1.414 | bundled Node.js |
| Python (lint) | ruff | 0.17.3 | native |
| TypeScript, JavaScript | typescript-language-server with TypeScript 6.0.3 | 6.0.3 | bundled Node.js |
| HTML, CSS, SCSS, Less, JSON | vscode-langservers-extracted | 4.10.0 | bundled Node.js |
| YAML | yaml-language-server | 2.0.0 | bundled Node.js |
| Shell Script | bash-language-server | 5.8.1 | bundled Node.js |
| Lua | lua-language-server | 3.19.2 | native |
| Markdown | marksman | 2026-02-08 | native |
| TOML | taplo | 0.10.0 | native |
| Java | jdtls | 1.61.0 | your Java (JDK 21 or newer) |

The JavaScript-based servers run on a private Node.js 24 that ships in the
bundle; it is not put on `PATH` and a system Node.js is never used for
them. jdtls runs on the Java found through `JAVA_HOME`, else on `PATH`;
without one, or with one older than Java 21, Arcode says so when a Java
file opens and the Languages page shows jdtls as "bundled, cannot start".
Arcode reads `JAVA_HOME` and `PATH` when it starts, so restart Arcode after
installing a JDK. jdtls keeps its per-project data in
`language-servers-state/jdtls` in the bundle's data folder (below). It is
pinned to 1.61.0, the newest release that runs on Java 21.

Not bundled:

- **Ruby (`ruby-lsp`)** runs inside your project's Ruby and Bundler, so it
  has to be installed with them: `gem install ruby-lsp`.
- **PHP (`intelephense`)**: its licence does not allow redistribution.
  Install it with `npm install --global intelephense`.

The Languages page shows bundled servers as "bundled" and offers no
install command for them. The licenses of everything in the bundle are
in `share/doc/arcode/THIRD-PARTY.txt` and
`share/arcode/language-servers/LICENSES/` of the installation.

### Where Arcode looks for the bundle

Arcode looks for the bundle when a language server starts, in the first
of the folders below. Once found it is kept; while there is none, it is
looked for again at later starts (and after an install), so a bundle
fetched while Arcode runs is used without a restart:

1. `<installation>/share/arcode/language-servers`, next to the executable
   (`bin/arcode` → `../share/arcode/language-servers`; on macOS
   `Arcode.app/Contents/share/arcode/language-servers`);
2. `language-servers` in Arcode's local data folder: `~/.local/share/arcode`
   on Linux, `~/Library/Application Support/arcode` on macOS,
   `%LOCALAPPDATA%\arcode` on Windows (not the roaming `%APPDATA%`, so
   roaming profiles do not copy it around), or `$ARCODE_DATA_DIR`.

A build from source (`cargo run -p arcode`) has no bundle next to it, so
only the servers on your `PATH` run. To get the same bundle the release
packages ship, run the fetch script from the checkout into the data
folder (restart the servers from the status bar, or just open a file):

```sh
scripts/fetch-language-servers.sh linux-x64 ~/.local/share/arcode
scripts/fetch-language-servers.sh macos-arm64 ~/Library/Application\ Support/arcode
scripts/fetch-language-servers.sh windows-x64 "$LOCALAPPDATA/arcode"   # Git Bash
```

The script needs Python 3.11 or newer and curl (and Go to build gopls);
it downloads about 300 MB, checks every file against its pinned sha256
and keeps the downloads in `~/.cache/arcode-language-servers`.

### Using your own server instead

Install it so it is on `PATH`, or point Arcode at it:

```toml
[lsp.pyright-langserver]
command = "/home/me/.local/bin/pyright-langserver"
args = ["--stdio"]
```

Either way the bundled copy is not used for that server. To use a newer
TypeScript for one project, add `typescript` to the project's
`devDependencies`: typescript-language-server prefers the project's own.

## Language packs

A language pack is everything Arcode knows about a language: its grammar,
file types, comment tokens, snippets, run commands, and the tools that
make it work well: language servers, debug adapters, formatters and
linters.

The **Languages** page shows every pack: open the Plugins view (puzzle
icon in the activity bar) and choose the Languages tab, or run
**Languages: Show Language Packs**. For each pack it shows which servers
are installed (on `PATH` or at their configured path), which are bundled
with Arcode, and the command to install a missing one on your system,
ready to copy, and the **Install** button described in
[When a server is missing](#when-a-server-is-missing).
Nothing is downloaded or installed from this page unless you click it.

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
