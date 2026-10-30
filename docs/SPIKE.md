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

---

# Plugin engine spike (docs/PLUGINS.md §13, Step 0)

Goal: measure what wasmtime costs before building the plugin host on it,
and prove that SQLite builds for `wasm32-wasip2` with this box's toolchain.

Environment: same cloud VM (4 vCPU Xeon @ 2.1 GHz, no GPU), rustc 1.97.0,
clang 18.1, Ubuntu `wasi-libc` (headers in `/usr/include/wasm32-wasi`),
wasmtime / wasmtime-wasi `=50.0.1`, wit-bindgen `=0.63.0`, rusqlite
`=0.41.1` (bundled SQLite 3.54.1).

Method: a throwaway host binary (not committed) built with Arcode's release
profile (`lto = "thin"`, `codegen-units = 1`, opt-level 3), with exactly the
§2 feature set: `wasmtime` `runtime, cranelift, component-model, std`;
`wasmtime-wasi` `p2` only. Three variants, then `strip`:
- baseline: the same `main` with the wasm code compiled out;
- wasmtime only (Engine + Component + Linker, no WASI);
- wasmtime + wasmtime-wasi (the real configuration).

The engine uses the §2 config (epoch interruption, 512 KiB wasm stack,
CoW memory init, on-demand allocator; serial compilation). RSS is `VmRSS`
from `/proc/self/status`. "Cached" runs load a `.cwasm` written earlier by
`Engine::precompile_component` with `Component::deserialize_file`, i.e. the
activation path of §2. The guest for RSS is the `echo` fixture (270 KB; it
links every arcode interface, so it is a fair stand-in for the HTTP client).

## Results

| measure | result | §2 limit |
|---|---|---|
| stripped binary, baseline | 343 KB | |
| stripped, + wasmtime only | 12.12 MB (+11.78 MB) | |
| stripped, + wasmtime + wasmtime-wasi | 15.15 MB (**+14.81 MB**, 14.1 MiB) | 15 MB |
| RSS: `Engine::new` | +1.4 to 1.7 MB | |
| RSS: engine + idle `echo` instance from cache (5 runs) | **+4.6 to 4.9 MB** over process start | 10 MB |
| RSS: engine + SQLite plugin from cache, after one `count(*)` | +9.9 MB | (DB target ~15 MB, §13 Step 12) |
| cold compile, `echo` (270 KB wasm, serial) | 0.29 to 0.40 s, peak RSS ~22 MB | |
| cold compile, SQLite plugin (1.26 MB wasm, serial) | 2.1 s, peak RSS ~53 MB | |
| `.cwasm` size | 770 KB (echo), 4.2 MB (SQLite) | |
| deserialize from cache | 0.3 to 0.5 ms | |
| instantiate | 0.11 to 0.26 ms | |
| first call (`provide-code-lenses`) | 35 to 65 µs (echo), 4 ms (SQLite open + count, cached) | |

Compile time and compile RSS are paid once at install (or first activation
without a cache), on a background thread, never at startup.

**Exit criterion: met, narrowly.** wasmtime + wasmtime-wasi add 14.81 MB
(decimal) to the stripped binary, under the 15 MB bar; idle RSS is about half
the 10 MB bar. About 3 MB of the size comes from wasmtime-wasi and what it
pulls (tokio, cap-std, rustix); Arcode already links tokio, so the delta in
the real app is somewhat lower. We continue with wasmtime. If the size later
creeps past 15 MB, the cheapest levers are `opt-level = "s"` for the
`cranelift-*` crates (most of the 11.8 MB) and keeping the `plugins` cargo
feature off for minimal builds; the engine stays swappable behind
`engine.rs`/`bindings.rs`.

## SQLite for wasm32-wasip2

- `rusqlite` `bundled` builds directly for `wasm32-wasip2` with clang 18:
  ~34 s, 1.26 MB wasm (`opt-level = "s"`, LTO). **Fallback A (p1 + adapter)
  and fallback B (`SQLITE_OS_OTHER` + Rust VFS) were not needed.**
