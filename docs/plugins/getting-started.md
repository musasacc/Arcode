# Getting started

This page takes you from an empty folder to a plugin running in Arcode.
The plugin adds one command, **Word Count: Count Words**, that shows how
many words the current document has. Along the way you will build it,
load it, change it, read its log and test it.

If you would rather start from working code, copy
[`plugins/hello-world`](../../plugins/hello-world) instead: it has a
command, a code lens and a panel, and every step below applies to it too.

## 1. Install the toolchain

You need Rust (through [rustup](https://rustup.rs)) and the WebAssembly
target Arcode plugins are built for:

```sh
rustup target add wasm32-wasip2
```

Use a recent stable Rust. Arcode itself is built with the version in its
`rust-toolchain.toml` (1.97 at the time of writing), so that one is known
to work. Pure Rust plugins need nothing else. A plugin that compiles C
code (for example SQLite) also needs clang and the wasi-libc headers; see
[Publishing](publishing.md#plugins-with-c-code).

## 2. Create the crate

```sh
cargo new --lib word-count
cd word-count
```

Replace `Cargo.toml` with:

```toml
[package]
name = "word-count"
version = "0.1.0"
edition = "2024"

[lib]
# cdylib is the WebAssembly component; rlib lets `cargo test` link the code.
crate-type = ["cdylib", "rlib"]

[dependencies]
arcode-plugin = { package = "arcode-plugin-api", git = "https://github.com/musasacc/arcode", rev = "<commit>" }

[profile.release]
opt-level = "s"     # smaller plugin.wasm
lto = true
strip = "debuginfo"
```

The SDK is not published on crates.io; depend on it from the Arcode
repository and pin a commit (`rev`) so your build does not change under
you. `<commit>` is a placeholder: replace it with a full commit hash, for
example the latest one from

```sh
git ls-remote https://github.com/musasacc/arcode HEAD
```

There are no tagged releases to pin yet. With a local checkout of Arcode
you can use a path instead:

```toml
arcode-plugin = { package = "arcode-plugin-api", path = "../arcode/crates/arcode-plugin-api" }
```

The package is named `arcode-plugin-api`; renaming it to `arcode-plugin`
as above lets you write `use arcode_plugin::...`, which is what the rest
of this guide uses. Add `serde = { version = "1", features = ["derive"] }`
when you want typed command arguments.

## 3. Write the manifest

Create `plugin.toml` next to `Cargo.toml`:

```toml
[plugin]
id = "acme.word-count"
name = "Word Count"
version = "0.1.0"
api = "0.1"
description = "Counts the words in the current document"

[capabilities]
editor = "context"     # read the document a command runs on

[[contributes.commands]]
id = "count"
title = "Word Count: Count Words"
```

- `id` is how Arcode knows your plugin. Use your own prefix (`acme.`
  here); `arcode.` is reserved for the plugins that ship with Arcode.
- `[capabilities]` lists what the plugin may ask for. This one only reads
  the document a command runs on, which needs no prompt.
- `[[contributes.commands]]` adds a palette command. Arcode lists it as
  soon as it reads the manifest, before any of your code runs.

Every field is described in [The manifest](manifest.md).

## 4. Write the code

Replace `src/lib.rs` with:

```rust
// The plugin type is only used by the wasm export below, so a native
// `cargo build` or `cargo test` would warn that it is never constructed.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use arcode_plugin::prelude::*;

#[derive(Default)]
struct WordCount;

impl Plugin for WordCount {
    fn command(&mut self, id: &str, _args: Json) -> Result<()> {
        match id {
            "count" => count_words(),
            other => Err(Error::msg(format!("unknown command `{other}`"))),
        }
    }
}

fn count_words() -> Result<()> {
    let Some(doc) = editor::current_document() else {
        host::notify(host::Level::Info, "Open a file first.");
        return Ok(());
    };
    let text = editor::text(doc.id, None)?;
    let words = words_in(&text);
    let name = doc.path.unwrap_or_else(|| "this document".to_owned());
    host::notify(host::Level::Info, &format!("{name}: {words} words"));
    Ok(())
}

/// Words are runs of characters separated by whitespace.
pub fn words_in(text: &str) -> usize {
    text.split_whitespace().count()
}

// Export the plugin. Only for wasm: the native build is what tests use, and
// the export names are not valid symbols there.
#[cfg(target_arch = "wasm32")]
arcode_plugin::export_plugin!(WordCount);
```

What is going on:

- Your plugin is a type that implements `Plugin` and `Default`. Arcode
  creates it with `Default` when the plugin activates and keeps it between
  calls until the plugin is unloaded.
- `command` is called with the manifest-local id (`"count"`) whenever one
  of your commands runs. Every `Plugin` method has a default, so you
  implement only what your manifest contributes.
- `editor::current_document`, `editor::text` and `host::notify` are host
  functions. In the WebAssembly build they call into Arcode; in a native
  build they go to a fake host used by tests (step 9).
- Returning `Err` from `command` shows the message to the user.
- `export_plugin!` connects your type to the component's exports.

## 5. Build

```sh
cargo build --release --target wasm32-wasip2
```

The component is `target/wasm32-wasip2/release/word_count.wasm` (the crate
name with `-` turned into `_`). A plugin folder needs it under the name
`plugin.wasm`, next to the manifest:

```sh
mkdir -p dist/acme.word-count
cp plugin.toml dist/acme.word-count/
cp target/wasm32-wasip2/release/word_count.wasm dist/acme.word-count/plugin.wasm
```

Name the folder after the plugin id. Arcode requires that for installed
plugins, and it keeps things tidy for development too.

## 6. Load it into Arcode

There are two ways to run a plugin you are working on. Neither copies
anything or makes the plugin permanent.

**Load Unpacked.** In Arcode, open the command palette (Ctrl+Shift+P,
Cmd+Shift+P on macOS), run **Plugins: Load Unpacked…** and pick the
`dist/acme.word-count` folder. If your system has no folder picker,
Arcode shows a path field instead; type the folder's full path there. The
plugin stays loaded until Arcode quits.

**`ARCODE_PLUGIN_DEV`.** Start Arcode with the environment variable set to
the folder (several folders are separated like `PATH`):

```sh
ARCODE_PLUGIN_DEV="$PWD/dist/acme.word-count" arcode ~/some/project
```

That assumes `arcode` is on your `PATH`, for example from an installed
package. If you built Arcode from source, run it from the Arcode checkout
instead and give the plugin folder as an absolute path:

```sh
# Adjust the paths: here Arcode is checked out in ~/src/arcode and the
# plugin crate is ~/word-count.
cd ~/src/arcode
ARCODE_PLUGIN_DEV="$HOME/word-count/dist/acme.word-count" cargo run -p arcode -- ~/some/project
# or, after a release build:
ARCODE_PLUGIN_DEV="$HOME/word-count/dist/acme.word-count" ./target/release/arcode ~/some/project
```

Either way the plugin appears in the Plugins view (the puzzle icon in the
activity bar) as "loaded unpacked", enabled. Unpacked plugins are trusted
for the session: they are not asked to be approved like installed ones,
but they still get every permission prompt.

A plugin that uses the same id as an installed or bundled plugin replaces
it while it is loaded unpacked.

## 7. Run it

Open any file, open the command palette and run **Word Count: Count
Words**. A notification appears: "Word Count: src/main.rs: 120 words".
Arcode puts your plugin's name in front of every notification.

If the command is not in the palette, the manifest was probably refused:
**Plugins: Show Log** (see step 8) ends with an entry that starts with
`Skipped /path/to/dist/acme.word-count: invalid plugin.toml: ...`. For a
TOML error it runs over several lines: the position, the offending line
with a caret under the problem, then the reason, such as
``unknown field `bogus`, expected one of ...``. If the
command is listed but fails, the notification and the plugin's log say
why (for example an import Arcode refused, or a crash).

## 8. Change, rebuild, reload

Arcode does not watch plugin folders. After a change:

```sh
cargo build --release --target wasm32-wasip2 \
  && cp target/wasm32-wasip2/release/word_count.wasm dist/acme.word-count/plugin.wasm
```

then run **Plugins: Reload** in Arcode. Reload re-reads every manifest and
stops every running plugin, so the next command starts your new code.
Changes to `plugin.toml` also need the copy and a reload.

### Logs

Your plugin has a log of its own:

- `host::log(host::Level::Info, "...")` writes a line;
- `println!` and `eprintln!` (stdout and stderr) also end up there;
- Arcode adds lines for crashes, dropped notifications and permission
  answers.

**Plugins: Show Log** opens every plugin's log in a read-only tab (run it
again to refresh). The **Log** button on your plugin's card in the
Plugins view opens just your plugin's log, followed by its network log.
The log keeps the newest 1 MiB.

## 9. Test it natively

Most plugin code can be tested with a plain `cargo test`, no WebAssembly
needed. In a native build every host function goes to
`arcode_plugin::testing`, a fake Arcode you arrange and inspect. Add to
`src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use arcode_plugin::testing;

    #[test]
    fn counts_words() {
        assert_eq!(words_in("fn main() {\n    println!(\"hi\");\n}\n"), 5);
        assert_eq!(words_in(""), 0);
    }

    #[test]
    fn notifies_with_the_count() {
        testing::reset();
        testing::with(|h| {
            let doc = h.add_document(Some("src/main.rs"), "rust", "one two three");
            h.current_document = Some(doc);
        });
        WordCount.command("count", Json::null()).unwrap();
        let shown = testing::with(|h| h.notifications.clone());
        assert_eq!(shown[0].1, "src/main.rs: 3 words");
    }
}
```

```sh
cargo test
```

[Testing and debugging](testing.md) covers the fake host in detail.

## 10. Where to go next

- Add a panel with a button: [Building UI](ui.md), or look at
  `plugins/hello-world/src/view.rs`.
- Show a code lens above lines: [API reference](api.md#code-lenses).
- Read project files or send requests: [Permissions](permissions.md)
  explains what the user is asked and when.
- Share your plugin: [Publishing](publishing.md).

## Building inside the Arcode repository

Plugins in the repository's `plugins/` folder (a cargo workspace of its
own) are built with a script that also lays out the folder:

```sh
scripts/build-plugins.sh hello-world
# -> target/wasm-plugins/example.hello-world/{plugin.toml,plugin.wasm,README.md}
```

An Arcode binary built in the checkout (`cargo run -p arcode`, or
`./target/release/arcode`) loads everything in `target/wasm-plugins` as if
it shipped with Arcode, so after the script you only need **Plugins:
Reload**. The example is not bundled: a plain `scripts/build-plugins.sh`
never builds it, and you can remove it again with
`rm -rf target/wasm-plugins/example.hello-world`. Set
`ARCODE_WASM_PLUGINS=<folder>` to write the plugin folders somewhere else.
