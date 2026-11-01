#!/usr/bin/env sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
OUT="$PROJECT_ROOT/build/wasm_converter/assetc_linked"

build_inside_docker() {
  mkdir -p "$OUT/obj"
  rm -f "$OUT/obj"/*.o "$OUT/libassetc.a"
  for src in tools/asset_compiler/*.c; do
    name=$(basename "$src" .c)
    extra=
    if [ "$name" = main ]; then extra=-Dmain=smg_assetc_entry; fi
    /opt/wasi-sdk/bin/clang -D_GNU_SOURCE -DSMG_ASSETC_MEMORY_FS -std=c11 -O2 \
      -ffp-contract=off -ffunction-sections -fdata-sections \
      -Ibuild/constructed/tools/asset_compiler $extra \
      -c "$src" -o "$OUT/obj/$name.o"
  done
  /opt/wasi-sdk/bin/llvm-ar rcs "$OUT/libassetc.a" "$OUT"/obj/*.o
  cp /opt/wasi-sdk/share/wasi-sysroot/lib/wasm32-wasip1/libc.a "$OUT/libc.a"
  cp /opt/wasi-sdk/share/wasi-sysroot/lib/wasm32-wasip1/libm.a "$OUT/libm.a"
}

if [ "${1:-}" = --inside-docker ]; then
  build_inside_docker
  exit 0
fi

if [ ! -f "$PROJECT_ROOT/build/constructed/tools/asset_compiler/water_volumes_gen.h" ]; then
  echo "missing generated asset compiler headers; build native asset construction inputs first" >&2
  exit 1
fi

docker run --rm \
  -u "$(id -u):$(id -g)" \
  -v "$PROJECT_ROOT:/workspace" -w /workspace \
  ghcr.io/webassembly/wasi-sdk:latest \
  sh /workspace/tools/wasm_converter/build_assetc_linked.sh --inside-docker

echo "built linked asset compiler under $OUT"
