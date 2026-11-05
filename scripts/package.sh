#!/usr/bin/env bash
# Lays out a release of Arcode as an installable prefix (docs/PACKAGING.md):
#
#   <out>/bin/arcode[.exe]
#   <out>/share/arcode/plugins/<id>/{plugin.toml,plugin.wasm[,README.md,LICENSE]}
#   <out>/share/arcode/language-servers/   bundled language servers
#   <out>/share/doc/arcode/{README.md,LICENSE-MIT,LICENSE-APACHE,THIRD-PARTY.txt}
#
# The app finds its bundled plugins at `<exe>/../share/arcode/plugins`
# (arcode_plugin_host::paths::bundled_roots), so the prefix can be moved or
# installed anywhere (/usr/local, /opt/arcode, a zip on Windows, the
# Contents/ folder of a macOS .app with the binary in Contents/MacOS).
#
# Steps: `cargo build --release -p arcode`, then scripts/build-plugins.sh
# with ARCODE_WASM_PLUGINS pointing into the prefix (needs the wasm32-wasip2
# target, clang and the wasi-libc headers; see that script), then
# scripts/fetch-language-servers.sh for the target's platform (downloads
# the pinned, checksummed servers of packaging/language-servers.toml), then
# the docs and the third-party notice.
#
# Environment:
#   TARGET               cargo target triple (default: the host)
#   ARCODE_BIN           package this binary instead of building one
#   ARCODE_SKIP_PLUGINS  1: no bundled plugins (the app works without them)
#   ARCODE_SKIP_LANGUAGE_SERVERS
#                        1: no bundled language servers (a small package for
#                        development; servers on PATH still work)
#   ARCODE_LSP_CACHE     download cache for the language servers
#                        (default: target/language-servers-cache)
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

# The language server platform of the target triple.
case "$target" in
    x86_64-*-linux-*) lsp_platform=linux-x64 ;;
    aarch64-*-linux-*) lsp_platform=linux-arm64 ;;
    aarch64-apple-darwin) lsp_platform=macos-arm64 ;;
    x86_64-apple-darwin) lsp_platform=macos-x64 ;;
    x86_64-pc-windows-*) lsp_platform=windows-x64 ;;
    *) lsp_platform="" ;;
esac
if [ "${ARCODE_SKIP_LANGUAGE_SERVERS:-}" != 1 ] && [ -z "$lsp_platform" ]; then
    echo "error: no bundled language servers for $target (set ARCODE_SKIP_LANGUAGE_SERVERS=1)" >&2
    exit 1
fi

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

if [ "${ARCODE_SKIP_LANGUAGE_SERVERS:-}" != 1 ]; then
    "$root/scripts/fetch-language-servers.sh" "$lsp_platform" "$out/share/arcode" \
        --cache "${ARCODE_LSP_CACHE:-${CARGO_TARGET_DIR:-$root/target}/language-servers-cache}"
    cp "$out/share/arcode/language-servers/THIRD-PARTY.txt" "$out/share/doc/arcode/THIRD-PARTY.txt"
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
(cd "$out" && find . -type f -not -path './share/arcode/language-servers/*' | sort | sed 's|^\./|  |')
if [ -d "$out/share/arcode/language-servers" ]; then
    size="$(du -sk "$out/share/arcode/language-servers" | cut -f1)"
    echo "  share/arcode/language-servers/ ($((size / 1024)) MB, see share/doc/arcode/THIRD-PARTY.txt)"
fi
