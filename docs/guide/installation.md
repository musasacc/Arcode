# Installation

Arcode does not have prebuilt packages yet, so you build it from source.
The first build takes a few minutes; later builds are incremental. Arcode
runs on Linux (X11 and Wayland), macOS and Windows. Linux gets the most
testing.

## What you need

On every platform:

- [rustup](https://rustup.rs). The repository pins its Rust version in
  `rust-toolchain.toml`, and rustup installs that version (with the
  `wasm32-wasip2` target used by plugins) on the first `cargo` command.
- `git`. Arcode uses the `git` command for its git features.

### Linux

On Debian and Ubuntu:

```sh
sudo apt-get install build-essential pkg-config \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev \
  libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev \
  libssl-dev libzstd-dev
```

Arcode draws with Vulkan, so you also need a Vulkan driver. Mesa covers
Intel and AMD GPUs (`sudo apt-get install mesa-vulkan-drivers`); NVIDIA's
driver ships its own. Other distributions have the same libraries under
similar names.

### macOS

Install Xcode from the App Store (the Command Line Tools alone are not
enough: the build compiles Metal shaders with `xcrun metal`). Open Xcode
once to accept its license.

### Windows

Install Visual Studio 2022 or the Build Tools for Visual Studio with the
"Desktop development with C++" workload, which brings MSVC and the
Windows SDK. Git for Windows gives you the `bash` shell the scripts in
`scripts/` need.

## Build and run

```sh
git clone https://github.com/musasacc/arcode.git
cd arcode
cargo run -p arcode
```

Open a folder or a file straight away:

```sh
cargo run -p arcode -- path/to/project
cargo run -p arcode -- path/to/file.rs     # opens the file's project and the file
```

Debug builds are pleasant to use: dependencies are built with
optimizations. For a release build:

```sh
cargo build --release -p arcode
./target/release/arcode path/to/project
```

## The bundled plugins

Arcode works without plugins. Two plugins come with it, the **HTTP
Client** and the **Database Viewer**. To build them:

```sh
scripts/build-plugins.sh http-client     # pure Rust, nothing else needed
scripts/build-plugins.sh                 # both bundled plugins
```

The Database Viewer compiles SQLite from C, which needs clang and the
wasi-libc headers: `sudo apt-get install clang llvm wasi-libc` on
Debian/Ubuntu. Elsewhere, unpack the
[WASI SDK](https://github.com/WebAssembly/wasi-sdk/releases) and point
the script at it:

```sh
export WASI_INCLUDE="$HOME/wasi-sdk/share/wasi-sysroot/include/wasm32-wasip2"
export CC_wasm32_wasip2="$HOME/wasi-sdk/bin/clang"
export AR_wasm32_wasip2="$HOME/wasi-sdk/bin/llvm-ar"
```

The plugins land in `target/wasm-plugins/`, where any Arcode binary built
in the checkout (debug or release, `./target/debug/arcode` or
`./target/release/arcode`) finds them. The example plugin in
`plugins/hello-world` is for plugin authors and is not bundled; the script
builds it only when you name it (`scripts/build-plugins.sh hello-world`).

## Installing a build

`scripts/package.sh` builds a release binary and the plugins and lays
them out as a folder you can put anywhere:

```
arcode-<version>-<target>/
  bin/arcode                      (arcode.exe on Windows)
  share/arcode/plugins/<id>/      bundled plugins
  share/doc/arcode/               README and licenses
```

```sh
scripts/package.sh                         # -> target/package/arcode-<version>-<target>/
ARCHIVE=1 scripts/package.sh               # also a .tar.gz (.zip on Windows)
```

Copy the folder's contents to a prefix such as `~/.local`, `/usr/local`
or `/opt/arcode`, and keep `bin/` and `share/` side by side: Arcode looks
for its plugins in `../share/arcode/plugins` next to its binary. On macOS
inside an app bundle, the binary is `Arcode.app/Contents/MacOS/arcode`
and the plugins go to `Arcode.app/Contents/share/arcode/plugins`. More
details are in [docs/PACKAGING.md](../PACKAGING.md).

## Optional tools

Arcode never downloads tools for you. These make it more useful, and the
Languages page tells you which ones are missing:

- **Language servers** for code intelligence, such as `rust-analyzer`,
  `pyright`, `typescript-language-server`, `gopls` or `clangd`. See
  [Languages](languages.md).
- **Debug adapters** for debugging: CodeLLDB or `lldb-dap` (Rust, C,
  C++), GDB 14 or newer, `debugpy` (Python), Delve (Go). See
  [Run and debug](run-and-debug.md).
- The **package managers** your projects use (cargo, npm, uv, ...), for
  the [Packages view](packages.md).

## Updating

```sh
git pull
cargo build --release -p arcode
```

Your settings and data are kept in your user folders (see
[Configuration](configuration.md#where-files-live)), so a new build
picks them up.

## Uninstalling

Delete the binary (or the installed folder), then, if you want, your
settings and data folders listed in
[Configuration](configuration.md#where-files-live).
