#!/usr/bin/env bash
# Release-builds the plugins in plugins/ for wasm32-wasip2 and lays each one
# out as a plugin folder (docs/PLUGINS.md §3, §12):
#
#   target/wasm-plugins/<id>/{plugin.toml,plugin.wasm[,README.md,LICENSE]}
#
# Packages (crate name -> plugin id):
#   http-client -> arcode.http-client   (§9; pure Rust, ~390 KB)
#   db-viewer   -> arcode.db-viewer     (§10; bundles SQLite 3.54, needs
#                                        clang + wasi-libc headers; ~1.7 MB)
#   hello-world -> example.hello-world  (the example plugin to copy; pure Rust;
#                                        not bundled, built only when named)
# arcode-plugin-host's end-to-end tests (tests/sandbox/http_client,
# tests/db_viewer, ...) load these folders, so run this before them. Native
# unit tests of the plugins need no wasm (the DB viewer's compile SQLite
# natively): cargo test --manifest-path plugins/Cargo.toml -p <crate>
#
# C dependencies (SQLite via rusqlite/bundled for the DB viewer) are compiled
# with clang, llvm-ar and the wasi-libc headers. They are needed only when a
# plugin being built has C code (rusqlite, libsqlite3-sys or cc in its
# Cargo.toml); pure Rust plugins such as http-client build without them.
# When they are installed, they are set up for every build, so C code that
# comes in through another dependency is compiled the same way.
# The headers are found where the distributions put them (Debian's
# /usr/include/wasm32-wasi, /usr/share/wasi-sysroot/include/<triple> on Arch
# and openSUSE, Fedora's /usr/wasm32-wasi/include/wasm32-wasi) and in
# /opt/wasi-sdk; see the list below.
# The flags below come from the toolchain tests in docs/SPIKE.md, "Plugin engine":
#   - clang 18 accepts `--target=wasm32-wasip2` but does not know where the
#     wasi-libc headers live for that triple and silently falls back to the
#     host's glibc headers, so we pass -nostdlibinc plus an explicit
#     -isystem to the wasi-libc include dir. Only headers are used: rustc
#     links against its own self-contained wasi-libc.
#   - SQLite flags: libsqlite3-sys already sets SQLITE_THREADSAFE=0 for
#     wasm32-wasi*; we add the rest from §10 through LIBSQLITE3_FLAGS.
#
# Environment overrides:
#   WASI_INCLUDE           wasi-libc include dir, the folder with stdio.h
#                          (auto-detected; set it for the WASI SDK or an
#                          unusual layout)
#   CC_wasm32_wasip2       C compiler (default: clang-18 or clang)
#   AR_wasm32_wasip2       archiver   (default: llvm-ar-18 or llvm-ar)
#   ARCODE_PLUGINS_DIR     cargo workspace to build (default: plugins/)
#   ARCODE_WASM_PLUGINS    output dir (default: target/wasm-plugins)
#   ARCODE_WASM_TARGET_DIR cargo target dir (default: target/wasm-plugins-build)
#
# Usage: scripts/build-plugins.sh [package...]
#   With no arguments it builds the plugins that ship with Arcode: every
#   member except examples (plugin ids starting with `example.`). Name a
#   package to build just that one, examples included, e.g.
#   scripts/build-plugins.sh http-client
#   scripts/build-plugins.sh hello-world
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

# A WASI_INCLUDE set by hand is used as given, but it has to hold
# wasi-libc's stdio.h like the auto-detected folders do.
wasi_include_bad=""
if [ -n "${WASI_INCLUDE:-}" ]; then
    if [ ! -f "$WASI_INCLUDE/stdio.h" ]; then
        wasi_include_bad="$WASI_INCLUDE"
        WASI_INCLUDE=""
    fi
else
    for dir in \
        /usr/share/wasi-sysroot/include/wasm32-wasip2 \
        /usr/share/wasi-sysroot/include/wasm32-wasip1 \
        /usr/share/wasi-sysroot/include/wasm32-wasi \
        /usr/share/wasi-sysroot/include \
        /usr/wasm32-wasi/include/wasm32-wasi \
        /usr/include/wasm32-wasi \
        /opt/homebrew/share/wasi-sysroot/include/wasm32-wasi \
        /opt/wasi-sdk/share/wasi-sysroot/include/wasm32-wasip2 \
        /opt/wasi-sdk/share/wasi-sysroot/include/wasm32-wasi; do
        if [ -f "$dir/stdio.h" ]; then WASI_INCLUDE="$dir"; break; fi
    done
