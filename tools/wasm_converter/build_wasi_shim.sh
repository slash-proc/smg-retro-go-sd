#!/usr/bin/env sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
VENDOR="$PROJECT_ROOT/tools/wasm_converter/web/vendor/wasi"
VERSION="browser_wasi_shim 0.3.0"

if [ -f "$VENDOR/VERSION.txt" ] && [ "$(cat "$VENDOR/VERSION.txt")" = "$VERSION" ] && \
   [ -f "$VENDOR/index.js" ]; then
  echo "using vendored $VERSION"
  exit 0
fi

docker run --rm \
  -u "$(id -u):$(id -g)" \
  -v "$PROJECT_ROOT:/workspace" -w /workspace \
  node:22-bookworm-slim \
  sh -c 'npm install --prefix build/wasm_converter/wasi-shim-package \
      --no-save --ignore-scripts --no-audit --no-fund \
      @bjorn3/browser_wasi_shim@0.3.0 && \
    mkdir -p tools/wasm_converter/web/vendor/wasi && \
    cp build/wasm_converter/wasi-shim-package/node_modules/@bjorn3/browser_wasi_shim/dist/*.js \
      tools/wasm_converter/web/vendor/wasi/ && \
    cp build/wasm_converter/wasi-shim-package/node_modules/@bjorn3/browser_wasi_shim/LICENSE-APACHE \
      tools/wasm_converter/web/vendor/wasi/ && \
    cp build/wasm_converter/wasi-shim-package/node_modules/@bjorn3/browser_wasi_shim/LICENSE-MIT \
      tools/wasm_converter/web/vendor/wasi/ && \
    printf "%s\\n" "browser_wasi_shim 0.3.0" > tools/wasm_converter/web/vendor/wasi/VERSION.txt'

echo "built browser WASI shim under $PROJECT_ROOT/tools/wasm_converter/web/vendor/wasi"
