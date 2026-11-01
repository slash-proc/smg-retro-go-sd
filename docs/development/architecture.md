# Port architecture

This guide describes the runtime artifacts, mapped asset relocation, and
bounded-input converter contracts that developers must preserve when changing
the STM32H7B0 port or its distribution.

## Runtime and storage

The port targets STM32H7B0 Game & Watch hardware and runs as a Retro-Go SD
homebrew; this repository does not build the firmware. `SMG.bin` is the GWHB
entry point. `smg_code.xip` holds mapped game code and `smg_assets.xip` holds
mapped generated assets. The executable and both sidecars are required together.
The game renders the world at 160×120 and composes the HUD at 320×240.

The manifest declares both XIPs as mapped. The code XIP has link base
`0xDEC00000`; the asset XIP is opaque and must not be relocated by scanning for
sentinel-looking words. Both code and asset payloads contain ordinary data that
can resemble an address. The build emits exact ABS32 relocation sites from the
ELF, including relevant linker-generated veneers, and validates them against the
final ELF. The manifest carries those tables for installers that support them.
Scanning opaque data for candidate pointer values can corrupt game assets.

## Asset conversion

The USA ISO is the user's input and is not shipped. The converter reads selected
files from the Wii disc, constructs the runtime assets, and emits
`smg_assets.xip` in the same layout as the native build. This layout matters:
the core's generated relocation offsets refer to that exact asset layout.

The WASM processor uses the version 1 `range-input` extension. It registers the
input size, yields requests for a file handle, 64-bit offset and bounded length,
and resumes after the host copies that range into WASM memory. The browser
worker services requests with `File.slice()`; the ISO is never copied wholesale
into WASM memory or uploaded. Disc extraction needs random access, so a
sequential stream alone is insufficient.

The published converter requests at most 8 MiB per range. This is an input
bound, not a bound on total memory: generated assets and compilation data also
occupy memory. A previous Chromium and Firefox test measured 583,008,256 bytes
of WASM linear memory. The browser also offers separate full-disc verification,
using a selectable input buffer. See [browser testing](browser-testing.md) for
parity checks against the native asset file.

## Distribution

The release's offline bundle contains a manifest, binary, code XIP, converter,
cover and debug symbols. The user supplies the ISO during installation; the
bundle does not contain generated game assets. The browser produces a separate
ZIP containing the executable and both XIPs. It is a plain file archive, not a
manifest-based GWRG bundle, and its direct SD installation has not been verified.
See [port development](port.md) for build and release procedures.
