# Hello World

An example Arcode plugin, meant to be copied as the starting point for
your own. It is small, but it uses the pieces most plugins need:

- **A palette command.** "Hello: Say Hello" shows a notification that
  names the file you are editing.
- **A code lens.** "Hello: explain this TODO" appears above every `TODO`
  comment in Rust, Python, JavaScript, TypeScript, Go, C, C++, Java and
  Markdown files. Clicking it runs a command with the lens's arguments.
- **A panel.** A "Hello" entry in the activity bar opens a sidebar panel
  with a text input, a Greet button and some text. The number of greetings
  is kept in the plugin's key-value storage, so it survives restarts.
- **A setting** (`greeting`, default "Hello") read with
  `host::settings_as`.
- **Unit tests** that run natively with `cargo test`, against the SDK's
  fake host.

It asks only for `editor = "context"`: it reads the document a lens or
command is about, and nothing else. No project files, no edits, no
network.

## Files

| File | What it shows |
|---|---|
| `plugin.toml` | The manifest: commands, a lens, a panel, a setting, capabilities. |
| `src/lib.rs` | The `Plugin` implementation: commands, lenses, panel events, kv storage. |
| `src/view.rs` | The panel as a tree of widgets, with keys for the nodes the plugin needs to find again. |
| `src/todo.rs` | Finding TODO comments: plain Rust, no host calls. |
| `src/tests.rs` | The plugin end to end against the fake host. |

## Build and try it

From the root of the Arcode repository:

```sh
scripts/build-plugins.sh hello-world
```

This writes `target/wasm-plugins/example.hello-world/` with `plugin.toml`,
`plugin.wasm` and this README. An Arcode binary built in the same checkout
(`cargo run -p arcode`, or `./target/release/arcode`) picks it up there;
if Arcode is already running, use **Plugins: Reload**. The example is not
bundled with Arcode: a plain `scripts/build-plugins.sh` never builds it,
and `rm -rf target/wasm-plugins/example.hello-world` removes it again.

With an Arcode installed elsewhere, run **Plugins: Load Unpacked…** and
pick that folder, or start Arcode with the folder in `ARCODE_PLUGIN_DEV`:

```sh
ARCODE_PLUGIN_DEV="$PWD/target/wasm-plugins/example.hello-world" arcode path/to/project
```

Then:

1. Open a source file with a `// TODO: ...` comment: a lens appears above
   it. Click it.
2. Run **Hello: Say Hello** from the command palette.
3. Click the info icon in the activity bar to open the panel, type a name
   and press Greet.

## Test

```sh
cargo test --manifest-path plugins/Cargo.toml -p hello-world
```

## Make it yours

1. Copy this folder out of the repository.
2. In `Cargo.toml`, rename the package and replace every
   `.workspace = true` entry with a real value: `version = "0.1.0"`,
   `edition = "2024"`, `serde = { version = "1", features = ["derive"] }`,
   and `arcode-plugin` as a `git` or `path` dependency on the SDK, as
   described in the comment there.
3. In `plugin.toml`, change `id` to your own (`yourname.something`),
   `name` and `description`, and replace the contributions.
4. Build with `cargo build --release --target wasm32-wasip2` and copy
   `target/wasm32-wasip2/release/<crate_name>.wasm` to `plugin.wasm` next
   to your `plugin.toml`.

The plugin author guide in [`docs/plugins/`](../../docs/plugins/README.md)
explains each step, starting with
[Getting started](../../docs/plugins/getting-started.md).
