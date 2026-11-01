# Documentation

This index introduces the project layout and points to setup instructions,
developer workflows, and historical technical notes.

## Guides

- [Getting started](getting-started.md) — prepare the game and install it with GWProv.
- [Port development](development/port.md) — build, test, install, and publish the STM32H7B0 port.
- [Browser testing](development/browser-testing.md) — verify conversion in Chromium and Firefox.

## Project layout

- `src/` and `asset_construction/` contain shared game and asset source.
- `retro-go-sd/` contains the device port, SDK, release scripts, and browser page.
- `tools/` contains asset, converter, and test tooling.
- `docs/reference/` contains historical investigation and test logs.

## Reference

- [Porting log](reference/porting-log.md) — technical decisions and hardware evidence.
- [WASM converter log](reference/wasm-converter-log.md) — converter design and experiments.
