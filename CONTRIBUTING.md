# Contributing to Arcode

Thanks for your interest in Arcode. Bug reports, fixes, features,
documentation and plugins are all welcome. This page explains how to set
up a development environment, what the checks are, and how to get a
change merged.

## Ground rules

Arcode has a few rules every change has to respect. They are why the
project exists, so they are not up for trade-offs in a single pull
request:

- **No telemetry, no accounts, no network access the user did not ask
  for.** Nothing may phone home, check for updates or download tools on
  its own. Network use must follow a user action and be visible.
- **Idle means idle.** No permanent frame loop, no polling timers. Views
  redraw when their state changes; background work wakes the UI through
  channels.
- **Every crate below the app builds and tests without GPUI.** Only
  `arcode`, `arcode-editor`, `arcode-terminal-view`, `arcode-ui-kit` and
  `arcode-spike` may depend on it.
- **Cross-platform.** Linux, macOS and Windows. Platform-specific code is
  behind `cfg` and has a fallback.
- **Plugins stay sandboxed.** A plugin must never gain access the user
  did not grant, and nothing inside a project may grant it.

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how the crates fit
together and how each feature is built. Read its first section before a
larger change.

## Development setup

You need [rustup](https://rustup.rs) and `git`, plus the platform
libraries listed in [Installation](docs/guide/installation.md#what-you-need)
(on Debian/Ubuntu: the X11, Wayland, Vulkan and font development
packages; the guide has the commands for Fedora, Arch, openSUSE, Void,
Alpine and NixOS). `rust-toolchain.toml` pins the Rust version, the `clippy` and
`rustfmt` components and the `wasm32-wasip2` target; rustup installs
them on the first `cargo` command.

For the Database Viewer plugin you also need, on Debian/Ubuntu,
`sudo apt-get install clang llvm wasi-libc` (it compiles SQLite with
them; the other plugins are pure Rust and build without them), the
matching packages on other distributions (see
[Installation](docs/guide/installation.md#linux)), or the
[WASI SDK](https://github.com/WebAssembly/wasi-sdk/releases) elsewhere;
see the header of `scripts/build-plugins.sh`.

```sh
git clone https://github.com/musasacc/arcode.git
cd arcode
cargo run -p arcode -- path/to/some/project
```

Useful while developing:

- `ARCODE_CONFIG_DIR=/tmp/a-cfg ARCODE_DATA_DIR=/tmp/a-data cargo run -p arcode`
  runs with empty settings and state, leaving your own untouched.
- `ARCODE_SKIP_FIRST_RUN=1` skips the first-run dialog.
- `RUST_LOG=arcode=debug` (or `arcode_lsp=debug`, `arcode_plugin_host=debug`)
  turns on detailed logs on stderr.
- `ARCODE_LOG_FRAMES=1` prints how many frames were drawn in each second
  that had any, so you can check that an idle window draws nothing.

## Building

```sh
cargo build --workspace --locked          # every crate
cargo run -p arcode                       # the app (debug build; dependencies are optimized)
cargo build --release -p arcode           # release build
```

The repository holds three cargo workspaces:

| Workspace | What | Built by |
|---|---|---|
| `Cargo.toml` (root) | the app and its crates in `crates/` | `cargo` |
| `plugins/Cargo.toml` | the bundled plugins and the example plugin, for `wasm32-wasip2` | `scripts/build-plugins.sh` |
| `crates/arcode-plugin-host/test-programs/Cargo.toml` | small fixture plugins for the plugin host's tests | `scripts/build-test-plugins.sh` |

The root workspace excludes the other two, so a plain `cargo build` or
`cargo test` never builds WebAssembly.

```sh
scripts/build-plugins.sh                  # bundled plugins -> target/wasm-plugins/
scripts/build-plugins.sh http-client      # one plugin
scripts/build-plugins.sh hello-world      # the example plugin (never built by default)
scripts/build-test-plugins.sh             # fixtures -> target/test-plugins/
scripts/package.sh                        # release layout -> target/package/ (see docs/PACKAGING.md)
scripts/fetch-language-servers.sh linux-x64 ~/.local/share/arcode   # bundled language servers for a source build
```

## Running the tests

What CI runs, in this order:

```sh
cargo build --workspace --locked
scripts/build-test-plugins.sh             # the plugin host's tests load these
scripts/build-plugins.sh                  # ... and these
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

On Linux, CI runs the tests through `scripts/check-test-leaks.py`, which
fails when a test leaves a process running (for example a fake language
server that was never killed):

```sh
python3 -I scripts/check-test-leaks.py cargo test --workspace --locked
```

The plugins have native unit tests of their own, run without any
WebAssembly:

```sh
cargo test --manifest-path plugins/Cargo.toml
cargo test --manifest-path plugins/Cargo.toml -p hello-world
```

Tips:

- `cargo test -p <crate>` runs one crate. Most crates have no GPUI
  dependency and test quickly.
- Plugin host tests that need a fixture fail with a message telling you
  which script to run first.
- Tests never touch the external network. HTTP tests use a loopback
  server.
- GUI behaviour of the app is tested with GPUI's test platform
  (`crates/arcode/src/**/*_tests.rs`), headless.

## Code style

- **Format** with `rustfmt` (`cargo fmt --all`). The edition is 2024.
- **Lint** with clippy; warnings are errors in CI
  (`cargo clippy --workspace --all-targets -- -D warnings`).
- **Small modules.** Aim for files under 400 lines, split by concern.
  Large tests go in a sibling `*_tests.rs` file.
- **Tests for new logic.** Put logic where it can be tested without a
  window (a UI-agnostic crate, or a pure function next to the view) and
  test it. A bug fix comes with a test that fails without the fix.
- **Comments say why**, not what. Public items get a doc comment. Keep
  comments in plain English and up to date when the code changes.
- **No panics on user data.** Files, configuration, plugin input and
  language server responses can be anything; return errors or fall back
  to defaults.
- **Never block the UI thread** on I/O, processes or plugins. Use the
  background executor and channels.
- **Dependencies**: think twice before adding one, prefer ones that are
  already in `Cargo.lock`, and keep default features off where they pull
  in things we do not need. GPUI, wasmtime and wit-bindgen are pinned to
  exact versions; bump them in a change of their own.
- **User-facing text** (commands, messages, settings): short, plain
  sentences. Palette commands are named `Category: Action`.

## Proposing a change

1. For anything bigger than a small fix, open an issue first and describe
   what you want to change and why. It saves you work if the direction
   needs discussion.
2. Fork the repository and create a branch from the default branch.
3. Make the change in focused commits with clear messages: a short
   summary line, then what changed and why.
4. Run the checks above. Update the documentation when behaviour,
   settings, commands or key bindings change.
5. Open a pull request that says what the change does, how you tested it
   (and on which operating system), and, for UI changes, a screenshot.
6. A maintainer reviews it. Expect questions and requests for changes;
   that is normal, not a rejection.

Bug reports are most useful with the operating system, how Arcode was
built, steps to reproduce, and the relevant part of the log (start
Arcode with `RUST_LOG=info`). See
[Troubleshooting](docs/guide/troubleshooting.md#reporting-a-problem).

## Where documentation lives

| What | Where |
|---|---|
| Overview, build instructions, feature list | `README.md` |
| User guide | `docs/guide/` |
| Plugin author guide | `docs/plugins/` |
| Example plugin to copy | `plugins/hello-world/` |
| Architecture and internals | `docs/ARCHITECTURE.md` |
| Plugin system design | `docs/PLUGINS.md` |
| Debugger and `launch.toml` reference | `docs/DEBUGGING.md` |
| Visual design and theme tokens | `docs/DESIGN.md`, `docs/UI_SPEC.md` |
| Release layout | `docs/PACKAGING.md` |
| Default settings, with comments | `crates/arcode-config/assets/default-config.toml` |
| The plugin API (WIT) | `crates/arcode-plugin-api/wit/arcode-plugin.wit` |

A change to settings should also update `default-config.toml` and
[docs/guide/configuration.md](docs/guide/configuration.md); a change to
key bindings, [docs/guide/keybindings.md](docs/guide/keybindings.md); a
change to the plugin API, the pages in `docs/plugins/`.

## Contributing a plugin

Plugins do not need to live in this repository; see
[Publishing](docs/plugins/publishing.md). A plugin proposed for bundling
with Arcode should be generally useful, ask only for the permissions it
needs, and come with native tests. Start from `plugins/hello-world` and
the [plugin author guide](docs/plugins/README.md).

## License

Arcode is dual-licensed under the MIT license and the Apache License,
Version 2.0. Unless you explicitly state otherwise, any contribution you
intentionally submit for inclusion in Arcode, as defined in the Apache-2.0
license, is dual-licensed as above, without any additional terms or
conditions.
