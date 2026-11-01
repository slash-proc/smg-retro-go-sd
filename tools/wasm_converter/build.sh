#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")"
PROJECT_ROOT=$(CDPATH= cd -- ../.. && pwd)
mkdir -p "$PROJECT_ROOT/build/wasm_converter"
docker run --rm \
  -u "$(id -u):$(id -g)" \
  -v "$PROJECT_ROOT:/workspace" -w /workspace \
  python:3.12-slim \
  sh -c 'python3 tools/asset_construction/stage_recipe_descriptors.py && \
    python3 tools/asset_construction/fixed_asset_inputs.py'
"$PROJECT_ROOT/tools/wasm_converter/build_assetc.sh"
"$PROJECT_ROOT/tools/wasm_converter/build_assetc_linked.sh"
"$PROJECT_ROOT/tools/wasm_converter/build_wasi_shim.sh"
docker run --rm \
  -u "$(id -u):$(id -g)" \
  -v "$PROJECT_ROOT:/workspace" -w /workspace \
  ghcr.io/webassembly/wasi-sdk:latest \
  /opt/wasi-sdk/bin/clang -nostdlib -ffreestanding \
  -std=c11 -O2 -ffp-contract=off -ffunction-sections -fdata-sections \
  -DTEXTURES_BAKE_CORE_LIBRARY -DTEXTURES_BAKE_CORE_ARENA \
  -c tools/asset_construction/textures_bake_core.c \
  -o build/wasm_converter/textures_bake_core.o
docker run --rm \
  -u "$(id -u):$(id -g)" \
  -e CARGO_HOME=/workspace/.tools/cargo-home \
  -e SMG_BAKE_CORE_OBJECT=/workspace/build/wasm_converter/textures_bake_core.o \
  -v "$PROJECT_ROOT:/workspace" -w /workspace/tools/wasm_converter \
  rust:1.89-bookworm \
  sh -c 'rustup target add wasm32-unknown-unknown && \
    cargo build --release --lib --target wasm32-unknown-unknown --no-default-features --features verifier-probe && \
    cp target/wasm32-unknown-unknown/release/smg_iso_verifier.wasm web/smg_iso_verifier.wasm && \
    SMG_EXTRACT_BUFFER_MIB=8 RUSTFLAGS="-C link-arg=--max-memory=1073741824" cargo build --release --lib --target wasm32-unknown-unknown --no-default-features --features processor && \
    cp target/wasm32-unknown-unknown/release/smg_iso_verifier.wasm web/smg_asset_converter.wasm && \
    for size in 64 128 256; do \
      SMG_EXTRACT_BUFFER_MIB=$size RUSTFLAGS="-C link-arg=--max-memory=1073741824" cargo build --release --lib --target wasm32-unknown-unknown --no-default-features --features processor && \
      cp target/wasm32-unknown-unknown/release/smg_iso_verifier.wasm web/smg_asset_converter_$size.wasm; \
    done && \
    RUSTFLAGS="-C link-arg=--max-memory=1073741824" cargo build --release --lib --target wasm32-unknown-unknown --no-default-features --features asset-assembler && \
    cp target/wasm32-unknown-unknown/release/smg_iso_verifier.wasm web/asset_assembler.wasm && \
    SMG_ASSETC_LINKED_LIB_DIR=/workspace/build/wasm_converter/assetc_linked \
      SMG_EXTRACT_BUFFER_MIB=8 RUSTFLAGS="-C link-arg=--max-memory=1073741824" \
      cargo build --release --lib --target wasm32-unknown-unknown --no-default-features --features processor,assetc-linked && \
    cp target/wasm32-unknown-unknown/release/smg_iso_verifier.wasm web/smg_asset_converter.wasm && \
    for size in 64 128 256; do \
      SMG_ASSETC_LINKED_LIB_DIR=/workspace/build/wasm_converter/assetc_linked \
        SMG_EXTRACT_BUFFER_MIB=$size RUSTFLAGS="-C link-arg=--max-memory=1073741824" \
        cargo build --release --lib --target wasm32-unknown-unknown --no-default-features --features processor,assetc-linked && \
      cp target/wasm32-unknown-unknown/release/smg_iso_verifier.wasm web/smg_asset_converter_$size.wasm; \
    done'
