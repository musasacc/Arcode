#!/usr/bin/env bash
# Downloads the language servers Arcode bundles, for one platform, into
# <dest>/language-servers (docs/PACKAGING.md, packaging/language-servers.toml).
#
#   scripts/fetch-language-servers.sh <platform> <dest> [--cache DIR] [--force]
#
#   platform  linux-x64, linux-arm64, macos-arm64, macos-x64 or windows-x64
#   dest      the folder that gets language-servers/, e.g.
#             <prefix>/share/arcode for a release (scripts/package.sh does
#             this), or Arcode's local data folder for a build from source:
#               Linux    ~/.local/share/arcode
#               macOS    ~/Library/Application Support/arcode
#               Windows  %LOCALAPPDATA%\arcode  (in Git Bash: "$LOCALAPPDATA/arcode")
#
# Every download is checked against the sha256 in the manifest; a mismatch
# stops the script before anything is installed. Downloads are cached
# (--cache, else $ARCODE_LSP_CACHE, else ~/.cache/arcode-language-servers),
# and a second run with the same manifest does nothing.
#
# Needs: Python 3.11+, curl, tar; npm when fetching the npm servers for a
# platform other than the one this runs on; Go to build gopls (it is left
# out with a note otherwise, or when ARCODE_LSP_SKIP_GO=1).
#
# Works in bash on Linux and macOS and in Git Bash on Windows.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python=""
for candidate in python3 python; do
    if command -v "$candidate" > /dev/null 2>&1 &&
        "$candidate" -c 'import sys; sys.exit(sys.version_info < (3, 11))' 2> /dev/null; then
        python="$candidate"
        break
    fi
done
if [ -z "$python" ]; then
    echo "error: Python 3.11 or newer is needed (python3 or python on PATH)" >&2
    exit 1
fi

exec "$python" -I "$root/scripts/fetch-language-servers.py" "$@"
