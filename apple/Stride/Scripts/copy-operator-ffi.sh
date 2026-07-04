#!/usr/bin/env bash
set -euo pipefail

if [[ "${SDK_NAME:-macosx}" != macosx* ]]; then
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
LIB="$ROOT/target/release/libstride_operator.dylib"

if [[ ! -f "$LIB" ]]; then
  echo "error: Missing $LIB. Run apple/Stride/Scripts/build-operator-ffi.sh before building Stride." >&2
  exit 1
fi

if [[ -z "${TARGET_BUILD_DIR:-}" || -z "${FRAMEWORKS_FOLDER_PATH:-}" ]]; then
  echo "error: TARGET_BUILD_DIR and FRAMEWORKS_FOLDER_PATH are required." >&2
  exit 1
fi

DEST_DIR="$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH"
DEST="$DEST_DIR/$(basename "$LIB")"
mkdir -p "$DEST_DIR"
cp "$LIB" "$DEST"

if [[ "${CODE_SIGNING_ALLOWED:-YES}" == "NO" ]]; then
  exit 0
fi

SIGN_IDENTITY="${EXPANDED_CODE_SIGN_IDENTITY:-${CODE_SIGN_IDENTITY:-}}"
if [[ -z "$SIGN_IDENTITY" ]]; then
  SIGN_IDENTITY="-"
fi

/usr/bin/codesign --force --sign "$SIGN_IDENTITY" --timestamp=none "$DEST"
