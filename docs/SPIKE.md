# Step 0: GPUI validation spike

Goal: decide whether GPUI (`gpui = "=0.2.2"` from crates.io) is a viable GUI
stack for Arcode, by opening and scrolling a 100k-line file smoothly, with
attention to machines without a capable GPU.

Run it:

```bash
cargo run -p arcode-spike --release                 # 100k generated lines
cargo run -p arcode-spike --release -- path/to/file  # any file
cargo run -p arcode-spike --release -- --bench       # 600-frame scroll benchmark
```

## Results

### Linux (measured here)

Environment: cloud VM, 4 vCPU, **no GPU**. Xvfb + openbox, Vulkan provided
by Mesa **llvmpipe/lavapipe (pure software rendering)**. Debug build of the
spike (dependencies at opt-level 2), with other compile jobs competing for
the CPU.

| scenario | result |
|---|---|
| bench, 100k ASCII lines, jump 150 lines/frame | 61 fps avg, p50 16.0 ms, p95 18.5 ms, p99 32 ms |
| bench, 100k lines with accents, CJK, symbols (font fallback) | 62 fps avg, p50 16.0 ms, p95 16.1 ms, p99 16.2 ms |
| first frame after process start | 106-151 ms |
| idle CPU (window open, nothing changing) | ~0.6% of one core; render count stays flat |
| RSS | ~150 MB (includes the llvmpipe software rasterizer) |
| mouse-wheel scrolling (xdotool) | works |
| CJK / symbol fallback fonts | works (system fallback via cosmic-text/fontconfig) |

GPUI only re-renders when a view is dirty (`cx.notify()`), so an idle editor
does no work beyond the platform's vsync tick.

**Software rendering verdict:** GPUI on Linux requires Vulkan, but Mesa's
lavapipe is a complete software Vulkan driver and GPUI runs at 60 fps on it.
That covers GPU-less machines, VMs, and old iGPUs without a Vulkan driver,
as long as Mesa is installed (it is on every mainstream distro). No OpenGL
backend exists in GPUI; lavapipe is the fallback.

### Windows (not measured here)

GPUI 0.2.2 renders with Direct3D 11 (feature level 10.1+) on Windows. DXGI
exposes the Microsoft Basic Render Driver (WARP, a software rasterizer) as an
adapter, so machines without a usable GPU have a software path. Build is
verified in CI; runtime performance on an iGPU must be checked on hardware.

### macOS (not measured here)

Metal, available on every Mac that can run current macOS. Build is verified
in CI; runtime performance must be checked on hardware.

## Issues found

1. **`libc` 0.2.190+ breaks `xattr` 0.2.3** (a transitive dependency of
   gpui's http client): `ENOATTR` was removed for Linux. Pinned `libc` to
   0.2.189 in `Cargo.lock`; CI builds with `--locked`.
2. **No window manager → only the first frame is drawn (X11).** GPUI's X11
   backend starts its frame timer from `MapNotify`; on a bare X server with no
   window manager it never ran. Any normal desktop has a WM, so this only
   matters for headless testing (we run openbox under Xvfb).
3. GPUI's Linux backend logs `Found no xinput mouse pointers` on Xvfb;
   harmless.

## What could not be tested here

- Real hardware: an old laptop with an integrated GPU, macOS, Windows and
  Wayland compositors. These need a manual run of `--bench` on that hardware.

## Decision

**GPUI stays.** It meets the scroll and idle-CPU goals even on pure software
rendering. The Lapce fork fallback is not needed.
