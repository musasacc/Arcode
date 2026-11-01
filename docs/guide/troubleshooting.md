# Troubleshooting

## Logs

Arcode writes its log to **standard error**. To see it, start Arcode from
a terminal:

```sh
arcode ~/code/my-app
```

By default only warnings and errors are logged. Set `RUST_LOG` for more:

```sh
RUST_LOG=info arcode ~/code/my-app                      # general information
RUST_LOG=arcode=debug arcode ~/code/my-app              # the application itself in detail
RUST_LOG=arcode_lsp=debug arcode ~/code/my-app          # language server traffic
RUST_LOG=arcode_plugin_host=debug arcode ~/code/my-app  # plugin loading and calls
```

To keep it in a file: `arcode ~/code/my-app 2> arcode.log`.

What you find there:

- warnings about `config.toml` (unknown keys, wrong types, syntax errors)
  and about theme files, prefixed with `config:` and `theme:`;
- language servers that could not start;
- plugin folders that were skipped and why.

Two more logs are inside Arcode:

- **Plugins: Show Log**: every plugin's log, and the plugin folders that
  were skipped with the reason.
- The **debug console** in the bottom panel: build output, program output
  and debug adapter messages during a debug session.

## Common problems

### The window is black, or Arcode does not start

Arcode draws with Vulkan on Linux and Windows. Make sure a Vulkan driver
is installed: on Debian/Ubuntu and Fedora, `mesa-vulkan-drivers` has
Mesa's Intel, AMD and nouveau drivers; NVIDIA's own driver brings its
own. The [installation guide](installation.md#linux) names the packages
for other distributions. `vulkaninfo` shows whether Vulkan works at all;
it is in the package `vulkan-tools` (`Vulkan-Tools` on Void Linux). In a
virtual machine without GPU support, Mesa's software driver, lavapipe,
works; turn on [low-end mode](low-end-mode.md) there. Its package is
named in the installation guide for each distribution (on Debian/Ubuntu
and Fedora it is part of `mesa-vulkan-drivers`).

When Arcode stops right at startup on Linux with
`Unable to init GPU context` in the log (after
`Failed to initialize X11 client` in an X11 session), the rest of the
message tells you what is missing:

- It names `libvulkan.so.1`: the Vulkan loader is missing. On Void Linux
  and Alpine Linux the Mesa driver packages do not pull it in; install
  `vulkan-loader`. On NixOS, start Arcode from inside `nix-shell`.
- It ends in `NoSupportedDeviceFound`: the loader is there, but no Vulkan
  driver is installed for your GPU. Install one, or Mesa's software
  driver.

### `not found; needed to compile the C code in db-viewer`

`scripts/build-plugins.sh` stops with an error that ends this way when it
is about to build the Database Viewer (`db-viewer`, which compiles SQLite
from C) and cannot find what that needs: clang, `llvm-ar` and the
wasi-libc headers. The start of the first line lists the pieces it did not
find, for example:

```text
error: wasi-libc headers not found; needed to compile the C code in db-viewer.
error: clang, llvm-ar not found; needed to compile the C code in db-viewer.
error: wasi-libc headers, clang, llvm-ar not found; needed to compile the C code in db-viewer.
```

Running the script without arguments builds the Database Viewer too. The
HTTP Client is pure Rust and builds without them:
`scripts/build-plugins.sh http-client`.

If `WASI_INCLUDE` is set to a folder without wasi-libc's `stdio.h`, for
example a mistyped path or one from an older setup, the script reports the
headers as not found and adds a line naming that folder. Correct the path,
or unset `WASI_INCLUDE` so the script looks in the usual places.

Install the packages named for your distribution under
[Linux](installation.md#linux), for example on Fedora:

```sh
sudo dnf install clang llvm wasi-libc-devel
```

The script finds the headers where the distributions in the installation
guide put them. If yours keeps them somewhere else, look for wasi-libc's
`stdio.h` and set `WASI_INCLUDE` to the folder that contains it:

```sh
find / -path '*wasm32*' -name stdio.h 2>/dev/null
WASI_INCLUDE=/path/to/that/folder scripts/build-plugins.sh
```

With the WASI SDK instead of distribution packages, set `WASI_INCLUDE`,
`CC_wasm32_wasip2` and `AR_wasm32_wasip2` as described under
[The bundled plugins](installation.md#the-bundled-plugins).

### Alpine Linux: the build fails at the end with `cannot find -lfreetype`

The final link step fails with `-static-pie` in the command line and
`ld: cannot find -lfreetype`, although `freetype-dev` is installed.
rustup's Rust for musl links statically by default; Arcode has to be
linked dynamically. Add the setting from
[Alpine Linux](installation.md#alpine-linux) to `~/.cargo/config.toml`
and build again.

### NixOS: Arcode does not start outside `nix-shell`

Arcode loads the Vulkan and Wayland libraries at run time and finds them
through the shell's `LD_LIBRARY_PATH`. Outside the shell it does not find
them; in a Wayland session it stops with `NoWaylandLib`. Start it from
inside `nix-shell`. If it then stops with `NoSupportedDeviceFound`,
check that `hardware.graphics.enable = true;` is set and that Arcode was
built with the same nixpkgs channel as your system (see
[NixOS](installation.md#nixos)).

### My settings are ignored

- A TOML syntax error makes Arcode ignore the whole file until it is
  fixed. Open it with **Preferences: Open Settings File (config.toml)**
  and look for the error, or check the log for
  `config: config is not valid TOML`.
- An unknown key or a value of the wrong type is ignored on its own and
  logged as a warning. Compare with the
  [default file](../../crates/arcode-config/assets/default-config.toml)
  and [Configuration](configuration.md).
- Check that you are editing the right file: `ARCODE_CONFIG_DIR` changes
  where it is.

### A language server does not work

- The status bar lists the language servers of the current file and
  marks missing ones. Open the Languages page (**Languages: Show Language
  Packs**) to see what is installed and how to install the rest.
- Arcode finds servers on `PATH`. If you start Arcode from a desktop
  launcher or the macOS Dock, it may not see the `PATH` of your shell
  (for example `~/.cargo/bin` or a Node version manager's folder). Start
  Arcode from a terminal, or set the full path:

  ```toml
  [lsp.rust-analyzer]
  command = "/home/me/.cargo/bin/rust-analyzer"
  ```

- Check that the language pack is not turned off
  (`[language.<id>] enabled = false`).
- `RUST_LOG=arcode_lsp=debug` shows what is sent to and received from
  servers.

### No debug adapter was found

Install one of the adapters for your language (see
[Run and debug](run-and-debug.md#debug-adapters)) or point Arcode at it
with `[debug] adapter_paths`. GDB must be version 14 or newer.

### A theme does not show up

The theme picker reads the `themes` folder each time it opens. If your
theme is missing, the log has a `theme:` warning saying why (for example
a missing `appearance` or a file that is not valid TOML).

### Characters show as boxes

Add a font that has them to `editor.font_fallbacks`, for example a CJK
or emoji font installed on your system.

### The terminal uses the wrong shell

Set it in `config.toml`:

```toml
[terminal]
shell = "/usr/bin/fish"
args = ["-l"]
```

### A plugin does not appear, or stopped

- Not listed at all: its manifest was refused; **Plugins: Show Log** ends
  with the reason.
- "<Plugin> stopped: ...": it ran out of CPU time or memory, or crashed.
  It restarts on the next action; after three crashes in ten minutes it
  stays off until you click **Restart** on its card.
- It cannot reach a host: check its Permissions section in the Plugins
  view for a remembered "Deny", and `[plugins] network`.

### Arcode is slow on my machine

Turn on [low-end mode](low-end-mode.md). `ui.show_perf_stats` shows frame
time and memory use in the status bar, which helps to see what makes a
difference.

## Starting fresh

To rule out your settings, start Arcode with empty folders:

```sh
ARCODE_CONFIG_DIR=/tmp/arcode-config ARCODE_DATA_DIR=/tmp/arcode-data arcode
```

This shows the first-run dialog and uses no saved state, permissions or
installed plugins. Your real folders stay untouched. To reset for good,
move or delete the folders listed in
[Configuration](configuration.md#where-files-live).

## Reporting a problem

Open an issue on the project's GitHub page with:

- your operating system, and the graphics driver on Linux;
- how you built Arcode (the commit, debug or release);
- what you did, what you expected, and what happened;
- the log, started with `RUST_LOG=info`, if it shows anything related.

Please check the log for paths or file contents you do not want to share
before you attach it.
