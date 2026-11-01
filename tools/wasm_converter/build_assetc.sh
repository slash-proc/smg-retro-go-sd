#!/usr/bin/env sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
GENERATED="$PROJECT_ROOT/build/constructed/tools/asset_compiler"
OUT="$PROJECT_ROOT/build/wasm_converter/assetc.wasm"

if [ ! -f "$GENERATED/water_volumes_gen.h" ]; then
  echo "missing $GENERATED/water_volumes_gen.h; build the native asset construction inputs first" >&2
  exit 1
fi

mkdir -p "$PROJECT_ROOT/build/wasm_converter"
docker run --rm \
  -u "$(id -u):$(id -g)" \
  -v "$PROJECT_ROOT:/workspace" -w /workspace \
  ghcr.io/webassembly/wasi-sdk:latest \
  sh -c '/opt/wasi-sdk/bin/clang \
    -D_GNU_SOURCE -std=c11 -O2 -ffp-contract=off \
    -ffunction-sections -fdata-sections \
    -Ibuild/constructed/tools/asset_compiler \
    -Wl,--strip-all -Wl,--gc-sections \
    -Wl,-z,stack-size=8388608 -Wl,--max-memory=1073741824 \
    tools/asset_compiler/*.c -lm \
    -o build/wasm_converter/assetc.wasm'
cp "$OUT" "$PROJECT_ROOT/tools/wasm_converter/web/assetc.wasm"

echo "built $OUT"
