# Packaging Arcode

A release of Arcode is a folder (a "prefix") that can be installed or moved
anywhere:

```
<prefix>/bin/arcode                         (arcode.exe on Windows)
<prefix>/share/arcode/plugins/<id>/         bundled WASM plugins
    plugin.toml  plugin.wasm  [README.md  LICENSE]
<prefix>/share/doc/arcode/                  README and licenses
```

`scripts/package.sh [out-dir]` builds it: `cargo build --release -p
arcode`, then `scripts/build-plugins.sh` with `ARCODE_WASM_PLUGINS` set to
`<prefix>/share/arcode/plugins`, then the docs. `ARCHIVE=1` also writes a
`.tar.gz` (a `.zip` for Windows targets). `TARGET=<triple>` cross-builds;
`ARCODE_BIN=<file>` packages an existing binary; `ARCODE_SKIP_PLUGINS=1`
leaves the plugins out (the app runs fine without them).

Building the plugins needs the `wasm32-wasip2` Rust target, and for the DB
viewer (SQLite is C) clang and the wasi-libc headers (`apt install
wasi-libc clang`, Homebrew `wasi-libc`, or the WASI SDK); see the header of
`scripts/build-plugins.sh`. The plugins are platform-independent: one build
serves every target.

## How the app finds the bundled plugins

At startup nothing is read. When the plugin manager is created (250 ms
after the window opens) it reads the `plugin.toml` files in, in order:

1. `<exe>/../share/arcode/plugins` (`arcode_plugin_host::paths::bundled_roots`);
2. debug builds only, and only when the binary is not inside such a
   prefix: this checkout's `target/wasm-plugins` (`scripts/build-plugins.sh`
   without arguments), so `cargo run` finds them;
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
for the DB viewer on a release build; see docs/PLUGINS.md, Appendix B,
"Step 12, shipping").
