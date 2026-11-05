# Packaging Arcode

A release of Arcode is a folder (a "prefix") that can be installed or moved
anywhere:

```
<prefix>/bin/arcode                         (arcode.exe on Windows)
<prefix>/share/arcode/plugins/<id>/         bundled WASM plugins
    plugin.toml  plugin.wasm  [README.md  LICENSE]
<prefix>/share/arcode/language-servers/     bundled language servers
<prefix>/share/doc/arcode/                  README, licenses, THIRD-PARTY.txt
```

`scripts/package.sh [out-dir]` builds it: `cargo build --release -p
arcode`, then `scripts/build-plugins.sh` with `ARCODE_WASM_PLUGINS` set to
`<prefix>/share/arcode/plugins`, then `scripts/fetch-language-servers.sh`
for the target's platform into `<prefix>/share/arcode`, then the docs and
the third-party notice. `ARCHIVE=1` also writes a `.tar.gz` (a `.zip` for
Windows targets). `TARGET=<triple>` cross-builds; `ARCODE_BIN=<file>`
packages an existing binary; `ARCODE_SKIP_PLUGINS=1` leaves the plugins
out and `ARCODE_SKIP_LANGUAGE_SERVERS=1` the language servers (both make
a small package for development; the app runs fine without them).

Building the plugins needs the `wasm32-wasip2` Rust target, and for the DB
viewer (SQLite is C) clang, `llvm-ar` and the wasi-libc headers (`apt install
clang llvm wasi-libc`, `dnf install clang llvm wasi-libc-devel`, Homebrew
`wasi-libc`, or the WASI SDK); see the header of `scripts/build-plugins.sh`.
The HTTP Client is pure Rust and builds without them. The plugins are
platform-independent: one build serves every target.

## Bundled language servers

Every release ships the language servers of the built-in packs, so code
intelligence works offline right after installing. They are pinned in
`packaging/language-servers.toml`: for each server its version, languages,
license, how it is laid out, and per platform (`linux-x64`, `linux-arm64`,
`macos-arm64`, `macos-x64`, `windows-x64`) the download URL and its
sha256. The JavaScript-based servers (pyright, typescript-language-server
with TypeScript, vscode-langservers-extracted, yaml-language-server,
bash-language-server) are npm packages pinned by `packaging/npm/package.json`
and its `package-lock.json` (integrity hashes for every package) and run
on a private Node.js (24 LTS, checksums from nodejs.org's
`SHASUMS256.txt`).

`scripts/fetch-language-servers.sh <platform> <dest>` builds
`<dest>/language-servers`:

```
manifest.toml        copy of the manifest (the app reads the commands)
bin/                 rust-analyzer, ruff, marksman, taplo, gopls
lib/clangd/          clangd with its builtin headers
lib/lua-language-server/
lib/jdtls/
lib/npm/             package.json, package-lock.json, node_modules/
node/                bin/node (node.exe on Windows), nothing else
LICENSES/<name>/     license texts; LICENSES/npm/<package>/ for npm
THIRD-PARTY.txt      what is included, versions, licenses, homepages
SKIPPED.txt          what was left out for this platform, and why
```

It downloads each file once into a cache (`--cache`, else
`$ARCODE_LSP_CACHE`, else `~/.cache/arcode-language-servers`;
`scripts/package.sh` uses `target/language-servers-cache`), checks the
sha256 of every download and stops on a mismatch before anything is
installed (fail closed), runs `npm ci --ignore-scripts` from the lockfile
with the downloaded Node.js (or the `npm` on `PATH` when fetching for
another platform), builds the bundle in a temporary folder and swaps it
in at the end. A second run with the same manifest and lockfile does
nothing. It needs Python 3.11+, curl and tar, and runs in bash on Linux
and macOS and in Git Bash on Windows.

Decisions per server:

- **gopls** has no prebuilt binaries. It is built with `go install
  golang.org/x/tools/gopls@<version>` for the target platform (pure Go,
  `CGO_ENABLED=0`, so it cross-compiles); the module is checked against
  the `h1:` hash from the Go checksum database recorded in the manifest,
  and Go fetches the toolchain the module needs. Without Go on the
  packaging machine (or with `ARCODE_LSP_SKIP_GO=1`) gopls is left out
  and listed in `SKIPPED.txt`: it runs the Go toolchain itself (`go
  list`), so anyone who can use it has Go and can install it from the
  Languages page.
- **clangd** has no upstream Linux arm64 build; that platform uses the
  distribution's clangd. The macOS build (as marksman's) is universal.
- **jdtls** is the same archive everywhere and runs on the user's Java
  (`JAVA_HOME`, else `PATH`; JDK 21 or newer). Java is not bundled; without
  it, or with an older one (the version is read from the JDK's `release`
  file), Arcode says so when a Java file opens. It is pinned to 1.61.0,
  the newest release that runs on Java 21 (some Eclipse platform bundles
  of 1.62.0 and later need Java 25). Its OSGi configuration and
  per-project data go to `<local data>/language-servers-state/jdtls`; the
  bundle is never written to.
