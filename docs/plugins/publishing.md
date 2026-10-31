# Publishing

Arcode has **no plugin marketplace and no network install**. It never
downloads plugins, never checks for plugin updates, and has no registry
or sign-in. A plugin reaches users as a folder: you publish it however
you like (a release on your code host, an archive on your website, a
folder in your company's shared drive), and users install it from a
folder on their machine.

## The plugin folder

```
acme.todo/
  plugin.toml     the manifest (required)
  plugin.wasm     the component (required; or the file named by `entry`)
  README.md       optional, at most 1 MiB
  LICENSE         optional, at most 1 MiB
```

- Name the folder after the plugin id.
- `plugin.wasm` may be at most 32 MiB.
- Nothing else is copied when a user installs the folder: no icons,
  images, data files or extra libraries. Everything your plugin needs at
  run time must be inside `plugin.wasm` (use `include_str!` or
  `include_bytes!` for small data).
- Every file must be a regular file. Symbolic links, pipes and devices
  are refused.

Build a release component and lay out the folder:

```sh
cargo build --release --target wasm32-wasip2
mkdir -p dist/acme.todo
cp plugin.toml README.md LICENSE dist/acme.todo/
cp target/wasm32-wasip2/release/acme_todo.wasm dist/acme.todo/plugin.wasm
```

Release settings that keep the component small:

```toml
[profile.release]
opt-level = "s"
lto = true
codegen-units = 1
strip = "debuginfo"
panic = "abort"
```

For reference, the example plugin is about 300 KB and the HTTP Client
about 390 KB.

Archive the folder (`.zip` or `.tar.gz`) for distribution and publish the
SHA-256 of the archive next to it, so users can check what they
downloaded:

```sh
tar czf acme.todo-1.2.0.tar.gz -C dist acme.todo
sha256sum acme.todo-1.2.0.tar.gz > acme.todo-1.2.0.tar.gz.sha256
```

## How users install a plugin

1. Unpack the folder anywhere.
2. In Arcode, open the Plugins view (puzzle icon in the activity bar),
   go to **Installed**, and click the folder button. The command palette
   has the same as **Plugins: Install from Folder…**, and Settings →
   Plugins has a button for it.
3. Pick the folder that contains `plugin.toml`.

Arcode then:

- validates the folder: the manifest, an id outside the reserved
  `arcode.` namespace, regular files, the size limits;
- copies `plugin.toml`, the entry component, `README.md` and `LICENSE`
  into the user's data folder (`<data folder>/plugins/<id>/`), staging
  the copy and swapping it in, so a failed install leaves the previous
  version untouched;
- records the SHA-256 of the copied `plugin.wasm`;
- compiles the component once in the background and caches the result.

The plugin is listed **disabled**. When the user turns it on, Arcode shows
the plugin's capability summary (see
[Permissions](permissions.md#what-users-see-before-enabling-a-plugin)).
Enabling approves that exact `plugin.wasm`.

Errors (for example "not an installable plugin: plugin id `arcode.x` uses
the reserved `arcode.` prefix") are shown as notifications.

### Trying a plugin without installing it

**Plugins: Load Unpacked…** loads a folder for the current session only,
without copying it, and enables it at once. This is meant for development
and for trying a plugin out. The `ARCODE_PLUGIN_DEV` environment variable
does the same at startup (several folders separated like `PATH`).

## Updates and versions

Installing a folder whose plugin id is already installed **updates** it.

- If the new version asks for the **same capabilities**, it stays enabled
  with the new hash.
- If it asks for **different capabilities**, it is disabled until the user
  enables it again, which shows the new capability summary.
- The user's grants, settings and stored data (`kv`) are kept, because
  they are keyed by the plugin id.

Arcode checks the hash of `plugin.wasm` every time the plugin starts. If
the file changed without going through Install from Folder (someone
replaced it on disk), the plugin does not run and the user is asked to
approve the new file.

Versioning advice:

- `version` in `plugin.toml` is shown to users and not interpreted. Use
  semantic versioning and bump it with every release, so users can tell
  versions apart in the Plugins view.
- Never change the plugin `id` between releases. A new id is a new
  plugin: users lose their grants and stored data.
- Keep `kv` data compatible across versions, or migrate it in `activate`.
- `api` is the plugin API version you built against (`"0.1"`). Arcode
  refuses plugins for an API it does not support, with a message that
  names the version.

## Uninstalling

**Uninstall** on the plugin's card (installed plugins only) removes its
folder, its entry in `plugins.toml` and its permission answers in every
project. Its stored `kv` data stays, so reinstalling the same plugin
keeps its data.

## Plugins with C code

A plugin may compile C code into its component, the way the Database
Viewer bundles SQLite. That needs clang with WebAssembly support and the
wasi-libc headers (Debian/Ubuntu: `apt install clang llvm wasi-libc`; or
the WASI SDK on any platform). `scripts/build-plugins.sh` in the Arcode
repository shows the exact compiler flags for the `wasm32-wasip2` target,
and `plugins/db-viewer` shows a complete example. Remember that the
sandbox has no file locks and no shared memory, which C libraries
sometimes assume.

## A checklist before you publish

- The id has your own prefix and will not change.
- The manifest asks only for the capabilities the plugin uses, and lists
  specific network hosts where possible.
- Long work calls `host::begin_task` and checks `host::cancelled()`.
- Errors are shown as plain sentences in your panel or as notifications.
- `cargo test` passes natively, and you tried the release build with
  **Plugins: Load Unpacked…** in a fresh profile (`ARCODE_CONFIG_DIR` and
  `ARCODE_DATA_DIR` pointing at empty folders) to see the first-run
  prompts as a new user would.
- The folder has a README that says what the plugin does and what it asks
  for, and a LICENSE.
