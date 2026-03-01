#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
JNI_SRC="$ROOT_DIR/compose/native/friday_bridge_jni.c"
BRIDGE_HEADERS="$ROOT_DIR/libs/FridayBridge/include"

resolve_java_home() {
  if [[ -n "${JAVA_HOME:-}" ]]; then
    return
  fi

  if command -v /usr/libexec/java_home >/dev/null 2>&1; then
    local detected
    detected="$(/usr/libexec/java_home 2>/dev/null || true)"
    if [[ -n "$detected" && -d "$detected" ]]; then
      export JAVA_HOME="$detected"
      return
    fi
  fi

  echo "JAVA_HOME is not set. Use IDE/Gradle-managed JDK or export JAVA_HOME explicitly."
  exit 1
}

resolve_ndk_home() {
  if [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
    return
  fi

  local sdk_dir=""
  local local_props="$ROOT_DIR/compose/local.properties"
  if [[ -f "$local_props" ]]; then
    sdk_dir="$(awk -F= '/^sdk.dir=/{print $2}' "$local_props" | tail -n 1 | sed 's|\\:|:|g' | sed 's|\\\\|/|g')"
  fi

  if [[ -z "$sdk_dir" && -n "${ANDROID_SDK_ROOT:-}" ]]; then
    sdk_dir="$ANDROID_SDK_ROOT"
  fi

  if [[ -n "$sdk_dir" && -d "$sdk_dir/ndk" ]]; then
    local latest_ndk
    latest_ndk="$(find "$sdk_dir/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)"
    if [[ -n "$latest_ndk" && -d "$latest_ndk" ]]; then
      export ANDROID_NDK_HOME="$latest_ndk"
      return
    fi
  fi

  echo "ANDROID_NDK_HOME must be set (or install NDK under the Android SDK directory in compose/local.properties)"
  exit 1
}

resolve_java_home

JAVA_INCLUDE_OS_DIR="$JAVA_HOME/include/linux"
if [[ "$(uname -s)" == "Darwin" ]]; then
  JAVA_INCLUDE_OS_DIR="$JAVA_HOME/include/darwin"
fi

build_linux_jni() {
  if ! command -v cc >/dev/null 2>&1; then
    echo "No C compiler found (cc) for Linux JNI build"
    exit 1
  fi

  out_dir="$ROOT_DIR/compose/native/linux-x86-64"
  mkdir -p "$out_dir"

  cc -shared -fPIC \
    "$JNI_SRC" \
    -I"$BRIDGE_HEADERS" \
    -I"$JAVA_HOME/include" \
    -I"$JAVA_INCLUDE_OS_DIR" \
    -L"$ROOT_DIR/libs/FridayBridge/.build/release" \
    -lFridayBridge \
    -o "$out_dir/libFridayBridgeJNI.so"

  cp "$ROOT_DIR/libs/FridayBridge/.build/release/libFridayBridge.so" "$out_dir/libFridayBridge.so"
}

build_android_jni() {
  resolve_ndk_home

  host_tag="darwin-x86_64"
  if [[ -d "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-arm64" ]]; then
    host_tag="darwin-arm64"
  fi

  toolchain_bin="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$host_tag/bin"
  if [[ ! -d "$toolchain_bin" ]]; then
    echo "Could not find NDK clang toolchain at $toolchain_bin"
    exit 1
  fi

  resolve_android_so_path() {
    local triple_api="$1"
    local found
    found="$(find "$ROOT_DIR/libs/FridayBridge/.build" -path "*/${triple_api}/release/libFridayBridge.so" | head -n 1)"
    if [[ -n "$found" ]]; then
      echo "$found"
      return
    fi

    echo "Could not find libFridayBridge.so for $triple_api under $ROOT_DIR/libs/FridayBridge/.build"
    exit 1
  }

  declare -a targets=(
    "aarch64-unknown-linux-android24:aarch64-linux-android24:arm64-v8a"
    "x86_64-unknown-linux-android24:x86_64-linux-android24:x86_64"
  )

  for target in "${targets[@]}"; do
    swift_triple_api="${target%%:*}"
    remainder="${target#*:}"
    ndk_triple_api="${remainder%%:*}"
    abi="${remainder##*:}"

    out_dir="$ROOT_DIR/compose/composeApp/src/androidMain/jniLibs/$abi"
    mkdir -p "$out_dir"

    "$toolchain_bin/${ndk_triple_api}-clang" -shared -fPIC \
      "$JNI_SRC" \
      -I"$BRIDGE_HEADERS" \
      -I"$JAVA_HOME/include" \
      -I"$JAVA_INCLUDE_OS_DIR" \
      -L"$(dirname "$(resolve_android_so_path "$swift_triple_api")")" \
      -lFridayBridge \
      -o "$out_dir/libFridayBridgeJNI.so"

    cp "$(resolve_android_so_path "$swift_triple_api")" "$out_dir/libFridayBridge.so"
  done
}

if [[ "${1:-all}" == "linux" || "${1:-all}" == "all" ]]; then
  echo "[jni-bridge] Building Linux JNI bridge"
  build_linux_jni
fi

if [[ "${1:-all}" == "android" || "${1:-all}" == "all" ]]; then
  echo "[jni-bridge] Building Android JNI bridge"
  build_android_jni
fi

echo "[jni-bridge] Done"
