#!/usr/bin/env bash
# Lays out a release of Arcode as an installable prefix (docs/PACKAGING.md):
#
#   <out>/bin/arcode[.exe]
#   <out>/share/arcode/plugins/<id>/{plugin.toml,plugin.wasm[,README.md,LICENSE]}
#   <out>/share/doc/arcode/{README.md,LICENSE-MIT,LICENSE-APACHE}
#
# The app finds its bundled plugins at `<exe>/../share/arcode/plugins`
# (arcode_plugin_host::paths::bundled_roots), so the prefix can be moved or
# installed anywhere (/usr/local, /opt/arcode, a zip on Windows, the
# Contents/ folder of a macOS .app with the binary in Contents/MacOS).
#
# Steps: `cargo build --release -p arcode`, then scripts/build-plugins.sh
# with ARCODE_WASM_PLUGINS pointing into the prefix (needs the wasm32-wasip2
# target, clang and the wasi-libc headers; see that script), then the docs.
# Nothing is downloaded beyond what cargo fetches for the build.
#
# Environment:
#   TARGET               cargo target triple (default: the host)
#   ARCODE_BIN           package this binary instead of building one
#   ARCODE_SKIP_PLUGINS  1: no bundled plugins (the app works without them)
#   ARCHIVE              1: also write <out>.tar.gz (or <out>.zip on Windows)
#
# Usage: scripts/package.sh [out-dir]
#   default out-dir: target/package/arcode-<version>-<target>
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$root/Cargo.toml" | head -n 1)"
host="$(rustc -vV | sed -n 's/^host: //p')"
target="${TARGET:-$host}"
out="${1:-$root/target/package/arcode-$version-$target}"

exe=arcode
case "$target" in *windows*) exe=arcode.exe ;; esac

if [ -n "${ARCODE_BIN:-}" ]; then
    bin="$ARCODE_BIN"
else
    cargo build --manifest-path "$root/Cargo.toml" --release --locked -p arcode --target "$target"
    bin="${CARGO_TARGET_DIR:-$root/target}/$target/release/$exe"
fi
if [ ! -f "$bin" ]; then
    echo "error: no Arcode binary at $bin" >&2
    exit 1
fi

rm -rf "${out:?}"
mkdir -p "$out/bin" "$out/share/arcode/plugins" "$out/share/doc/arcode"
cp "$bin" "$out/bin/$exe"
chmod 755 "$out/bin/$exe"

if [ "${ARCODE_SKIP_PLUGINS:-}" != 1 ]; then
    ARCODE_WASM_PLUGINS="$out/share/arcode/plugins" "$root/scripts/build-plugins.sh"
    found=0
    for dir in "$out/share/arcode/plugins"/*/; do
        [ -d "$dir" ] || continue
        if [ ! -f "$dir/plugin.toml" ] || [ ! -f "$dir/plugin.wasm" ]; then
            echo "error: $dir is not a plugin folder" >&2
            exit 1
        fi
        found=$((found + 1))
    done
    if [ "$found" -eq 0 ]; then
        echo "error: no plugins were built into $out/share/arcode/plugins" >&2
        exit 1
    fi
fi

for doc in README.md LICENSE-MIT LICENSE-APACHE; do
    if [ -f "$root/$doc" ]; then cp "$root/$doc" "$out/share/doc/arcode/"; fi
done

if [ "${ARCHIVE:-}" = 1 ]; then
    parent="$(dirname "$out")"
    name="$(basename "$out")"
    case "$target" in
        *windows*) (cd "$parent" && zip -qr "$name.zip" "$name") && echo "$out.zip" ;;
        *) tar -C "$parent" -czf "$out.tar.gz" "$name" && echo "$out.tar.gz" ;;
    esac
fi

echo "packaged $out:"
(cd "$out" && find . -type f | sort | sed 's|^\./|  |')
