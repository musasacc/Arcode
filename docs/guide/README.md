# Arcode user guide

Arcode is a native code editor and IDE written in Rust. It is offline by
default, has no telemetry and no account, and is built to stay fast on
older laptops. This guide explains how to install it and how to use it
day to day.

## Contents

| Page | What it covers |
|---|---|
| [Installation](installation.md) | Building Arcode from source on Linux, macOS and Windows |
| [Getting started](getting-started.md) | Opening a folder, the finder, the command palette, the terminal, running code |
| [Configuration](configuration.md) | Where files live, and every setting in `config.toml` |
| [Key bindings](keybindings.md) | Every default shortcut on macOS and on Linux / Windows |
| [Themes](themes.md) | Creating, editing and writing your own themes; the built-in ones |
| [Languages](languages.md) | Built-in languages, language servers, language packs |
| [Run and debug](run-and-debug.md) | Run buttons, the Run button, debugging, `launch.toml` |
| [Git](git.md) | Change bars, blame, staging, committing, branches, conflicts |
| [Packages](packages.md) | The Packages view and the package managers it supports |
| [Plugins](plugins.md) | Installing, enabling and trusting plugins |
| [Low-end mode](low-end-mode.md) | Settings for older and slower machines |
| [Discord presence](discord.md) | Showing what you are editing in your Discord status (off by default) |
| [Privacy](privacy.md) | Exactly what can use the network, and when |
| [Troubleshooting](troubleshooting.md) | Logs and common problems |

## A two-minute tour

- **Open a project**: Ctrl+O (Cmd+O on macOS), or start Arcode with a
  path: `arcode ~/code/my-app`.
- **Find a file**: Ctrl+P (Cmd+P) and type part of its name.
- **Run any command**: Ctrl+Shift+P (Cmd+Shift+P) or F1 opens the command
  palette. Every feature in this guide is in there, with its shortcut.
- **Open a terminal**: Ctrl+\` focuses the terminal, Ctrl+Shift+\` opens a
  new one.
- **Run the project**: the Run button in the title bar (Ctrl+F5 / Cmd+F5)
  runs the project's detected command, such as `cargo run` or `npm start`.
- **Change settings**: Ctrl+, (Cmd+,) opens the Settings page; everything
  is saved to one `config.toml` file.

On the first start Arcode shows a short setup dialog (theme, font,
keymap, performance) and offers a guided tour of the window. You can see
both again with **Help: Show Welcome Setup** and **Help: Start Tour**.

## Writing plugins

If you want to extend Arcode, see the [plugin author guide](../plugins/README.md).
