# Super Mario Galaxy for Game & Watch

Play the [SMG GBA](https://github.com/TobiasFriedly/smg-gba) project on a
modified Nintendo Game & Watch running
[Retro-Go SD](https://github.com/sylverb/game-and-watch-retro-go-sd). This port
targets the STM32H7B0 devices used by the Super Mario Bros. and Zelda Game &
Watch systems.

The game has run on hardware at 30 frames per second. This is an early release:
some scenes may run slowly or have visual or gameplay issues.

## What you need

- A modified Game & Watch with Retro-Go SD set up, plus a way to manage its
  games. This project does not install firmware onto the device.
- Your own USA copy of Super Mario Galaxy, dumped as an ISO file. The game data
  is generated from your copy and is not included in the download.
- A computer and a browser for preparing the game files.

## Prepare the game

1. Open the [Super Mario Galaxy converter](https://slash-proc.github.io/smg-retro-go-sd/).
2. Choose your USA ISO and select **Verify and convert**. The ISO stays on your
   computer. Conversion can take several minutes.
3. Download the ZIP when it is ready. It contains the game and the two files it
   needs to run. You can also download the generated asset file separately.

The download is a set of game files. Simply copying the ZIP to an SD card has
not been verified as an installation method, and a user-friendly installation
procedure is not yet available. See [Getting started](docs/getting-started.md)
for the current status.

## Developers

This fork focuses on the STM32H7B0 port. We aim to keep shared code close to
[upstream](https://github.com/TobiasFriedly/smg-gba) so future changes are easier
to incorporate. See the [documentation index](docs/README.md) for the project
layout, build instructions, testing, and release process.
