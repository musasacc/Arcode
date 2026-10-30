#!/usr/bin/env bash
# Builds the fixture guests in crates/arcode-plugin-host/test-programs for
# wasm32-wasip2 and lays each one out as a plugin folder:
#
#   target/test-plugins/<fixture>/{plugin.toml,plugin.wasm}
#
# Host integration tests load fixtures from there (override the location
# with ARCODE_TEST_PLUGINS). CI runs this before `cargo test`.
#
# Usage: scripts/build-test-plugins.sh [fixture...]   (default: all)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
programs="$root/crates/arcode-plugin-host/test-programs"
out="${ARCODE_TEST_PLUGINS:-$root/target/test-plugins}"
# Separate from host builds so the wasm and native caches never fight.
target_dir="${ARCODE_WASM_TARGET_DIR:-$root/target/wasm-test-programs}"

if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-wasip2; then
    echo "error: the wasm32-wasip2 target is missing; run: rustup target add wasm32-wasip2" >&2
    exit 1
fi

if [ "$#" -gt 0 ]; then
    fixtures=("$@")
else
    fixtures=()
    for manifest in "$programs"/*/plugin.toml; do
        fixtures+=("$(basename "$(dirname "$manifest")")")
    done
fi

packages=()
for f in "${fixtures[@]}"; do packages+=(-p "$f"); done

CARGO_TARGET_DIR="$target_dir" cargo build \
    --manifest-path "$programs/Cargo.toml" \
    --target wasm32-wasip2 --release --locked "${packages[@]}"

mkdir -p "$out"
for f in "${fixtures[@]}"; do
    wasm="$target_dir/wasm32-wasip2/release/${f//-/_}.wasm"
    if [ ! -f "$wasm" ]; then
        echo "error: $wasm was not produced" >&2
        exit 1
    fi
    rm -rf "${out:?}/$f"
    mkdir -p "$out/$f"
    cp "$programs/$f/plugin.toml" "$out/$f/plugin.toml"
    cp "$wasm" "$out/$f/plugin.wasm"
    printf '%-20s %8d bytes\n' "$f" "$(wc -c < "$out/$f/plugin.wasm")"
done