- **ruby-lsp** is not bundled: it runs inside the project's Ruby and
  Bundler. **intelephense** is not bundled: its licence forbids
  redistribution. Both keep their install command on the Languages page.

Sizes for `linux-x64` (measured with the versions above):

| | Size |
|---|---|
| Downloads | about 300 MB |
| `language-servers/` unpacked | 732 MB (751 MB on disk, 15,900 files) |
| `language-servers/` as `.tar.gz` | 307 MB |
| largest parts | clangd 225 MB, npm servers 202 MB, Node.js 122 MB, jdtls 53 MB, rust-analyzer 42 MB, gopls 30 MB |

### How the app finds the bundled language servers

At startup nothing is read. The first time a language server starts, the
app looks, once and off the UI thread, for `manifest.toml` in:

1. `<exe>/../share/arcode/language-servers` (the prefix layout above, on
   every OS: `Arcode.app/Contents/share/arcode/language-servers` on
   macOS, `share\arcode\language-servers` next to `bin\` on Windows);
2. `<local data>/language-servers` (`~/.local/share/arcode`,
   `~/Library/Application Support/arcode`, `%LOCALAPPDATA%\arcode` (not the
   roaming `%APPDATA%`), or `$ARCODE_DATA_DIR`), where a build from source
   gets a bundle with
   `scripts/fetch-language-servers.sh linux-x64 ~/.local/share/arcode`.

Each time a server starts (and restarts), its command is resolved, in the
language server runtime: a configured path as is, else `PATH`, else the
bundle. So what the user installs or configures always wins. Node-based servers are started as `<bundle>/node/bin/node <script>`,
never with a system Node.js. The bundle is read-only for the app; jdtls
keeps its configuration and per-project data in
`<local data>/language-servers-state/jdtls`.

### Updating a server

Change its version, URLs and sha256 values in the manifest together (for
GitHub releases the release page lists each asset's sha256 digest; for
Node.js use `SHASUMS256.txt`); for an npm server change
`packaging/npm/package.json` and run `npm install --package-lock-only
--ignore-scripts` there. `cargo test -p arcode-project bundled` checks the
manifest (every platform has a URL and sha256, versions are pinned, npm
versions match the lockfile), and the "Language servers" workflow fetches
and verifies Linux, Windows and macOS bundles when the manifest, the
lockfile or the fetch script change.

### Licensing

The bundled servers are unmodified upstream builds under their own
licenses (MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-3-Clause,
EPL-2.0 for jdtls); Node.js is MIT with the notices of its dependencies
in its LICENSE. The fetch script collects every license text into
`language-servers/LICENSES/` and writes `THIRD-PARTY.txt`, which
`scripts/package.sh` also copies to `share/doc/arcode/`. See
[THIRD-PARTY.md](THIRD-PARTY.md) for the notes behind these choices.

## How the app finds the bundled plugins

At startup nothing is read. When the plugin manager is created (250 ms
after the window opens) it reads the `plugin.toml` files in, in order:

1. `<exe>/../share/arcode/plugins`, and, for a binary whose folder is one
   or two levels below a folder named `target` (`target/debug`,
   `target/release`, a cross build), that `target` folder's `wasm-plugins`
   (`arcode_plugin_host::paths::bundled_roots`). So any build run from a
   checkout, debug or release, loads what `scripts/build-plugins.sh` wrote
   to `target/wasm-plugins`;
2. debug builds only, and only when the binary is not inside such a
   prefix: this checkout's `target/wasm-plugins`, found from the source
   path, so a debug build with `CARGO_TARGET_DIR` outside the checkout
   finds them too;
3. the user's installed plugins (`<data>/plugins`), then "Load Unpacked"
   folders. A later source replaces a plugin with the same id.

Platform layouts:

- **Linux / BSD:** install the prefix as is (`/usr/local`, `/opt/arcode`,
  `~/.local`). A distribution package puts the binary in `/usr/bin` and the
  plugins in `/usr/share/arcode/plugins`, which is the same relative path.
- **macOS:** in an app bundle the binary is `Arcode.app/Contents/MacOS/arcode`,
  so the plugins go to `Arcode.app/Contents/share/arcode/plugins`.
- **Windows:** keep `bin\arcode.exe` and `share\arcode\plugins\` side by
  side in the installation folder.

The bundled folders are read-only for the app. Compiled components are
cached per user (`<cache>/arcode/plugins`), so the first activation of each
bundled plugin compiles it once (about 0.5 s for the HTTP client and 3 s
for the DB viewer on a release build; see docs/PLUGINS.md, Appendix B.8).
