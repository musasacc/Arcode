# Arcode

A fast, native, open-source IDE written in Rust. No account, no telemetry,
offline by default. Built on [GPUI](https://crates.io/crates/gpui).

Status: early development. See `docs/SPIKE.md` for the GUI stack validation
and `docs/ARCHITECTURE.md` for the crate layout.

## Building

Linux needs a few system libraries:

```bash
sudo apt-get install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libx11-xcb-dev libfontconfig-dev libfreetype-dev mesa-vulkan-drivers
```

Then:

```bash
cargo build --release
```

## License

Licensed under either of Apache License 2.0 (`LICENSE-APACHE`) or MIT
(`LICENSE-MIT`) at your option. The bundled JetBrains Mono font is under the
SIL Open Font License (`assets/fonts/jetbrains-mono/OFL.txt`).
