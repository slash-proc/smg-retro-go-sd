# Getting started

This guide explains what you need to prepare Super Mario Galaxy for a modified
Game & Watch and what the current download contains.

## What you need

- A Super Mario Bros. or Zelda Game & Watch modified to run
  [Retro-Go SD](https://github.com/sylverb/game-and-watch-retro-go-sd).
- Your own USA Super Mario Galaxy disc image (`RMGE01`) in ISO format.
- A computer and a browser. The converter reads the ISO on your computer; it
  does not upload the image.

This is an early release. The game has run on hardware at 30 frames per second,
but some scenes may be slow or have visual or gameplay issues.

## Prepare the game files

1. Open the [converter](https://slash-proc.github.io/smg-retro-go-sd/) and
   choose your ISO.
2. Select **Verify and convert**. Verification checks the disc image, then
   conversion builds the game assets. This can take several minutes.
3. When the conversion finishes, download the ZIP. It contains `SMG.bin`,
   `smg_code.xip`, and `smg_assets.xip`; keep these files together.

You can also choose **Convert without full verification** if you have already
verified the ISO, or download just the asset file after conversion.

## Installing on a device

The ZIP provides the game files, but a simple installation by copying them to
an SD card has not yet been verified. We do not yet have a documented,
user-friendly installation procedure for this download. The developer workflow
for testing and provisioning a device is covered in the
[port development guide](development/port.md).
