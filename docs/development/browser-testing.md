# Browser converter testing

This guide explains how to run the headless browser tests that verify ISO conversion and compare its output with the native asset sidecar.

This uses the Playwright version and official browser container family used by
the adjacent `gnw-web-builder/tools/device-test/` harness. It opens the real static converter
page, supplies `baserom_us.iso`, builds the linked asset XIP, and compares the
downloaded bytes with the native `retro-go-sd/smg_assets.xip` in headless
Chromium and Firefox. The default run uses the recommended 8 MiB range size.

The browser script starts the static server and stops it when the checks finish.
Run the checks from the repository root with:

```sh
tools/wasm_converter/browser-test/run.sh
```

The script builds and runs the test container with Docker. The default ISO
path is `baserom_us.iso` in the repository root; override it with
`SMG_ISO_PATH=/path/to/image.iso`. Browser reports use the repository's
gitignored `tmp/wasm-browser-report/` directory and are removed when the script
finishes. Set `SMG_BROWSER_REPORT_DIR` to retain them elsewhere.
Run only one engine with `SMG_BROWSERS=firefox` or `SMG_BROWSERS=chromium`.
Set `SMG_TEST_GREP` to a Playwright title pattern to run a subset, for example
`SMG_TEST_GREP='convert the verified USA ISO'` for the ISO-to-XIP parity test.

The identity check times full-ISO SHA-1 reads. The conversion test reads all
597 manifest-selected files through browser ranges, constructs the runtime
assets with the WASI asset compiler, assembles the XIP, and requires exact
byte-for-byte equality with the native linked sidecar. Set
`SMG_EXTRACT_WINDOWS=8,64,128,256` to repeat that complete conversion at the
larger input range sizes.
