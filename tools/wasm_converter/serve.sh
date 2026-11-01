#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")"
if [ ! -f web/smg_iso_verifier.wasm ] || [ ! -f web/assetc.wasm ] || \
   [ ! -f web/asset_assembler.wasm ] || \
   [ ! -f web/vendor/wasi/index.js ]; then
  ./build.sh
fi
cd web
exec python3 -m http.server "${PORT:-8000}" --bind "${HOST:-127.0.0.1}"
