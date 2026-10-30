#!/usr/bin/env bash
# Release-builds the bundled plugins in plugins/ for wasm32-wasip2 and lays
# each one out as a plugin folder (docs/PLUGINS.md §3, §12):
#
#   target/wasm-plugins/<id>/{plugin.toml,plugin.wasm[,README.md,LICENSE]}
#
# Packages (crate name -> plugin id):
#   http-client -> arcode.http-client   (§9; pure Rust, ~390 KB)
#   db-viewer   -> arcode.db-viewer     (§10; bundles SQLite 3.54, needs
#                                        clang + wasi-libc headers; ~1.7 MB)
# arcode-plugin-host's end-to-end tests (tests/sandbox/http_client,
# tests/db_viewer, ...) load these folders, so run this before them. Native
# unit tests of the plugins need no wasm (the DB viewer's compile SQLite
# natively): cargo test --manifest-path plugins/Cargo.toml -p <crate>
#
# C dependencies (SQLite via rusqlite/bundled for the DB viewer) are compiled
# with clang and the wasi-libc headers. The flags below were pinned by the
# Step 0 spike (docs/SPIKE.md, "Plugin engine"):
#   - clang 18 accepts `--target=wasm32-wasip2` but does not know where the
#     wasi-libc headers live for that triple and silently falls back to the
#     host's glibc headers, so we pass -nostdlibinc plus an explicit
#     -isystem to the wasi-libc include dir. Only headers are used: rustc
#     links against its own self-contained wasi-libc.
#   - SQLite flags: libsqlite3-sys already sets SQLITE_THREADSAFE=0 for
#     wasm32-wasi*; we add the rest from §10 through LIBSQLITE3_FLAGS.
#
# Environment overrides:
#   WASI_INCLUDE           wasi-libc include dir (auto-detected)
#   CC_wasm32_wasip2       C compiler (default: clang-18 or clang)
#   AR_wasm32_wasip2       archiver   (default: llvm-ar-18 or llvm-ar)
#   ARCODE_PLUGINS_DIR     cargo workspace to build (default: plugins/)
#   ARCODE_WASM_PLUGINS    output dir (default: target/wasm-plugins)
#   ARCODE_WASM_TARGET_DIR cargo target dir (default: target/wasm-plugins-build)
#
# Usage: scripts/build-plugins.sh [package...]   (default: all members)
#   e.g. scripts/build-plugins.sh http-client
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
plugins="${ARCODE_PLUGINS_DIR:-$root/plugins}"
out="${ARCODE_WASM_PLUGINS:-$root/target/wasm-plugins}"
target_dir="${ARCODE_WASM_TARGET_DIR:-$root/target/wasm-plugins-build}"

if [ ! -f "$plugins/Cargo.toml" ]; then
    echo "no plugin workspace at $plugins; nothing to build"
    exit 0
fi

first_of() { for c in "$@"; do if command -v "$c" >/dev/null 2>&1; then echo "$c"; return; fi; done; }

if [ -z "${WASI_INCLUDE:-}" ]; then
    for dir in \
        /usr/share/wasi-sysroot/include/wasm32-wasip2 \
        /usr/share/wasi-sysroot/include/wasm32-wasi \
        /usr/share/wasi-sysroot/include \
        /usr/include/wasm32-wasi \
        /opt/homebrew/share/wasi-sysroot/include/wasm32-wasi \
        /opt/wasi-sdk/share/wasi-sysroot/include/wasm32-wasip2 \
        /opt/wasi-sdk/share/wasi-sysroot/include/wasm32-wasi; do
        if [ -f "$dir/stdio.h" ]; then WASI_INCLUDE="$dir"; break; fi
    done
fi
if [ -z "${WASI_INCLUDE:-}" ]; then
    echo "error: wasi-libc headers not found; install wasi-libc (apt install wasi-libc) or set WASI_INCLUDE" >&2
    exit 1
fi

export CC_wasm32_wasip2="${CC_wasm32_wasip2:-$(first_of clang-18 clang)}"
export AR_wasm32_wasip2="${AR_wasm32_wasip2:-$(first_of llvm-ar-18 llvm-ar)}"
export CFLAGS_wasm32_wasip2="${CFLAGS_wasm32_wasip2:--nostdlibinc -isystem $WASI_INCLUDE}"
export LIBSQLITE3_FLAGS="${LIBSQLITE3_FLAGS:--DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_DEFAULT_MEMSTATUS=0 -DSQLITE_OMIT_SHARED_CACHE}"

packages=()
for p in "$@"; do packages+=(-p "$p"); done

CARGO_TARGET_DIR="$target_dir" cargo build \
    --manifest-path "$plugins/Cargo.toml" \
    --target wasm32-wasip2 --release --locked "${packages[@]}"

mkdir -p "$out"
for manifest in "$plugins"/*/plugin.toml; do
    [ -f "$manifest" ] || continue
    dir="$(dirname "$manifest")"
    crate="$(basename "$dir")"
    if [ "$#" -gt 0 ] && [[ ! " $* " == *" $crate "* ]]; then continue; fi
    id="$(sed -n 's/^id[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
    wasm="$target_dir/wasm32-wasip2/release/${crate//-/_}.wasm"
    if [ -z "$id" ] || [ ! -f "$wasm" ]; then
        echo "error: $crate: missing plugin id or $wasm" >&2
        exit 1
    fi
    rm -rf "${out:?}/$id"
    mkdir -p "$out/$id"
    cp "$manifest" "$out/$id/plugin.toml"
    cp "$wasm" "$out/$id/plugin.wasm"
    for extra in README.md LICENSE; do
        if [ -f "$dir/$extra" ]; then cp "$dir/$extra" "$out/$id/"; fi
    done
    printf '%-24s %9d bytes\n' "$id" "$(wc -c < "$out/$id/plugin.wasm")"
done
