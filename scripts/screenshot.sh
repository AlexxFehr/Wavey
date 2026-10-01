#!/usr/bin/env bash
# Renders the debug app's window to a PNG (the app draws itself; nothing else
# on screen is captured). Usage: scripts/screenshot.sh out.png [app args...]
set -euo pipefail
OUT="$1"; shift
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
"$ROOT/target.nosync/swift/debug/Wavey" "$@" --snapshot "$OUT"
