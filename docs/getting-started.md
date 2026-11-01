# Getting started

This guide explains how to prepare Super Mario Galaxy from your own disc image
and install the published release into an existing Retro-Go SD profile using
GWProv.

## Before you begin

You need a modified STM32H7B0 Game & Watch with Retro-Go SD, an existing GWProv
profile for that device, and a USA Super Mario Galaxy ISO (`RMGE01`) made from
your own disc. See the [Retro-Go SD project](https://github.com/sylverb/game-and-watch-retro-go-sd)
for firmware and hardware setup, and the [GWProv guide](https://github.com/slash-proc/gwprov)
for installing GWProv, creating profiles, connecting a device, and applying a profile.

The converter reads the ISO on your computer; it does not upload it. Building
the assets can take several minutes. The port has run on hardware at 30 FPS,
but it is an early release and some scenes may be slow or incomplete.

## Prepare the files in your browser

1. Open the [converter](https://slash-proc.github.io/smg-retro-go-sd/) and
   select the USA ISO.
2. Select **Verify and convert**. Verification checks the entire disc image;
   conversion builds the game assets. Wait for both steps to finish.
3. Download the install-files ZIP. It contains `SMG.bin`, `smg_code.xip`, and
   `smg_assets.xip`. Keep the files together if your installation tool asks for
   them.

The browser ZIP is a simple archive of those three files. It is not the offline
bundle used by `gwprov project install`, which has a manifest and its own
converter. Use the route below for the documented GWProv installation.

## Install with GWProv

Download **offline bundle** from the converter page. Install GWProv with its
`dist` and `media` extras as described in the [GWProv installation guide](https://github.com/slash-proc/gwprov#install).
Then, in a terminal, replace the placeholders with your offline bundle, ISO,
and existing profile names:

```sh
gwprov project install ./smg-v0.0.1-bundle.zip profile:PROFILE \
  --input disc=./baserom_us.iso --wasm-fuel 10000000000000
```

This runs the bundle's converter on the ISO and adds the game to the profile;
it does not use the browser-generated asset file. To launch the game when the
profile boots, configure autostart:

```sh
gwprov retro-go config profile:PROFILE --auto-start /homebrews/SMG.bin
```

To run the profile in GWemu, use `gwprov start profile:PROFILE`. To put it on a
physical device, consult [GWProv's profile and device instructions](https://github.com/slash-proc/gwprov#quick-start-create-and-flash-a-profile)
and use the profile apply command for your connected device. The installer must place the mapped game files
correctly; simply copying the browser ZIP to a card has not been established
here as a supported installation procedure.
