#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/wasm_range_probe.wasm web/range_probe.wasm
