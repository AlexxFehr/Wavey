#!/usr/bin/env bash
# Builds the Rust core as a static library and generates the Swift bindings
# that the SwiftPM app package compiles against.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${1:-release}"
cd "$ROOT"

if [[ "$PROFILE" == "release" ]]; then
  cargo build --release -p wavey-ffi
else
  cargo build -p wavey-ffi
fi

LIB="$ROOT/target.nosync/$PROFILE/libwavey_ffi.a"
SWIFT_OUT="$ROOT/app/Sources/WaveyCore"
FFI_OUT="$ROOT/app/Sources/WaveyCoreFFI"
mkdir -p "$SWIFT_OUT" "$FFI_OUT" "$ROOT/target.nosync/lib"

BINDGEN=(cargo run -q -p wavey-ffi --bin uniffi-bindgen-swift --)
"${BINDGEN[@]}" --swift-sources "$LIB" "$SWIFT_OUT"
"${BINDGEN[@]}" --headers "$LIB" "$FFI_OUT"
"${BINDGEN[@]}" --modulemap --module-name wavey_ffiFFI --modulemap-filename module.modulemap "$LIB" "$FFI_OUT"

# Stable path for the SwiftPM linker flags, independent of profile.
cp "$LIB" "$ROOT/target.nosync/lib/libwavey_ffi.a"
echo "core built ($PROFILE) and bindings generated"