fi

plugin_id() { sed -n 's/^id[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$1" | head -n 1; }

# The crates to build: the ones named, or every plugin except examples.
crates=()
if [ "$#" -gt 0 ]; then
    crates=("$@")
else
    for manifest in "$plugins"/*/plugin.toml; do
        [ -f "$manifest" ] || continue
        case "$(plugin_id "$manifest")" in
            example.*) ;;
            *) crates+=("$(basename "$(dirname "$manifest")")") ;;
        esac
    done
fi
if [ "${#crates[@]}" -eq 0 ]; then
    echo "no plugins in $plugins; nothing to build"
    exit 0
fi
# Plugins with C code (SQLite in the DB viewer) need clang and the
# wasi-libc headers; pure Rust plugins build without them. A plugin counts
# as having C code when its Cargo.toml depends on rusqlite, libsqlite3-sys
# or cc, either as `name = ...` / `name.workspace = true` or as a
# [dependencies.name] table.
c_deps='(rusqlite|libsqlite3-sys|cc)'
needs_c=()
for c in "${crates[@]}"; do
    if grep -qE "^${c_deps}[.[:space:]=]|^\[(target\..+\.)?(build-)?dependencies\.${c_deps}\]" \
        "$plugins/$c/Cargo.toml" 2>/dev/null; then
        needs_c+=("$c")
    fi
done
cc_bin="${CC_wasm32_wasip2:-$(first_of clang-18 clang)}"
ar_bin="${AR_wasm32_wasip2:-$(first_of llvm-ar-18 llvm-ar)}"
if [ "${#needs_c[@]}" -gt 0 ]; then
    missing=()
    if [ -z "${WASI_INCLUDE:-}" ]; then missing+=("wasi-libc headers"); fi
    if [ -z "$cc_bin" ]; then missing+=("clang"); fi
    if [ -z "$ar_bin" ]; then missing+=("llvm-ar"); fi
    if [ "${#missing[@]}" -gt 0 ]; then
        list="$(printf '%s, ' "${missing[@]}")"
        echo "error: ${list%, } not found; needed to compile the C code in ${needs_c[*]}." >&2
        if [ -n "$wasi_include_bad" ]; then
            echo "WASI_INCLUDE is set to $wasi_include_bad, but that folder has no stdio.h." >&2
        fi
        echo "Install clang, llvm and wasi-libc (Fedora: wasi-libc-devel). If the headers are" >&2
        echo "elsewhere, set WASI_INCLUDE to the folder that holds their stdio.h." >&2
        echo "See docs/guide/installation.md." >&2
        exit 1
    fi
fi
# Set up the C toolchain whenever all of it is there, not only for the
# plugins found above, so C code that a plugin pulls in through another
# dependency is also compiled against the wasi-libc headers.
if [ -n "${WASI_INCLUDE:-}" ] && [ -n "$cc_bin" ] && [ -n "$ar_bin" ]; then
    export CC_wasm32_wasip2="$cc_bin"
    export AR_wasm32_wasip2="$ar_bin"
    export CFLAGS_wasm32_wasip2="${CFLAGS_wasm32_wasip2:--nostdlibinc -isystem $WASI_INCLUDE}"
    export LIBSQLITE3_FLAGS="${LIBSQLITE3_FLAGS:--DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_DEFAULT_MEMSTATUS=0 -DSQLITE_OMIT_SHARED_CACHE}"
fi

packages=()
for c in "${crates[@]}"; do packages+=(-p "$c"); done

CARGO_TARGET_DIR="$target_dir" cargo build \
    --manifest-path "$plugins/Cargo.toml" \
    --target wasm32-wasip2 --release --locked "${packages[@]}"

mkdir -p "$out"
for manifest in "$plugins"/*/plugin.toml; do
    [ -f "$manifest" ] || continue
    dir="$(dirname "$manifest")"
    crate="$(basename "$dir")"
    if [[ ! " ${crates[*]} " == *" $crate "* ]]; then continue; fi
    id="$(plugin_id "$manifest")"
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
