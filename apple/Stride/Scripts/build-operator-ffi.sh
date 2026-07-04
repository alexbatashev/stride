#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
OUT_DIR="$ROOT/apple/Stride/Stride/Stride/Generated/Operator"
LIB="$ROOT/target/release/libstride_operator.dylib"

cd "$ROOT"

if [[ "${SDK_NAME:-macosx}" != macosx* ]]; then
  exit 0
fi

cargo build -p stride-operator --features ffi --release
install_name_tool -id "@rpath/libstride_operator.dylib" "$LIB"
mkdir -p "$OUT_DIR"

cargo run \
  -p stride-operator \
  --features uniffi-cli \
  --bin stride-operator-uniffi-bindgen \
  -- generate \
  --library "$LIB" \
  --language swift \
  --out-dir "$OUT_DIR"

if [[ -n "${TARGET_BUILD_DIR:-}" && -n "${FRAMEWORKS_FOLDER_PATH:-}" ]]; then
  "$ROOT/apple/Stride/Scripts/copy-operator-ffi.sh"
fi
