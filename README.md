# Super Mario Galaxy for Retro-Go SD

This repository ports [SMG GBA](https://github.com/TobiasFriedly/smg-gba) to
STM32H7B0-based Game & Watch devices running a compatible Retro-Go SD firmware.
Our focus is the STM32H7B0 port. We aim to keep the rest of the project close to
upstream so that future upstream changes are easier to incorporate.

The port produces a homebrew entry point, `SMG.bin`, and two mapped sidecars,
`smg_code.xip` and `smg_assets.xip`. It does not build or modify Retro-Go
firmware. The game has run on hardware at 30 FPS; sustained 60 FPS remains a
goal.

## Get the files

1. Use your own legally obtained USA Super Mario Galaxy disc image (`RMGE01`)
   as an ISO. Game assets are generated from your copy and are not distributed
   in this repository.
2. Open the [browser converter](https://slash-proc.github.io/smg-retro-go-sd/),
   select the ISO, and convert it. The ISO stays on your device. The page can
   verify the full image before conversion, or convert without that full scan.
3. Download the install-files ZIP containing `SMG.bin`, `smg_code.xip`, and
   your generated `smg_assets.xip`. The asset sidecar is also available on its
   own. The converter processes the ISO in bounded ranges; generating the
   assets can take several minutes and uses additional memory.

Installation requires a compatible Retro-Go SD profile with GWHB ABI v2 and
at least 888 bytes of ABI data. Mapped sidecars require correct placement and
relocation; see the [port build and installation instructions](retro-go-sd/README.md)
for the GWProv workflow and profile requirements. The separate offline release
bundle includes the manifest and converter for use with that workflow.

## Build and test

From the repository root, with Docker, Python 3, Make, a host C compiler,
`arm-none-eabi-objcopy`, and your USA ISO named `baserom_us.iso`:

```sh
make build/assets/assets_gen.h
tools/wasm_converter/build.sh
make -C retro-go-sd local-bundle
```

The port README covers the resulting files, installation, release process,
and current limitations. Run the local CI push gate with:

```sh
sh tools/run_ci.sh
```

For disk use and cleanup, `make disk-usage` reports the main consumers;
`make clean-cache` removes compiler caches, and `make clean-history` removes
performance runs and temporary profiles. Stop builds and tests first. `make
clean` removes the root `build/` directory, including its generated assets.

## Upstream and development

The original GBA project is [TobiasFriedly/smg-gba](https://github.com/TobiasFriedly/smg-gba).
The upstream project builds its own GBA target; this fork's supported target
and development focus are the STM32H7B0 Retro-Go SD port. Port-specific code
and tooling should stay separate from upstream where practical. Existing shared
code changes support the port and should be kept as small as practical when
integrating future upstream work.

See the [port README](retro-go-sd/README.md) for build and installation details
and the [porting log](PORTING.md) for technical notes and test evidence.
