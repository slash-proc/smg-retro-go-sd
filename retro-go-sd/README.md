# SMG Retro-Go SD homebrew

This target builds the Super Mario Galaxy game runtime as a Retro-Go SD
homebrew for the STM32H7B0 Game & Watch. The GWHB entry point is `SMG.bin`;
linked game code and generated game assets live in separate mapped XIP sidecars,
`smg_code.xip` and `smg_assets.xip`. This project builds and packages the
homebrew only. It does not build or modify Retro-Go firmware.

## Build and package

Use the vendored SDK builder Docker image:

```sh
make docker
make local-bundle
```

`make docker` produces `SMG.bin` and both XIP sidecars. `make local-bundle`
rebuilds with Docker and writes the standard GWRG manifest and installable ZIP
to `build/smg/local-bundle/smg-local-bundle.zip`. The ZIP includes the entry
binary, mapped code XIP, converter module, symbols, and cover. At install, the
converter reads the user's verified USA ISO in bounded ranges and produces
`smg_assets.xip` beside the binary. The opaque asset XIP has no `relocBase`;
the code XIP declares `relocBase: 0xDEC00000`. The homebrew uses
linker-generated exact relocation sites for asset pointers at runtime.

## Install and run

Install the bundle into a compatible Retro-Go SD firmware profile with GWProv:

```sh
gwprov project install ./build/smg/local-bundle/smg-local-bundle.zip profile:PROFILE \
  --input disc=../baserom_us.iso --wasm-fuel 10000000000000
gwprov retro-go config profile:PROFILE --auto-start /homebrews/SMG.bin
gwprov start profile:PROFILE
```

Use an existing SD firmware profile that provides GWHB ABI v2 with at least
888 bytes of ABI data. GWProv stages the files on the SD card. When configured
to autostart SMG, it pre-seeds the executable and both XIP sidecars into virtual
NOR and writes Retro-Go's cache index. The CRC-protected GWLB superblock baked
into bank 2 defines the reserved extflash boundary used by the FrogFS/SD-cache
layout. GWProv requires exactly one valid record with the reserved-boundary flag,
cross-checks its offset against profile metadata when present, and requires the
declared extflash capacity to match. Cache data starts at the next 4 KiB boundary
after that offset, leaving the prefix below it untouched. GWProv also invalidates
the old cache index before writing and rejects a cache set that exceeds the
declared extflash capacity.
This avoids the first-boot sidecar copy.
Profiles with the unpatched zero-CRC initializer are rejected by the seeder.
Do not build Retro-Go firmware as part of this project.

### Local flash profile preparation

The hardware crash investigation found that scanning mapped payloads for
sentinel words corrupts opaque geometry, sine, pose, and image data. Before
deploying a local flash profile, reconstruct its mapped payloads at their final
FrogFS addresses with the preserved sidecars and final ELF:

```sh
# Run using the Python environment where GWProv is installed.
python scripts/prepare_flash_profile.py PROFILE
python scripts/prepare_flash_profile.py PROFILE --apply
gwprov profile PROFILE apply device:DEVICE --region bank2 --region frogfs \
  --start --bank 2 --yes
```

The first command audits without writing. The second patches only actual
ABS32 pointers, updates the FrogFS CRC and deployable component hash, and leaves
other flash regions unchanged. Repeat this step after installing project files
or changing the FrogFS layout. Both XIPs must be declared mapped and their
preserved sources must match the profile's ELF. This is a local preparation
step; automatic exact relocation in the general flash bundle workflow remains
to be integrated. Do not declare an opaque asset sidecar's sentinel as a safe
generic word-scanning range.

## Port status

The port renders the world at 160×120 and composites the HUD into 320×240
LCD output. The pause menu offers 30/60 FPS rendering, scene detail, a depth
buffer, and **Texture mapping: Fast / Accurate**. Accurate perspective-corrects
every textured surface and is the default for new or unconfigured profiles.
Fast affine mapping remains selectable and the choice persists in the existing
per-app settings store. Mapping is independent of the depth-buffer setting;
neither choice requires a firmware change.

The game has run on real hardware at 30 FPS. Sustained 60 FPS is still a goal;
headless GWemu tests establish rendering/logic correctness rather than device
performance. The current Docker build uses 75,120 bytes of RAM code/data,
602,712 bytes of BSS and 55,248 bytes of ITCM code. Accurate perspective
mapping now permits bounded 16-pixel spans while keeping the existing
1/16-texel interpolation error limit. The M7 packed-transform prototype remains
disabled pending hardware measurement.
An optional 128 KiB SRAM chunk-table cache preserves a 32 KiB RAM reserve and
falls back to flash when it cannot fit; the tested VM has 34,808 bytes of
RAM_EMU remaining after allocation. Its hardware speedup is unmeasured.

Earlier Chromium, Firefox and GWProv conversion checks verified the USA ISO
and exact native asset XIP output. Runtime-only changes can use the existing
converted assets with `scripts/package_native_bundle.py`; they do not require
another ISO conversion. GWProv pre-caches the homebrew and sidecars. Current
rendering and collision regressions, accepted-recording replays, and remaining
performance/quality limitations are documented in the porting log.
See [`../PORTING.md`](../PORTING.md) for the porting log and test evidence.
