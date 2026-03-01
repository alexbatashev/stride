#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
BRIDGE_DIR="$ROOT_DIR/libs/FridayBridge"

ANDROID_TARGETS=(
  "aarch64-unknown-linux-android24:arm64-v8a"
  "x86_64-unknown-linux-android24:x86_64"
)

resolve_android_swift_sdk() {
  if [[ -n "${ANDROID_SWIFT_SDK:-}" ]]; then
    return
  fi

  local detected
  detected="$(swift sdk list | awk '/android/{print $1}' | tail -n 1)"
  if [[ -z "$detected" ]]; then
    echo "No Android Swift SDK found. Install one with 'swift sdk install ...' first."
    exit 1
  fi

  export ANDROID_SWIFT_SDK="$detected"
}

resolve_android_so_path() {
  local triple_api="$1"
  local found
  found="$(find "$BRIDGE_DIR/.build" -path "*/${triple_api}/release/libFridayBridge.so" | head -n 1)"
  if [[ -n "$found" ]]; then
    echo "$found"
    return
  fi

  echo "Could not find libFridayBridge.so for $triple_api under $BRIDGE_DIR/.build"
  exit 1
}

build_linux() {
  echo "[swift-bridge] Building Linux host libFridayBridge.so"
  pushd "$BRIDGE_DIR" >/dev/null
  swift build -c release
  popd >/dev/null
}

build_android() {
  resolve_android_swift_sdk

  for target in "${ANDROID_TARGETS[@]}"; do
    triple_api="${target%%:*}"
    android_abi="${target##*:}"

    echo "[swift-bridge] Building Android libFridayBridge.so for ${triple_api} (sdk: ${ANDROID_SWIFT_SDK})"
    pushd "$BRIDGE_DIR" >/dev/null
    swift build -c release --swift-sdk "$ANDROID_SWIFT_SDK" --triple "$triple_api"
    popd >/dev/null

    out_dir="$ROOT_DIR/compose/composeApp/src/androidMain/jniLibs/$android_abi"
    mkdir -p "$out_dir"

    cp "$(resolve_android_so_path "$triple_api")" "$out_dir/libFridayBridge.so"
  done
}

if [[ "${1:-all}" == "linux" || "${1:-all}" == "all" ]]; then
  build_linux
fi

if [[ "${1:-all}" == "android" || "${1:-all}" == "all" ]]; then
  build_android
fi

echo "[swift-bridge] Done"