- The Ubuntu package has no `/usr/share/wasi-sysroot`, so the `--sysroot`
  flag planned in §13 does not apply. Worse, clang 18 accepts
  `--target=wasm32-wasip2` but does not know the wasi-libc header dir for
  that triple and quietly falls back to the host's glibc headers. The pinned
  flags are therefore `CFLAGS_wasm32_wasip2="-nostdlibinc -isystem
  /usr/include/wasm32-wasi"`; rustc links its own self-contained wasi-libc.
  `scripts/build-plugins.sh` detects the include dir (Debian/Ubuntu,
  wasi-sdk, Homebrew layouts, or `WASI_INCLUDE`).
- `libsqlite3-sys` already defines `SQLITE_THREADSAFE=0` and the
  `_WASI_EMULATED_*` macros for `wasm32-wasi*`; the remaining §10 flags
  (`SQLITE_OMIT_LOAD_EXTENSION`, `SQLITE_DEFAULT_MEMSTATUS=0`,
  `SQLITE_OMIT_SHARED_CACHE`) go in through `LIBSQLITE3_FLAGS`.
- **Test:** a fixture DB with 1234 rows, preopened read-only at `/project`,
  opened as `file:/project/fixture.db?mode=ro&nolock=1`:
  - as a WASI command component: prints `rows=1234 sqlite=3.54.1`; an
    `INSERT` fails with `attempt to write a readonly database`;
  - as a plugin-world component (reactor, `export_plugin!`, built by
    `scripts/build-plugins.sh`): `provide-code-lenses` returns a lens titled
    `rows=1234`. SQLite reaches the file through wasi-libc's preopen lookup
    (`lstat` → `__wasilibc_find_relpath`), so the WASI preopen is the only
    file path it needs, as §4 intends.

## Findings that change code, not the plan

Recorded in docs/PLUGINS.md, Appendix B: `wasmtime-wasi`'s `p2` feature
enables `wasmtime/async` (sync API still used); `Config::parallel_compilation`
and `Config::wasm_threads` do not exist with the minimal feature set (and are
not needed); `FsPerms` replaced `DirPerms`/`FilePerms`; guests import WASI
`@0.2.9` and `define_unknown_imports_as_traps` must not be used with
semver-matched WASI; every component imports `arcode:plugin/{types,editor,ui}`
because the exports use their types, while unused interfaces such as `http`
are pruned, so the import check can rely on `http`.

## Plugin engine, Steps 3-4 measurements

Same VM, debug test build (`opt-level = 2` for dependencies), fixtures from
`scripts/build-test-plugins.sh` (release, `opt-level = "s"`):

| measure | result |
|---|---|
| fixture sizes | echo 270 KB, fs-escape 175 KB, sockets 220 KB, alloc-bomb 131 KB, crash 117 KB, stdout-flood 114 KB, spin 108 KB, undeclared-import 71 KB, spin-cancellable 69 KB, wasi-http 15 KB |
| store caps a Rust fixture needs | 1 memory, 2 tables, 3 core instances (§5 defaults 2/8/16 kept) |
| `spin`, 300 ms budget, wall time to trap | 312-330 ms (debug host build); up to ~380 ms while other builds load the VM (slices count guest time only) |
| hard cancel, 200 ms grace | trap ~210 ms after cancel |
| `spin-cancellable` | returns 0.15-0.4 ms after cancel (polls `host.cancelled()` every 10k iterations) |
| `alloc-bomb`, 16 MiB limit | `try_reserve` loop stops at 14 MiB of 1 MiB chunks; infallible loop traps as "out of memory"; peak ≤ 16 MiB |
| `stdout-flood`, 4 MiB | log keeps ≤ 1 MiB (newest lines), > 3000 lines dropped, call succeeds |
| idle instance | 0 mailbox wake-ups and 0 epoch ticks over 500 ms; thread state `S` |
| ticker without calls in flight | 0 increments over 1 s |
