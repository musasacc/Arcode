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
- `curl` for the rustup script.

The Linux commands below install `git` and `curl` together with the
build packages.

### Linux

To build Arcode you need a C compiler, pkg-config and the development
packages of xkbcommon (with its X11 part), Wayland, the Vulkan loader,
xcb, fontconfig, FreeType, OpenSSL and zstd. To run it you need an X11
or Wayland session and a **Vulkan driver**: Mesa's drivers cover Intel
and AMD GPUs and NVIDIA's open driver (nouveau), NVIDIA's own driver
ships its own Vulkan driver, and Mesa's software driver, lavapipe, works
on machines without a usable GPU (turn on
[low-end mode](low-end-mode.md) there).

The sections below give the exact commands. They were tested on x86_64
on the versions named in each section: Arcode and its bundled plugins
built, and Arcode started and drew its window under X11 with Mesa's
software Vulkan driver. GPU drivers and Wayland sessions have not been
tested on each of these distributions yet.

Install the packages for your distribution first, then rustup with the
script from [rustup.rs](https://rustup.rs), and load it into the current
shell (or open a new terminal):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
```

On NixOS, skip this: the shell below brings rustup. The first `cargo`
command in the checkout then installs the pinned Rust version, so you do
not need your distribution's Rust packages.

#### Debian and Ubuntu

Tested on Debian 13 (trixie); CI builds on Ubuntu.

```sh
sudo apt-get update
sudo apt-get install build-essential pkg-config git curl \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev \
  libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev \
  libssl-dev libzstd-dev
```

Vulkan driver: `sudo apt-get install mesa-vulkan-drivers`. This one
package has the Intel, AMD and nouveau drivers and lavapipe, and pulls
in the Vulkan loader.

Plugins: `sudo apt-get install clang llvm wasi-libc`. The script finds
them on its own. `llvm` is needed for `llvm-ar`, which the `clang`
package does not include.

#### Fedora

Tested on Fedora 44.

```sh
sudo dnf install gcc pkgconf-pkg-config git curl libxkbcommon-devel \
  libxkbcommon-x11-devel wayland-devel vulkan-loader-devel libX11-devel \
  libxcb-devel fontconfig-devel freetype-devel openssl-devel libzstd-devel
```

Vulkan driver: `sudo dnf install mesa-vulkan-drivers` (Fedora
Workstation already has it). It has the Intel, AMD and nouveau drivers
and lavapipe, and pulls in the Vulkan loader.

Plugins: `sudo dnf install clang llvm wasi-libc-devel`. The script finds
them on its own.

#### Arch Linux

Tested on Arch Linux as of November 2026.

```sh
sudo pacman -Syu --needed base-devel git curl libxkbcommon libxkbcommon-x11 \
  wayland vulkan-icd-loader libx11 libxcb fontconfig freetype2 openssl zstd
```

Arch packages include their headers, so there are no separate `-dev`
packages. `vulkan-icd-loader` is the Vulkan loader.

Vulkan driver: `vulkan-intel` (Intel), `vulkan-radeon` (AMD) or
`vulkan-nouveau` (NVIDIA, open driver); NVIDIA's own driver brings
Vulkan with `nvidia-utils`. Without a usable GPU, install `vulkan-swrast`
(lavapipe).

Plugins: `sudo pacman -Syu --needed clang llvm wasi-libc`. The script finds
them on its own.

#### openSUSE Tumbleweed

Tested on openSUSE Tumbleweed (snapshot 20261030).

```sh
sudo zypper install gcc pkgconf-pkg-config git curl libxkbcommon-devel \
  libxkbcommon-x11-devel wayland-devel vulkan-devel libX11-devel \
  libxcb-devel fontconfig-devel freetype2-devel libopenssl-devel libzstd-devel
```

Vulkan driver: the loader `libvulkan1` plus `libvulkan_intel` (Intel),
`libvulkan_radeon` (AMD) or `libvulkan_nouveau` (NVIDIA, open driver).
Without a usable GPU, install `libvulkan_lvp` (lavapipe):

```sh
sudo zypper install libvulkan1 libvulkan_intel   # or libvulkan_radeon, libvulkan_lvp, ...
```

Plugins: `sudo zypper install clang llvm wasi-libc`. The script finds
them on its own.

#### Void Linux

Tested on Void Linux (glibc) as of November 2026 (Mesa 26.2.6). The musl
variant has not been tested.

```sh
sudo xbps-install -S base-devel git curl libxkbcommon-devel wayland-devel \
  vulkan-loader-devel libX11-devel libxcb-devel fontconfig-devel \
  freetype-devel openssl-devel libzstd-devel
```

`base-devel` includes pkg-config, `libxkbcommon-devel` includes the X11
part, and `libX11-devel` includes X11-xcb.

Vulkan driver: `mesa-vulkan-intel` (Intel), `mesa-vulkan-radeon` (AMD) or
`mesa-vulkan-nouveau` (NVIDIA, open driver); without a usable GPU,
`mesa-vulkan-lavapipe`. These packages do not pull in the Vulkan loader,
so install `vulkan-loader` too (the build packages above already do):

```sh
sudo xbps-install -S vulkan-loader mesa-vulkan-intel   # or -radeon, -nouveau, -lavapipe
```

Plugins: `sudo xbps-install -S clang llvm wasi-libc`. The script finds
them on its own (`llvm` is needed for `llvm-ar`).

#### Alpine Linux

Tested on Alpine Linux 3.24 (x86_64).

```sh
sudo apk add build-base pkgconf git curl libxkbcommon-dev wayland-dev \
  vulkan-loader-dev libx11-dev libxcb-dev fontconfig-dev freetype-dev \
  openssl-dev zstd-dev
```

Alpine uses musl, and rustup's Rust for musl links programs statically
by default. Arcode has to be linked dynamically (it loads Vulkan and
Wayland at run time), so turn that off once before the first build;
otherwise the build fails at its very last step with
`cannot find -lfreetype`:

```sh
mkdir -p ~/.cargo
cat >> ~/.cargo/config.toml <<'EOF'
[target.x86_64-unknown-linux-musl]
rustflags = ["-C", "target-feature=-crt-static"]
EOF
```

If your `~/.cargo/config.toml` already has a
`[target.x86_64-unknown-linux-musl]` section, add the `rustflags` line to
that section instead, because a section may appear only once. On ARM, use
`aarch64-unknown-linux-musl` instead (not tested). Use
rustup rather than Alpine's own `rust` package, which is older than the
version Arcode pins.

Vulkan driver: `mesa-vulkan-intel` (Intel), `mesa-vulkan-ati` (AMD) or
`mesa-vulkan-nouveau` (NVIDIA, open driver); without a usable GPU,
`mesa-vulkan-swrast` (lavapipe). These packages do not pull in the
Vulkan loader, so install `vulkan-loader` too:

```sh
sudo apk add vulkan-loader mesa-vulkan-intel   # or mesa-vulkan-ati, mesa-vulkan-swrast, ...
```

Plugins: `sudo apk add clang llvm wasi-libc bash`. The script finds them
on its own. `bash` is needed because the scripts in `scripts/` are bash
scripts and Alpine has only `sh`.

#### NixOS

The repository has a `shell.nix` with everything needed. Tested with the
nixos-25.11 channel and with nixpkgs-unstable.

```sh
cd arcode
nix-shell                  # Rust (through rustup), pkg-config, git and the libraries
cargo run -p arcode
```

Nothing is installed system-wide. Start Arcode from inside `nix-shell`
(or with the same `LD_LIBRARY_PATH`): it loads Vulkan and Wayland at run
time and finds them only through the shell's `LD_LIBRARY_PATH`.

Vulkan driver: enable the graphics stack in `configuration.nix`, which
installs Mesa's drivers, lavapipe included, where the Vulkan loader looks
for them:

```nix
hardware.graphics.enable = true;   # hardware.opengl.enable before NixOS 24.11
```

NVIDIA's own driver also needs `services.xserver.videoDrivers = [ "nvidia" ];`.
Build Arcode with the same channel as your system: a driver from a newer
nixpkgs than the one Arcode was built with may not load. This setting has
not been tested on a NixOS machine yet; the shell was tested with Mesa's
software driver from the same channel. Using the shell with Nix on
another distribution has not been tested either.

Plugins: `nix-shell --arg withPlugins true --run scripts/build-plugins.sh`.
The shell then also brings clang, `llvm-ar` and the wasi-libc headers and
sets the variables the script needs.

#### Other distributions

Install the development packages of the libraries listed at the top of
this section: xkbcommon and xkbcommon-x11, Wayland (client), the Vulkan
loader, xcb and X11-xcb, fontconfig, FreeType, OpenSSL and zstd, plus
pkg-config and a C compiler (gcc or clang). Then add a Vulkan driver for
your GPU, or Mesa's lavapipe.

For the plugins, install clang, `llvm-ar` and wasi-libc from your
distribution if it has them. If `scripts/build-plugins.sh` does not find
the headers, set `WASI_INCLUDE` to the folder that contains wasi-libc's
`stdio.h`. Otherwise use the WASI SDK as described under
[The bundled plugins](#the-bundled-plugins).

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
scripts/build-plugins.sh                 # both bundled plugins
scripts/build-plugins.sh http-client     # just one (no C toolchain needed)
```

The HTTP Client is pure Rust and needs only the `wasm32-wasip2` target,
which rustup installs from `rust-toolchain.toml`. The Database Viewer
compiles SQLite from C, which also needs clang, `llvm-ar` and the
wasi-libc headers; the script stops with a message naming the missing
pieces if they are not installed. On Linux, the section for your
distribution under [Linux](#linux) names the packages (on Debian/Ubuntu,
`sudo apt-get install clang llvm wasi-libc`), and the script finds them
on its own. Elsewhere, unpack the
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
