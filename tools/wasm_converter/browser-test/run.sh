#!/usr/bin/env sh
set -eu
REPO=$(CDPATH= cd -- "$(dirname "$0")/../../.." && pwd)
TEST_DIR="$REPO/tools/wasm_converter/browser-test"
IMAGE=smg-wasm-chromium-test
ISO=${SMG_ISO_PATH:-"$REPO/baserom_us.iso"}
REPORT=${SMG_BROWSER_REPORT_DIR:-"$REPO/tmp/wasm-browser-report"}
SERVER_LOG="$REPO/tmp/wasm-browser-http.log"
if [ -z "${SMG_BROWSER_REPORT_DIR:-}" ]; then
  rm -rf "$REPORT"
fi
BROWSERS=${SMG_BROWSERS:-"chromium firefox"}

if [ ! -f "$ISO" ]; then
  echo "ISO not found: $ISO" >&2
  exit 2
fi
if [ ! -f "$REPO/tools/wasm_converter/web/smg_iso_verifier.wasm" ] || \
   [ ! -f "$REPO/tools/wasm_converter/web/smg_asset_converter.wasm" ] || \
   [ ! -f "$REPO/tools/wasm_converter/web/assetc.wasm" ] || \
   [ ! -f "$REPO/tools/wasm_converter/web/asset_assembler.wasm" ] || \
   [ ! -f "$REPO/tools/wasm_converter/web/vendor/wasi/index.js" ]; then
  "$REPO/tools/wasm_converter/build.sh"
fi

mkdir -p "$REPORT/chromium" "$REPORT/firefox"
mkdir -p "$REPO/tmp"
PORT=${SMG_BROWSER_PORT:-8000}
HOST=127.0.0.1 PORT="$PORT" "$REPO/tools/wasm_converter/serve.sh" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
cleanup() {
  kill "$SERVER_PID" 2>/dev/null || true
  wait "$SERVER_PID" 2>/dev/null || true
  rm -f "$SERVER_LOG"
  if [ -z "${SMG_BROWSER_REPORT_DIR:-}" ]; then rm -rf "$REPORT"; fi
}
trap cleanup EXIT INT TERM
SMG_BROWSER_URL="http://127.0.0.1:$PORT" python3 -c 'import os,socket,time; h,u=os.environ["SMG_BROWSER_URL"].split("://",1)[1].split(":"); p=int(u); end=time.time()+15
while time.time()<end:
 try: socket.create_connection((h,p),timeout=.25).close(); break
 except OSError: time.sleep(.1)
else: raise SystemExit("converter HTTP server did not start")'
docker build -f "$TEST_DIR/Dockerfile" -t "$IMAGE" "$TEST_DIR"
for browser in $BROWSERS; do
  case "$browser" in chromium|firefox) ;; *) echo "Unsupported browser: $browser" >&2; exit 2 ;; esac
  mkdir -p "$REPORT/$browser"
  if [ -n "${SMG_TEST_GREP:-}" ]; then
    docker run --rm --init --network host --ipc host \
      --user "$(id -u):$(id -g)" \
      -e HOME=/reports/home \
      -e SMG_BROWSER="$browser" \
      -e SMG_ISO_PATH=/workspace/baserom_us.iso \
      -e SMG_BROWSER_URL="http://127.0.0.1:$PORT" \
      -e SMG_EXTRACT_WINDOWS="${SMG_EXTRACT_WINDOWS:-8}" \
      -e SMG_ASSETC_FULL="${SMG_ASSETC_FULL:-}" \
      -e SMG_BROWSER_REPORT_DIR="/reports/$browser" \
      -v "$REPO:/workspace:ro" \
      -v "$REPORT:/reports" \
      "$IMAGE" -- --grep "$SMG_TEST_GREP"
  else
    docker run --rm --init --network host --ipc host \
      --user "$(id -u):$(id -g)" \
      -e HOME=/reports/home \
      -e SMG_BROWSER="$browser" \
      -e SMG_ISO_PATH=/workspace/baserom_us.iso \
      -e SMG_BROWSER_URL="http://127.0.0.1:$PORT" \
      -e SMG_EXTRACT_WINDOWS="${SMG_EXTRACT_WINDOWS:-8}" \
      -e SMG_ASSETC_FULL="${SMG_ASSETC_FULL:-}" \
      -e SMG_BROWSER_REPORT_DIR="/reports/$browser" \
      -v "$REPO:/workspace:ro" \
      -v "$REPORT:/reports" \
      "$IMAGE"
  fi
done
