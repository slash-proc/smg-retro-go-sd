# SMG GBA

Release of my GBA SMG Project. It uses the engine from Joshua Barettos https://github.com/zesterer as a base and adds a lot of new features. It also implements code from the Galaxy Decomp https://github.com/SMGCommunity/Petari 

This project contains only code. It contains no assets. Every model, animation, texture, sound, piece of music and line of text is constructed on your computer from your own copy of SMG while the game is built.

You need:

- YOUR OWN PERSONAL legal dump of **SMG (USA)** as an `.iso` file

## macOS

1. Open **Terminal**
2. Install Apple's developer tools:

       xcode-select --install

Click **Install** on the opened window

3. Install devkitPro:
   - Download `devkitpro-pacman-installer.pkg` from https://github.com/devkitPro/pacman/releases/latest and open it. If macOS blocks it, go to **System Settings > Privacy & Security** and click **Open Anyway**.
   - Then run this command with sudo access:

         sudo dkp-pacman -S gba-dev

4. Open a new terminal to build following the steps below. 

## Windows

We use WSL.

1. Open **Windows PowerShell** and choose **Run as administrator**. Run:

       wsl --install -d Ubuntu

   When it says the changes take effect after a reboot, restart your PC. After the restart, open **Windows PowerShell** as administrator again and run the same command again. Now it should dowload Ubuntu and starts it in that window. Create a default Unix user account (with a password for sudo later).

2. In Ubuntu install the tools:

       sudo apt update
       sudo apt install -y build-essential python3 curl wget

3. Install devkitPro:

       curl -fsSLO -A "dkp apt" https://apt.devkitpro.org/install-devkitpro-pacman
       chmod +x install-devkitpro-pacman
       sudo ./install-devkitpro-pacman
       sudo dkp-pacman -S gba-dev

4. Close the Ubuntu window and open it again

5. Copy this project folder and your `.iso` into Ubuntu: in Windows Explorer, open `\\wsl$\Ubuntu\home\<your user name>` and copy them there. Maybe don't build from the `C:` drive: it works, but it's a bit slower...

Then build with the building steps below ! :)) 

## Linux (Ubuntu or Debian)

So actually like windows but easier: do steps 2 to 4 of the Windows section in a normal terminal.

## Building

1. Put your game dump into the project folder and rename it`baserom_us.iso`. Windows Explorer hides file extensions, so rename it to just `baserom_us` there (otherwise it becomes `baserom_us.iso.iso`). Or, in Ubuntu: `mv "<your dump>.iso" galaxy-gba/baserom_us.iso`.
2. In terminal go into the project folder. Type `cd ` drag the project folder into the terminal window and press Enter. On Windows/Ubuntu, if you copied the folder into your home folder as in step 5, type `cd galaxy-gba` instead, using your folder's name.
3. Start the build:

       make

The first build takes a few minutes. It downloads the disc tool `nodtool` and unpacks your disc, checks that every file it needs matches the original game. Also it constructs assets.

## Disk usage and cleanup

Run these commands from the project root:

```sh
make disk-usage    # Show the main disk consumers (build/ includes build/perf/).
make clean-cache   # Remove Rust compiler caches, Ruff cache and Python bytecode.
make clean-history # Remove performance runs and temporary emulator profiles/captures.
```

Stop builds and tests before cleaning caches. The next Rust build recompiles its
dependencies. `clean-cache` preserves your ISO, extracted disc files, constructed
assets, generated web WASM modules, Retro-Go bundles, performance evidence in
`build/perf/`, and emulator profiles and captures in `tmp/`.

`clean-history` removes `build/perf/`, `retro-go-sd/build/perf/`, and `tmp/`,
while keeping the current build outputs and bundles.

`make clean` removes the entire root `build/` directory, **including performance
evidence**. `make distclean` also removes downloaded tools and dependency caches
in `.tools/`. Use these only when you want a full rebuild and have saved any
evidence you need. The Retro-Go build directory is separate; see
[its build instructions](retro-go-sd/README.md).

## Come here for help

- **`WSL2 is unable to start since virtualization is not enabled on this machine`** (Windows): turn on virtualization (Intel VT-x / AMD-V / SVM) in your PC's BIOS/UEFI settings, then run `wsl --install -d Ubuntu` again.
- **`curl: (22) The requested URL returned error: 403`** in the devkitPro step: the devkitPro server blocks plain `curl`. Copy the command exactly as written above, including `-A "dkp apt"`.
- **`arm-none-eabi-gcc: No such file or directory`**: the GBA compiler isn't installed. Repeat the devkitPro step, then open a new terminal window.




Hope it all works:)) 
