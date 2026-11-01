# Retro-Go SD port

## Goal and target

Port the SMG GBA project to the Game & Watch using the Retro-Go SD homebrew
interface. The result is a standalone homebrew program and its asset sidecar,
launched by an existing Retro-Go firmware installation. This project does not
build or modify Retro-Go firmware.

The target device is the STM32H7B0 based Game & Watch. Use the SDK and example
homebrews in `https://github.com/slash-proc/retro-go-sd-templates/` as the porting model.
Use `gwprov` to install and launch the homebrew through headless GWemu. Hardware
remains relinquished until explicitly granted; do not poll it. Firmware
development/building is outside this port's workflow.

Current asset update (2026-10-28): the Tower variant now includes the missing
regular Honeyhive pond/platform terrain in both main and far meshes, retaining
its mission-specific geometry. The shared recipes produce the tested candidate
through native and WASM conversion; see the final entry for evidence and the
remaining gameplay and hardware checks.

Latest performance verification (2026-10-25): processing perspective spans as
bounded runs saves7.21% world and8.39% raster guest instructions on the accepted
route, and4.78%/5.97% in Deep Dark. All400 route and59 Deep Dark six-call windows
improve; sampled states, images, contexts and geometry counters match. A
follow-up raised the maximum span from16 to32: route world/raster instructions
fall1.82%/2.15%, while Deep Dark falls0.49%/0.61%. All sampled states, contexts
and geometry counters match. The route has19 small late-frame timing regressions
and sparse pixel differences (at most60/76,800 indexed pixels in one capture);
Deep Dark has no timing-window regressions and converges to an identical final
image. These are controlled emulator instruction counts; native firmware
pacing, physical M7 cycles and sustained hardware60FPS remain unmeasured.
Earlier individual raster optimizations and their results are retained below.

Current correctness release (2026-10-25): Garden's existing galaxy-menu trigger
now reaches its visible doorway. Real headless walking reproduced the old spire
trap and confirmed the corrected transition; roof/side probes stay inactive.
The correction changes only eight asset bytes (one object's position/default
target), and native, Chromium, Firefox and GWProv outputs match. The later span32
raster executable is separately validated by headless route/Deep Dark replays.
Current correctness artifacts: build/perf/2026-10-25-spire-invisible/.
Coverage-outline is archived and unpromoted; current renderer source includes
the validated depth-specialized path and the measured span-run raster. REA findings/limits:
build/perf/2026-10-25-rea-audit/findings.md. Review VM295208 was closed by the user;
its daemon stopped and profile/capture retained. Do not reopen automatically.
Mario's eye texture correction is included; face geometry remains unchanged.

## Display target and memory

- The current world raster is **160×120**, with a 320-byte indexed stride.
  The port expands it exactly 2× to the 320×240 RGB565 LCD output and composites
  the HUD at native output resolution. Legacy menus retain their original
  240×160 art coordinates and scale to the LCD.
- `src/graphics/frame_dimensions.h` is the shared world-resolution control.
  A 320×240 world experiment was too slow; retain 160×120 for current performance
  work. Differential raster tests cover both sizes for later native experiments.

Retro-Go reserves two 320×240 RGB565 framebuffers (300 KiB) in 1 MiB AXI SRAM,
leaving 724 KiB for RAM_EMU. The current `-O3` raster build loads 78,236 bytes
of code/data and 602,716 bytes of BSS there, plus 47,792 bytes of hot rendering
code and the span-step table in ITCM. Cold game code and generated assets remain
mapped XIP sidecars. The game
retains a76,800-byte indexed framebuffer and a114,688-byte triangle arena.
Geometry limits are5,120 points/8,192 visible triangles/3,584 draw entries.
The optional depth path reserves38,400 bytes for pixel depths and43,008 bytes
for per-entry reciprocal depths.
No firmware rebuild is part of this workflow.

The STM32H7B0 also has separate 64 KiB ITCM, 128 KiB DTCM, 128 KiB AHB SRAM,
and 32 KiB SRAM4. The SDK uses distinct pools and respects firmware reservations.
Latest startup queries in the preserved baseline GWemu profile report:
RAM_EMU **32,776 B**, ITCM **17,744 B**, DTCM **22,528 B**, AHB **21,036 B** free.
These are largest available allocation spans after startup, not pool totals or
hardware measurements. A bounded approximately28KiB chunk-table slab in RAM_EMU covers
Observatory's22,496 bytes; larger stages retain canonical XIP tables. There is a32KiB immutable
source-vertex cache and 54 KiB texture cache in DTCM. Textures get allocation
priority; source coordinates fall back to 16 KiB if needed. A reusable 64 KiB
AHB allocation holds the compact point-to-slot map (or canonical home-table
cache with compact claiming disabled), preserving
a16KiB free span and releasing its owned block on shutdown. Cache allocation
failures retain canonical/AXI paths. The chunk cache preserves at least 32 KiB RAM_EMU
headroom, DTCM caches preserve at least 16 KiB free after firmware reservations.
The preserved firmware reserves its DTCM stack separately: redzone 0x2001b000,
stack 0x2001b020..0x20020000. Never reclaim that window.

This is not a 1 MiB total RAM limit. Likewise the GBA's 256 KiB EWRAM excludes its
other memory regions and cartridge ROM. See AQ and later measurement entries
below for current cache hit rates and subsequent changes.

The game runtime mainly uses fixed, compile-time BSS arrays rather than a
general-purpose heap. Stage geometry and figures are read in place from the
mapped asset sidecar; bounded working sets hold visible geometry, decoded
poses, collision state, and a 54-slot texture cache. The homebrew caches the
sidecars into external flash and applies relocations there. There are no
runtime general-heap `malloc` calls in the game loop. The linked BSS and
SDK's bounded startup pool allocations together determine the RAM budget.
A historical two-second GWProv/GWemu sample of an earlier native-resolution build observed 60.56 game-loop
increments per wall second and 45.5% emulator CPU; the earlier 160×120 run
observed 60.02 increments per second and 29.5% emulator CPU. In the native
build, 31.25% of sampled program counters resolved to `engine_raster`. These
are virtual timing and sampled-PC observations, not STM32 frame-time claims.

## Assets and storage

The generated game assets are distributed in `smg_assets.xip`; executable game
code is in `smg_code.xip`; the GWHB entry point and hot runtime code are in
`SMG.bin`. The linker places assets in separate sections and removes unreferenced
sections. The current asset sidecar is32,750,308 bytes; the water blend lookup
occupies64KiB of that sidecar and adds no framebuffer or runtime data buffer.
Current binary sizes/hashes are recorded in `build/perf/2026-10-25-spire-invisible/bundle.json`.

The standard GWRG bundle has a root `manifest.json` and the named files. Both
sidecars are declared `mapped`. `smg_code.xip` has `relocBase: 0xDEC00000`;
`smg_assets.xip` has no `relocBase` because its opaque payload must not be
scanned as arbitrary words. A scan of an earlier 31,084,812-byte payload found
4,344,091 distinct non-pointer word values; their largest gap is 62,722 bytes,
so no sentinel window large enough for the sidecar avoids false matches. The
build now emits exact pointer sites for both XIPs, including GNU ld's RAM
far-call veneer literals. Numeric rodata in the code XIP can also resemble code
pointers: a generic scan changes 14 non-pointer words in the current corpus.
The homebrew caches assets first, patches their exact sites, then caches code
and patches its exact code and asset references. The build checks its generated
tables against the final ELF. GWProv's SD pre-cache uses the new exact code
table when present. Normal GWProv project installation with --variant flash
installs the declared mapped sidecars; current tests need no manual memory
patch or firmware rebuild. The bundle is built
with the vendored dist-spec manifest and ZIP scripts; it adds no
project-specific bundle format.

The browser converter reads the user-supplied disc image through the optional
processor v1 `range-input` extension, so it does not allocate the >4 GiB ISO in
WASM memory. It extracts selected files, builds and packs the assets, then emits
the canonical XIP. See [`WASM_CONVERTER.md`](WASM_CONVERTER.md) for protocol,
measurements, and remaining production-host integration. Do not treat this as a
reason to build Retro-Go firmware.

The project image currently verified as Super Mario Galaxy USA RMGE01 Rev.00
has SHA-1 `a36105c9fbfff6041dc5babf4318c6a216ad470a`.

## Release acceptance review (2026-10-25)

The current review build is `2026-10-25-texture-address`, not the unvalidated
signed32 depth-setup prototype. The prototype is archived under
`build/perf/2026-10-25-depth-setup/`; the production source was restored to the
verified release snapshot. Review and agree the remaining scope before treating
performance regression results as release acceptance.

| Area | Current evidence / remaining work | Acceptance |
| --- | --- | --- |
| Rendering correctness | Original UV transpose and culling discrepancies fixed; depth, perspective, opaque-material repairs, palm UVs, platform winding and two Honeyhive rock repairs validated in targeted tests. Water blends continuously. | User review of corrected Mario eye proportions and Deep Dark shutter bounds, buildings, foliage, ground, water and curved/distant surfaces; no unexplained disappearing faces, ordering errors or severe texture deformation in supported content. Previous-build image matches preserve behavior but do not independently prove every view correct. |
| First-launch settings | Current review VM has depth On, mapping Accurate, High detail and 60 FPS. The new candidate defaults depth/mapping to On/Accurate and passes clean-start plus all four saved-setting combinations across reboots. | Implemented and headlessly validated. Retain the saved-choice override behavior; the review VM remains on its prior build. |
| Visual policy | World remains160x120, HUD/LCD320x240. Camera clip scales still use1648/2472 (original3:2), yielding a1.125 axis-scale mismatch at4:3. Far geometry remains simplified authored geometry; water uses grayscale converted material without Wii tint/refraction/environment mapping. | Review aspect/FOV and agree acceptable source-mesh/material limitations. Do not classify authored simplification as an achieved Wii-equivalent reconstruction. |
| Collision and gameplay | Precision and known floor/seam defects repaired; local ray tests and recordings pass. Broad mission traversal is incomplete. | No unintended falls through supported walkable surfaces; verify jumps, moving platforms, swimming, gravity changes, damage and death/retry. |
| Audio | GNW audio functions in src/audio/audio.c are no-ops; submit_audio emits silence. Template movement beeps are suppressed. | Implement and verify actual music/effects, mixing, looping and rate/pause/sleep behavior at30/60 FPS. Current silence is missing functionality, not a completed audio repair. |
| Transitions and stability | Automated stage/arrival coverage and the accepted route pass without faults or capacity loss in tested cohorts. | Exercise mission selection, arrivals, star collection, return to hub, death/retry and pause/resume; longer active-gameplay soak with geometry-loss/fault checks. |
| Progress and saves | SaveState/LoadState callbacks return false; collected-star state is held in g_game and settings persistence is separate. | Agree whether progress persistence is required for the release. Implement/test it if required; full snapshot save states are a separate scope decision. |
| Hardware performance/memory | Two passive 60-second GWProv hardware captures in Freezeflame at target30 show about18 FPS and53.2 ms game work/frame; DTCM LUT is active and exact. Allocator spans, actor count and geometry high-water marks stayed flat. User-reported movement-dependent growing lag was not reproduced. Sustained60FPS is unproven. | Reproduce the growing lag under representative movement; sample frame work, actor count and free spans during that route. Measure busy-scene performance at30/60, verify no geometry loss, safe pool/stack headroom and sleep/wake. GWemu host speed cannot pass this item. |
| Conversion/install | Native, Chromium, Firefox and GWProv asset bytes match; normal exact-relocation bundle tested. | Smoke-test the final release bundle from a clean profile/config. GNW web-builder production Page preview remains unverified; do not modify its concurrent worktree casually. |

The accepted Observatory recording remains the general regression baseline:
`build/perf/player-route-20261023-165706/route-02.tl`. More useful recordings are
focused, repeatable routes, not aimless movement or a corner-bound synthetic run:

| Recording | Purpose |
| --- | --- |
| Honeyhive visual/collision patrol | Buildings, trees, repaired rock corners, staggered paths, jumps and changing camera angles; include actual travel rather than standing still. |
| Beach Bowl shore-to-water-to-land | Water coverage/blending, foreground occlusion, swimming and camera transitions across the water surface. |
| Deep Dark active gameplay | Traverse a busy view with actors/switches and camera movement after arrival; complement the measured expensive opening with representative input. |
| Freezeflame curved/ice route | Run around the previously invisible donut/curved area and change viewpoint, covering gravity and material boundaries. |
| One complete mission lifecycle | Selection/arrival, relevant gameplay, death/retry where practical, star acquisition and return; separate captures are fine when one continuous route is inconvenient. |

For each recording, preserve an immutable initial profile, exact homebrew/assets
and emulator identity, settings, stage/mission, RTC/timing and initialization
procedure alongside the timeline. Capture important views and relevant counters.
A timeline alone is insufficient, and the portable position/camera restore is
not a full game-state snapshot. Replaying must first reproduce the known baseline;
then compare player/camera/progression, images and capacity/fault counters before
accepting timings. Exclude startup and final pause. Native input recordings use
GWProv's `record_timeline` launch option; the current review VM is not recording.


## Current state

- The current bundle is built with the SDK Docker image and the standard
  `make_manifest.py` / `make_bundle.py` scripts. Its root entries are
  `manifest.json`, `SMG.bin`, `cover.jpg`, `smg_code.xip`,
  `smg_asset_converter.wasm`, and `smg_core.elf`. The manifest declares
  `range-input:1`; the strict USA ISO input produces `smg_assets.xip` beside
  the homebrew at install time. GWProv imported and hash-checked this ZIP and
  accepted the ISO through its streaming input gate. A per-install 10-trillion
  fuel budget completed the conversion and staged all three homebrew files;
  the asset sidecar matches the native XIP SHA-256. GWProv installed those
  artifacts into a disposable clone of the known ABI-v2 SD profile.
- On 2026-10-23, I independently verified the current runtime binary through
  GWProv's normal ZIP installation path. A disposable GWRG test bundle carried
  the current `SMG.bin`, code sidecar, and native asset sidecar; GWProv installed
  it into a cloned ABI-v2 SD profile without touching firmware. Headless GWemu
  reported `Application: Running: smg_core`; a 640×480 capture showed the
  Observatory world and HUD after runtime XIP caching. The temporary profile,
  bundle, and capture were removed after the check. This validates current
  runtime packaging and boot.
- An initial mixed code-and-data sidecar failed because generic 32-bit-word
  relocation changed Thumb instructions. The split artifacts avoid scanning
  opaque asset bytes. Linker-generated relocation-site tables patch the 9,511
  internal asset pointers and 512 code-to-asset references. The code sidecar
  retains the dist-spec `relocBase` contract.
- The app exports `smg_code_xip_runtime_base` and
  `smg_assets_xip_runtime_base`, matching the ELF sections `.xip_smg_code` and
  `.xip_smg_assets`. GWProv's standard runtime-pointer rebasing resolves both
  sections after cache load; a clean GWemu run reported bases `0x91DA6000` and
  `0x90000000`.
- A first SD test used a flash-only firmware profile with an SD card attached;
  it could not launch SD homebrew. The verified run uses the existing
  ABI-v2 SD firmware profile `tgbdual-gwemu-test`, cloned under `tmp/` and
  configured through GWProv. No firmware was built.
- The managed SD profiles `tgbdual-gwemu-test` and `smg-render-160x120-sd`
  currently contain the unpatched GWLB initializer (zero CRC) and no declared
  `reservedOffset`. The official bootloader copy also has a zero extflash-size
  marker, so this legacy profile falls back to cache base zero. It is suitable
  only as a disposable GWemu test image, not evidence that cache writes protect
  pre-existing NOR contents. GWProv now rejects it for host pre-caching; use a
  profile whose bank-2 GWLB record is patched and CRC-valid.
- On 2026-10-23, GWProv autostart pre-caching was extended to include the SMG
  executable and both XIP sidecars. A disposable profile with a valid bank-2
  GWLB record declaring `reservedOffset=0x200000` seeded all three entries at
  `0x90200000`, `0x9020C000`, and `0x91FB2000`. A 2 MiB sentinel below X remained
  unchanged. Headless GWemu reported `Application: Running: smg_core` and showed
  gameplay immediately, without a “Caching game file” screen. The first run
  exposed a cache-key mismatch: the firmware hashes an 8-byte `time_t`; GWProv
  now uses the same width. A GDB-observed runtime key vector and nine focused
  GWProv tests verify the correction. The disposable profile and screenshots
  were removed. I repeated the headless boot using GWProv's profile launch
  specification and GWemu QMP: the initial scene and eight successive Select
  transitions each produced nonblack 640×480 frames, and QMP still reported the
  guest running. This exercises the normal stage cycle through indices 0–8;
  hub rooms remain outside that cycle.
- A follow-up autostart test found that PyFatFS's displayed short names can
  differ from FatFs's `FILINFO.fname` spelling. For example, the firmware sees
  `SMG.bin` and `smg_code.xip` because of FAT NTRes lowercase flags, while
  PyFatFS reports uppercase short names. Retro-Go compares the startup path
  with `strcmp`, and hashes that exact path for its cache lookup. GWProv now
  derives firmware-visible names from valid long names or the short-name case
  flags, while retaining PyFatFS's own path for reading each input file. With
  this correction, GWProv wrote `/homebrews/SMG.bin` to `/CONFIG` and seeded
  exact cache keys for the executable and both sidecars. The 15 focused GWProv
  tests pass; headless GWemu entered active gameplay directly with health 3,
  and each stored cache key matched the firmware-visible path, mtime, and size.
- A fresh install from the current bundle streamed the 4.38 GiB USA ISO through
  GWProv with a 10-trillion fuel budget and installed all three homebrew files.
  Readback from the SD image matched the expected SHA-256 for `SMG.bin`,
  `smg_assets.xip`, and `smg_code.xip`; the cache index contained valid entries
  at `0x90200000`, `0x9020C000`, and `0x91FB2000`. The source profile had the
  known zero-CRC GWLB placeholder, so GWProv correctly refused to pre-cache it.
  I applied GWProv's own `patch_layout` provisioning routine to the disposable
  clone with a test-only `reservedOffset=0x200000`, then configured autostart
  and seeded the cache. That literal was not derived from an extflash scan: the
  inherited profile's image yielded no recognized partitions, and this run only
  verified that the seeder honors the patched boundary. The matching 2 MiB
  prefix hash is not evidence that this is the correct boundary for a stock
  layout. Headless GWemu reported `smg_core` running and captured a nonblack
  640×480 gameplay scene with HUD. The emulator, disposable profile, scratch
  directory, and screenshot were removed afterward.
- A read-only scan of the live `DEVICE` extflash found Zelda Assets at
  `0x00000000..0x00400000`, FrogFS beginning at `0x00400000`, and LittleFS at
  `0x03800000..0x04000000`; no FAT partition or internal-flash backup was
  detected there. For a layout preserving the stock Zelda assets, both
  web-builder and GWProv derive `X=0x00400000`: web-builder reuses the aligned
  FrogFS start, and GWProv's SD-profile calculation reaches the same boundary
  from the Zelda asset end and model floor. Both scanners recognize Mario/Zelda
  asset regions, relocated internal-flash backups, FAT, FrogFS, and LittleFS;
  FrogFS and LittleFS are excluded from the fallback maximum end of occupied
  non-filesystem regions. Web-builder prefers an aligned existing FrogFS start;
  otherwise it rounds that maximum (including the detected Mario/Zelda model
  floor) up to the erase block. GWProv's `next_free_offset` uses the same
  non-filesystem regions and model floor, but rounds up to at least 1 MiB or the
  erase block, whichever is larger. Web-builder can also choose zero when its
  Retro-Go flow replaces the stock firmware. The live bank-2 GWLB record
  currently has `reservedOffset=0` and lacks the reserved-boundary flag, so it
  does not yet declare this scan-derived 4 MiB boundary for GWProv's cache
  seeder. Earlier 2 MiB cache tests used a manually patched test boundary and
  do not verify preservation of the live device's stock extflash layout.
- The GWProv CLI path matters. Creating Retro-Go from a Mario/Zelda stock
  profile passes that profile's extflash image and model into `create_profile`,
  so GWProv scans the image and patches the resulting boundary into bank 2's
  GWLB record. Creating from generic `--content` does not pass either value;
  it uses the default zero floor, appropriate for a blank emulator image but
  not sufficient evidence for preserving existing stock NOR. Use the stock
  profile migration path when producing a hardware-targeted profile. I
  exercised the scan calculation against a GWProv-created Zelda stock profile
  from the registered stock backup: the scan found Zelda OFW at `0..4 MiB` and
  `_profile_reserved_offset()` returned `0x00400000`. The disposable profile
  was removed.
- Repeated the GWProv autostart-cache and GWemu integration check using
  `X=0x00400000`, derived from that live scan. GWProv's `patch_layout` wrote a
  CRC-valid GWLB record with the reserved-boundary flag into a disposable SD
  profile. The executable, assets, and code XIP were seeded at
  `0x90400000`, `0x9040C000`, and `0x921B2000`. A 4 MiB sentinel prefix hash
  matched before and after seeding. GWProv's headless launch specification and
  QMP stdio reported the guest running after eight seconds; a 640×480 capture
  had 306,916 nonblack pixels. The profile and capture were removed. This
  verifies cache placement and boot on a layout using the scan-derived X, but
  the disposable profile does not reproduce the live extflash contents or its
  currently undeclared GWLB boundary.
- An early full-profile experiment incorrectly included SMG's installed-project
  marker in the firmware input tree before profile assembly. That nonstandard
  order put its ELF under `debug/apps/` and made cache seeding fail. The normal
  flow creates the stock-based Retro-Go profile first, then installs SMG as a
  separate GWRG project; the check below validates that supported sequence.
- Ran the supported stock-profile → Retro-Go SD profile → GWRG project install
  → autostart cache flow end to end. GWProv scanned the Zelda backup and wrote
  a CRC-valid GWLB record with `reservedOffset=0x00400000`, then converted the
  USA ISO with a 10-trillion fuel budget and installed the executable and both
  XIP sidecars. Setting autostart pre-cached them at `0x90400000`,
  `0x9040C000`, and `0x921B2000`; GWProv found the ELF under `debug/projects/`.
  The 4 MiB stock-asset prefix hash stayed
  `cad7e32b0783250c29b4684fda0c8cd5b1a11e6c000ca8dd5633fd32ebffcaed`.
  Headless GWemu reported `running` and rendered 306,916 nonblack pixels in a
  640×480 frame. All disposable profile, source, cache, and capture artifacts
  were removed. This verifies the standard GWProv path against the registered
  Zelda stock backup; the live device itself still has an undeclared boundary.
- Added an `Enter Astrodome` route to the existing Galaxy Test Menu, then rebuilt
  `make local-bundle` with the SDK Docker image. GWProv pre-cached the rebuilt
  executable and XIP sidecars into a disposable profile; headless GWemu opened
  Astrodome from the menu and rendered its 640×480 scene while QMP remained
  running. GWProv first rejected the clone's stale ELF because its code XIP
  section was 28 bytes smaller than the new sidecar; replacing the clone's
  symbol ELF with the matching build allowed seeding. The clone and capture
  files were removed.
- A headless Honeyhive mission-1 run selected through the Galaxy Test Menu
  remained active through about 50 seconds of gameplay, movement, and idle.
  QMP reads confirmed `stage_index=1`, player health stayed at 3, and Mario's
  position changed after direction/jump input. `on_ground` changed from true
  to false on jump and returned to true after four seconds; QMP still reported
  the guest running. This verifies movement and landing on the stage geometry,
  not enemy/hazard damage or stage exits. The disposable profile was removed.
- Reinstalled the current bundle into a disposable profile through GWProv,
  supplying the verified USA ISO. The generated asset XIP matched the native
  sidecar hash, and the code XIP matched the bundle. GWProv pre-cached all
  three files at a disposable profile's manually patched test boundary
  (`X=0x200000`); the 2 MiB prefix hash was unchanged before and after
  seeding. This does not test the live device's scan-derived 4 MiB boundary. A guest-clock timeline entered Good
  Egg mission 4, and QMP confirmed `stage_index=2` with the time limit active.
  At appear-animation frame 29, OAM entries 10–15 had the affine flag and the
  headless capture showed the enlarged timer. A later stable countdown sample
  had no affine entries. The disposable profile was removed.
- In an earlier runtime-copy test on the legacy profile, GWemu cached both
  sidecars, reported
  `smg: raw assets 31084812 bytes`, `smg: code XIP 1746804 bytes`, and entered
  `app_main`. A live screenshot shows Mario in the game scene. The G&W template
  maps its physical Select/Start buttons to `ODROID_INPUT_Y`/`ODROID_INPUT_X`;
  GAME/TIME are separate firmware controls. The homebrew now maps Y/X to the
  game's Select/Start keys. Select presses advanced `g_game.stage_index` from
  0 through 4, and every scene rendered. The first transition exposed a
  precise bus fault at `BFAR=0x040000B8`, the GBA DMA0 control register in
  `star_get.c::wipe_vblank`; skipping that GBA-only transition effect allowed
  stage 1 to load. Stage 2 then exposed a second precise bus fault at
  `BFAR=0x050003E0`, a GBA OBJ palette write in the pointer overlay path. The
  G&W build now backs OBJ tile, palette, and OAM memory in software. Its RGB565
  compositor preserves the 160×120 scene renderer and draws original-resolution
  HUD sprites on the 320×240 LCD output. HUD sprites and transitions through
  stage 4 rendered in GWemu without a fault.
- A fresh GWProv install from the current local GWRG bundle was launched on
  2026-10-21 as `smg-gwemu-01` from a duplicate of `tgbdual-gwemu-test`.
  GWProv's symbolized diagnosis found the live PC in `app_main` under
  `common_emu_sound_sync`; the app's `g_game.stage_index` was 2 and the display
  had completed the cache screen and rendered a nonblack scene. This instance
  remains available at `tmp/gwprov-profiles/smg-gwemu` for continued testing.
- The 2026-10-21 renderer update changed the regular indexed raster target to
  160×120 across C, ARM assembly projection/clipping, and the LCD blit while
  retaining the 240-byte stride and 240×160 storage. This doubles active raster
  pixels from 120×80 without growing BSS. Docker build, standard bundle
  packaging, and GWProv install all succeeded. A fresh SD profile reached the
  live game scene in headless GWemu; a two-second GWProv sample observed 60.02
  game-frame increments per wall second and 29.5% emulator CPU. This is virtual
  test evidence, not an STM32 speed claim.
- On 2026-10-22, the Docker build raised the C and ARM assembly rasterizers
  to 320×240, set the indexed framebuffer stride to 320, and changed the LCD
  blit to sample the world 1:1. Linked BSS rose by 38,400 bytes to 419,732;
  static entry/code/data/BSS totals 464,656 bytes and leaves 276,720 bytes in
  the measured RAM_EMU budget. A clean GWProv SD-profile install booted in
  headless GWemu. Controller input moved Mario; SELECT advanced the game to
  stage index 4. Stage 2 and stage 4 captures were 640×480 emulator output,
  nonblack, and showed the scene and HUD filling the frame. A two-second sample
  observed 60.56 game-loop increments per wall second, 45.5% emulator CPU, and
  31.25% of PC samples in `engine_raster`. This is virtual performance
  evidence, not an STM32 speed claim. The live instance is
  `smg-render-320x240-headless-01`. Sustained gameplay, collision behavior,
  device frame time, and real hardware remain unverified. Audio is a no-op.
- A 2026-10-22 stage-coverage run found another GBA-only memory access in
  `oceanbowl_color_filter()`: writes to `BG_PALETTE` at `0x05000000` caused a
  precise HardFault in stage 6. The G&W target now backs the background palette
  in software, initializes it from the game palette, and synchronizes palette
  changes into the RGB565 compositor each frame. Docker build and standard
  bundle packaging succeeded. A fresh GWProv SD profile rendered the normal
  320×240 Observatory scene after first-run XIP caching; stages 6 (Buoy Base),
  7 (Deep Dark), and 8 (Gusty Garden) then reached rendered gameplay without
  the palette fault. The updated core has 420,244 bytes of BSS and leaves
  276,200 bytes in the measured RAM_EMU budget. The live instance is
  `smg-palette-320x240-headless-01`; later stages and sustained collision and
  underwater-color behavior still need testing.
- A fresh 2026-10-22 Docker `make local-bundle` produced a 42,005,941-byte
  standard GWRG ZIP. Its manifest declares ABI v2/minimum size 888, `SMG.bin`
  (47,109 bytes), the 1,746,884-byte code XIP at `relocBase: 0xDEC00000`, and
  the 31,084,812-byte asset XIP without a relocation base. The XIP hashes
  match the checked-in sidecars. GWProv validated the ZIP and installed all
  three artifacts into an isolated SD profile under `tmp/`. The bundle booted
  in headless GWemu; after first-run XIP caching, a 640×480 emulator capture
  showed the 320×240 game scene and HUD. A QMP right-direction input changed
  Mario's rendered pose. GWProv's managed launcher was unavailable because
  its daemon was draining around other active VMs and a hardware session, so
  this isolated run used GWemu's loopback QMP directly. This verifies the
  current native-generated bundle path, not physical-device performance.
- The GWHB target links the game unity translation unit with platform
  adapters. GBA-only ARM attributes/sections are guarded, Thumb-2 C replacements
  cover ARM math helpers, and the `__GNW__` path supplies C fallbacks for
  visibility, transforms, stage/figure dispatch, and the indexed 320×240
  triangle rasterizer. Stages 0–8 and the HUD rendered through the stage-cycle
  control; the debug menu now exposes the Astrodome hub separately. Remaining
  fallbacks still need broader gameplay validation. Audio is currently a
  no-op.
- Packed poses decode into bounded C scratch slots instead of copying
  executable decoder code into GBA VRAM. The current build fits game state and
  code in the GWHB RAM budget, while the linked game code/assets execute from
  mapped flash.
- The preceding Docker build added affine transforms to the software OBJ
  compositor and avoids carrying sprites over full-resolution scenes. It
  linked with 381,332 bytes of BSS and produced a 1,746,812-byte code sidecar
  and standard bundle. A current-bundle headless mission-4 run verified the
  affine comet timer appear animation in GWemu.
- The linked WASM converter runs extraction, asset compilation, canonical
  packing, and final XIP assembly from the verified USA ISO. Chromium 153 and
  Firefox 155 each produced the exact native sidecar from all 597 selected
  files. The shared `range-input:1` host protocol is implemented in GWProv and
  GNW web-builder. GWProv completes conversion with the per-install
  10-trillion fuel override; the generated sidecar matches the native XIP.
  See [`WASM_CONVERTER.md`](WASM_CONVERTER.md) for measurements and remaining
  integration work.
- On 2026-10-23, rebuilt the standard offline bundle with Docker
  (`retro-go-sd/build/smg/local-bundle/smg-local-bundle.zip`, 27,319,388 bytes).
  `gwprov project install --wasm-fuel 10000000000000` validated the supplied
  ISO through bounded streaming, completed conversion, and staged the package.
  The direct install into a disposable clone of `tgbdual-gwemu-test` succeeded.
  Its generated `smg_assets.xip` matched the native sidecar byte-for-byte
  (`ef7875b6eca7e7f9b4f43f00312571e847a9ded1b5411f87639c52832dfcd7c0`).
  The per-install override leaves the default at 500 billion and caps overrides
  at 10 trillion. Build intermediates and completed scratch outputs were
  removed; the reproducible ZIP remains under ignored `build/`.
- A further 2026-10-23 GWProv install used that current ZIP and the verified ISO
  on a disposable profile. The cache began at a CRC-valid, manually patched
  test boundary `X=0x200000`, pre-caching the executable, assets, and code at
  `0x90200000`, `0x9020C000`, and `0x91FB2000`; the 2 MiB extflash prefix hash
  remained unchanged. Headless GWemu entered Honeyhive mission 1, loaded 40
  actors, and responded to movement. A debug-selected planet start followed by
  movement returned Mario to the mission start with health 3. This run did not
  establish enemy or hazard damage, which remains unverified.
- A follow-up used a fresh disposable profile and a temporary standard GWRG
  bundle containing the current executable and both generated XIPs, installed
  and pre-cached by GWProv at the bank2 GWLB boundary. The headless GWemu
  timeline entered Good Egg mission 1 (stage 2, scenario 1). QMP read 87 actors
  at spawn and 89 after movement; Mario's position changed, while health stayed
  at 3. Headless captures show movement across the planet surface. This route
  still does not establish enemy contact or damage. The disposable profile and
  captures are temporary and are removed after this run.
- GWProv's registered `DEVICE` raw GDB endpoint accepted an attach-only
  connection. It reported the target running before and after passive reads.
  With the SMG ELF loaded locally, `g_game.stage_index`, `scenario`, and player
  health at the expected addresses were all zero; the PC (`0x240E90CC`) did not
  resolve against the available symbols. This does not establish SMG running
  on the STM32. No reset, flash, or watchdog-freeze write was issued; the
  `where()` inspection briefly halted and resumed execution, and the target
  returned to running before disconnect.
- Rechecked 2026-10-23: `DEVICE` is still running the Retro-Go SD recovery
  stub, not SMG. The local card `/dev/mmcblk0p1` is mounted read-only at
  `<mount point>`; I left it unchanged. Physical install and device
  performance verification remain unavailable until the card is writable and
  the target can launch the homebrew.
- A read-only inspection of that mounted card confirms `/homebrews/SMG.bin` and
  both SMG XIP sidecars are absent, and `/CONFIG` has a valid CRC but no startup
  file. Its existing `flashcachedata.bin` is 4,124 bytes and carries GWemu's
  synthetic UID plus two unrelated cache entries, so it is not a prepared
  hardware autostart cache. Do not copy or reuse that metadata for the SMG
  deployment; the supported GWProv profile flow must stage the bundle, set
  `/homebrews/SMG.bin` autostart, and seed the cache against bank 2's patched
  GWLB `reservedOffset`. This card remains mounted read-only (`errors=remount-ro`);
  no write or remount was attempted.
- Revalidated the current bundle in a fresh clone of the preserved GWProv profile.
  GWProv converted the USA ISO and installed the current executable and XIPs;
  SD readback SHA-256 values matched the current bundle/build outputs. Using the
  scan-derived `X=0x00400000`, GWProv wrote cache entries at `0x90400000`,
  `0x9040C000`, and `0x921B2000` (32,878,833 bytes total). The cloned bank-2
  GWLB record had a valid CRC, extflash size 64 MiB, `reservedOffset=4 MiB`, and
  flags `0xF`. The first 4 MiB of virtual extflash matched its pre-seed hash
  (`fe2895d20f0e5cf1690b20ead97a65b7ac9816b4b8cf54a97372017444e3828f`). This
  clone did not contain the live device's stock NOR contents, so the prefix
  check verifies the cache boundary but is not a physical preservation test.
- Launched that clone with GWemu headless through GWProv's profile launch spec
  because its managed-start daemon was unavailable in this sandbox. QMP reported
  the guest running; a Right input visibly moved Mario. Eight Select transitions
  advanced `g_game.stage_index` from 0 through 8, with `scenario=1` and the guest
  still running at each sample. All nine 640×480 captures were nonblack and
  showed distinct scenes. The clone and all captures were removed; the preserved
  profile remains untouched.
- Used the current build's Galaxy Test Menu to enter Good Egg / Dino Piranha
  (`stage_index=2`, `scenario=1`). After about 40 emulated seconds, QMP reads
  showed the guest still running, `g_game.scene=SCENE_PLAY`, Mario in `MA_WAIT`
  and grounded with health 3 and no invincibility timer, and 89 loaded actors.
  Three active Takobos were nearby; directional movement changed Mario's world
  position, but health stayed at 3. This confirms sustained stage execution and
  input, but does not yet verify an enemy damage/contact event.

## Hardware relocation crash, 2026-10-23

The Right-input crash on `DEVICE` was a precise BusFault at the texel load
in `src/graphics/engine.c:400`, not a watchdog timeout. The saved core frame at
`0x2001F470` had PC `0x922E071A`; `CFSR=0x8200`, `HFSR=0x40000000`, and
`BFAR=0xEB026DBF`. The asset XIP was mapped at `0x904DB040` and code at
`0x9228014C`.

The installed profile had declared the entire raw asset payload with
`relocBase=0xBEEF0000`. Mario triangle 35 has texture index 5 and flags
`0xBF00`, forming the aligned word `0xBF000005` at asset offset `0xA679C8`.
That word is not a pointer or an ELF relocation site. The generic scan applied
the asset relocation delta and changed it to `0x905EB045`, making the texture
index 45,125. Comparing the preserved source with the installed payload found
38,987 such false asset matches, plus 14 non-pointer matches in the code
sidecar's sine, pose, and image data. The earlier pointer-only verification
missed these changes.

`prepare_flash_profile.py` reconstructs both mapped payloads from sources that
must match the final ELF, patches only recorded ABS32 sites, checks non-pointer
bytes, and updates the FrogFS CRC, component blob, and flash-map hash. It
preserves payload addresses and bytes outside FrogFS. Its second audit of the
repaired original profile reported zero differences. The old runtime then
returned to the main menu because its broad validation scan mistook restored
sine-table data at code offset `0x65764` for an unrelocated pointer.

The loader now patches and validates exact sites for both sidecars and RAM.
The tables contain 9,511 asset self references, 512 code-to-asset references,
2,163 code self references, and four linker-generated RAM veneer literals.
Generated tables follow initialized RAM data so the final link does not move
sites recorded by the preliminary link. Docker built `SMG.bin` at 55,761 bytes
with 53,552 bytes of loaded RAM code/data and 420,240 bytes of BSS; its final-ELF
table check passed. The 31,084,812-byte asset sidecar is unchanged
(`ef7875b6eca7e7f9b4f43f00312571e847a9ded1b5411f87639c52832dfcd7c0`).

GWProv's SD pre-cache now reads the optional exact code table from the ELF,
preserving numeric values in the code sentinel range. Its chunked output
matches both corrected flash payloads byte for byte. Sixteen cache and batch
debugger tests passed, including a collision regression across chunk boundaries.
Typed debug reads also required fixing GWProv's DWARF reader to apply ELF
relocations only to ET_REL inputs: final ET_EXEC DWARF is already relocated.

The corrected loader was installed through a temporary standard GWRG bundle
containing the native generated assets, with both XIPs mapped and no generic
word-scanning relocation declaration. The post-layout script then relocated
only real pointers at the new runtime addresses (`assets=0x904DD20C`,
`code=0x92282318`). Hardware movement and jump validation passed with both fault
registers zero. The corrupt word is restored to `0xBF000005`, and the sine-table
word remains `0xDEC3DF19`. The user confirmed that the game works on hardware.
The converter's asset bytes did not change.

## Current hardware build and paired performance capture

The user selected a 160×120 world raster with the HUD and RGB565 output kept at
320×240. Shared dimensions are in `src/graphics/frame_dimensions.h`; the indexed
stride remains 320 for legacy menus. Docker built this version: `SMG.bin` is
55,845 bytes, with 53,636 bytes of loaded RAM code/data and 420,244 bytes of BSS.
Code XIP is 1,746,864 bytes; assets remain 31,084,812 bytes. The installed profile
is `smg-retro-go-flash-complete`; its runtime bases are assets `0x904DD260` and
code `0x9228236C`.

`smg_render_stats` exposes cumulative rendered frames, game/world milliseconds,
compositor milliseconds and dimensions. The requested paired captures completed
600 samples each at 10 Hz over 60 seconds. Both stayed at the same stationary
Observatory position with the same game ELF, stage/scenario, and no fault flags.
Artifacts are retained in `build/perf/2026-10-23-160x120/`:

| Measurement | Hardware | Default GWemu |
|---|---:|---:|
| Duration | 60.015 s | 60.003 s |
| Rendered frames/s | 31.792 | 59.947 |
| Game + world time/frame | 26.432 ms | 4.333 ms |
| HUD + compositor time/frame | 5.155 ms | 0.791 ms |
| `engine_raster` PC samples | 36.33% | 16.00% |
| `cpumon_common` PC samples | 0% | 67.50% |

Default VM reports 6.16 times less game/render/compositor work per frame. Its
60 FPS output hides excess virtual CPU headroom. Hardware PC sampling used DWT
PCSR without halting; VM sampling used QMP. Hardware CYCCNT advanced continuously
through four wraps at about 281.3 million counts/s (`SystemCoreClock=280 MHz`).
The port does not call the firmware's per-loop reset helper. Default VM DWT
CTRL/CYCCNT read zero, so it supplies no elapsed-cycle measurement. Fixed 10 Hz
PC sampling may alias with a 60 Hz frame clock; shares are hotspot estimates.
Hardware acquisition averaged 9.27 ms, with a maximum sample gap of 208.5 ms;
VM acquisition averaged 3.12 ms.

The optional baseline timing-mode preflight stalled after one frame in
`common_emu_sound_sync`/`cpumon_common`; it was stopped without taking a 60-second
capture. The completed VM run used normal default timing. Reusable tools are
`tools/profile_runtime.py` and `tools/compare_profiles.py`; raw JSON/CSV traces,
comparison JSON/PNG/SVG, and VM launch identities are saved with the results.

The user reports missing/poor meshes on both targets and loud movement tones.
The tones are the leftover template `tone_hz()`/`submit_audio()` path; actual
game audio is still stubbed. Both audio and visual defects remain unresolved.
No audio change was made during measurement.

**Hardware access was relinquished at the user's request.** The debug console
is closed; do not access `DEVICE` again until access is granted. Both benchmark
VMs were stopped and their temporary profile removed. Original BusFault evidence
is saved in `build/debug/smg-right-busfault-2026-10-23.json`.

## Exact relocation distribution integration: unfinished handoff

The user approved optional exact relocation tables in schema version 1, keeping
the existing assets. Changes are present in this project, GWProv, the dist spec,
and web-builder. Preserve unrelated dirty work in those sibling repositories.

The optional `relocations` object specifies method `exact-relocation-sites`,
version 1, and groups of `{target, relocBase, offsets}`. Offsets are aligned
source-file byte offsets and may reference sibling mapped files, including
fixed-name converter outputs. An exact table suppresses all generic scanning
for its source. Old manifests retain their current behavior.

Implemented and checked so far:

- Spec schema/docs/conformance checks accept the fields on artifacts and fixed
  outputs. Existing and new conformance checks pass.
- GWProv has `mapped_relocations.py`, installer propagation, generic FrogFS
  post-layout relocation, profile creation, and bounded generic SD pre-cache
  handling. The selected 31 Python tests pass, including a real converter-output
  install fixture and collision/chunk regressions.
- Web-builder has `exactRelocations.ts`, parser/types, preparation/reload, and
  forwarding in Library, Wizard and advanced installation. Eight mapped-install
  checks, focused exact-site checks, and 495 converter-host checks pass. The
  filesystem package compiled in Docker using isolated outputs because existing
  generated files belong to another UID; do not chown those files.
- `elf_relocations.py` is the pure ELF reader. `package_local_bundle.py` injects
  exact declarations into the standard manifest. Docker `make local-bundle`
  succeeded. The manifest is 340,919 bytes and carries 2,163 code self sites,
  512 code-to-asset sites and 9,511 asset self sites. The ZIP is 27,356,955 bytes.

Before calling the integration complete:

- Compare full generated SMG sidecars through GWProv's normal exact-table install
  path and web-builder's packer, independently against the ELF.
- Finish relevant schema/host checks and review cross-directory/prior-placement
  validation. The full web application build has not been checked.
- Correct stale CLI descriptions that label exact-relocated files as having no
  host relocation, and replace local-repair installation docs with the automatic
  workflow once verified.
- Review profile component coherence when FrogFS rebuilding patches bank2.bin:
  deployment displayed an old 829.9 KiB layout declaration. Do not investigate or
  deploy this on hardware while access is relinquished.

No fresh ISO reconversion or browser Page preview was run for the metadata
change; the converter and canonical assets are unchanged. Completed scratch
outputs and the temporary native test bundle were removed. Recreate that fixture
from current build artifacts if needed; the standard release ZIP remains in
`retro-go-sd/build/smg/local-bundle/`.

## Reference material

- Retro-Go SD homebrew template and examples: `https://github.com/slash-proc/retro-go-sd-templates/`
- `gwprov` provisioning guide: `https://github.com/slash-proc/gwprov/blob/main/docs/PROVISIONING.md`
- GWRG distribution specification: `https://github.com/slash-proc/gwrg-dist-spec/`

## Performance optimization in progress (2026-10-23)

The user granted hardware access again and requested a sustained 30 FPS minimum,
a 30/60 render toggle, and a path toward 60 FPS. Work is in
`build/perf/2026-10-23-optimization/`. Baseline and intermediate ELFs/binaries
are archived there for accurate attribution. Do not claim the stationary result
proves all stages or movement meet the minimum.

Build A changed game compilation from `-Os` to `-O3 -fno-strict-aliasing`, moved
selected render/visibility functions into RAM_EMU, specialized the exact 2×
RGB565 upscale, and added a persisted Render rate option (default 30, optional
60; LCD refresh remains 60). Game simulation now uses elapsed active wall time
and updates during skipped draws, so reducing render frequency does not slow
the game. Pause-menu time is excluded and catch-up is bounded at 100 ms. The
leftover template button beeps are suppressed; real game audio remains stubbed.

Hardware A stationary captures (15 s each) measured 30.256 FPS at the 30 target
and 52.038 FPS at 60. At 30, work averaged 19.906 ms, versus the previous 31.6 ms,
and none of 454 measured frames exceeded 33.3 ms. At 60, work averaged 19.122 ms.
Nested cycle counters expose logic/world/visibility/transform/emission/raster/
palette/scaling/HUD time, with 2 ms work histograms, over-budget counts and the
last 256 frame work times. The debug ABI is `smg_perf` version 1; the existing
`smg_render_stats` prefix remains compatible. DWT is read without resets.

Build B eliminated four 64-bit mip divisions, skipped UV gradients for flat
materials, and used exact wrapping 32-bit UV accumulators. Build C additionally
solves triangle half-plane bounds per scanline, eliminating per-pixel bounding
box edge tests. All changes preserve the old coverage and texture sampling:
`python3 tools/test_raster.py` runs 24,000 byte comparisons at each of 160×120
and 320×240 under Docker/AddressSanitizer, including extreme coordinates,
negative UVs, mip thresholds, blending, null textures and degenerate triangles.
Build the test image with `docker build -t smg-runtime-tests -f
tools/tests/Dockerfile .`. Both sizes pass. VM C completed 600 samples at 10 Hz
over 60 s, with 60.031 FPS and 2.456 ms game/world work per frame. Hardware C
validation and movement captures are still pending at this point.

`retro-go-sd/scripts/package_native_bundle.py --out tmp/.../smg.zip` packages
already-converted assets through the standard exact-relocation GWRG installer.
Both complete installed sidecars independently match the final ELF (zero
differences) through `prepare_flash_profile.py`; no ISO conversion is needed
when only the runtime changes. Production WASM-converter packaging is separate.

Hardware deployment initially stalled with an ERASE mailbox but the live CPU
was in the bank1 bootloader. Two live PC snapshots, VTOR and idle FLASH_SR1/2
proved the mailbox stale; clearing only that SRAM status allowed the normal
`gwprov device DEVICE recover` command to start a verified idle recovery stub
at VTOR 0x240e6800. Evidence: `stale-recovery-mailbox.json`. This does not yet
prove what caused the original write failure. GWProv now checks stale active
mailboxes during explicit recovery using two flash PCs, unlocked state, stable
VTOR/mailbox and idle controllers, preserving real/unknown busy services. Its
24 focused lifecycle/deployment regressions pass; README describes this.

FrogFS rebuild still changes the profile bank2 GWLB declaration without updating
the deployable bank2 component blob. For these isolated test profiles, the
component bytes/hash were synchronized to the corresponding bank2.bin before
apply. Fix this generically in GWProv rather than modifying Retro-Go.

The wider build-C sweep found one remaining budget failure: Deep Dark's opening
camera/movement route exceeded 33.3 ms on 96 frames, although its 15-second
average was 46.69 FPS. The other seven tested stages had no >33.3 ms work frames.
Do not report build C as meeting the minimum across the stage sweep.

Build D adds a GNW-only cached-screen figure path for triangles whose vertices
are inside all guard planes, keeps the old clip callback for crossings, inlines
clip-distance math, snapshots raster mode/colour before framebuffer stores,
and moves additional clip/emission/rank-collection helpers to AXI RAM.
`tools/test_figure_paths.py` compares 40,000 triangles against the original clip
callback: 23,326 cached and 16,674 clipped cases match exactly. Raster checks
pass 24,000 cases at each resolution using the actual mip stride of 64. Test
executables use `-no-pie` for stable ASan shadow placement; recursive signal
reporting is disabled without disabling memory checks. Temporary test files
are cleaned on exit. Build D loads 76,716 bytes and has 420,340 BSS bytes; mapped
code is independently verified against the ELF before deployment. Hardware D
validation is pending at this handoff point.

The performance collector now snapshots frame rings every second to retain
window-wide maximum/p95/p99 work times rather than only the last 256 frames.
It reports missing history explicitly; four unit checks cover early spikes,
ring-counter wrapping, dropped history and unavailable VM cycle counters.
`input_events` injects timestamped shadow masks and releases buttons in `finally`.
Stage-cycle and input traces are recorded, with capture counters sampled at 10 Hz.

## Hardware released again at user request

**Do not access DEVICE until the user grants access again.** All debugger
sessions are closed. The device was left running build D in Deep Dark (stage 7),
at the 30 FPS setting, with input shadow zero and CFSR/HFSR zero. The temporary
RAM point-cache pointer and allocator cursor were restored before disconnect.
`hardware-released.json` records the state. The permanent profile
`smg-retro-go-flash-complete` now matches installed build D, passes the independent
ELF sidecar audit with zero differences, and is the local hardware assignment.
No hardware action is needed for this metadata synchronization.

Latest build-D hardware evidence (600 samples at 10 Hz over 60 s each):

- Observatory 60 target: 60.329 FPS, 13.010 ms mean frame work, 14.469 ms maximum,
  zero frames exceeding either 16.67 or 33.33 ms.
- Deep Dark 60 target: 54.379 FPS, 17.434 ms mean, 35.047 ms maximum; four frames
  exceeded 33.33 ms. Deep Dark 30 target: 30.164 FPS, 18.789 ms mean, 35.078 ms
  maximum; five frames exceeded 33.33 ms. Thus the installed build still has a
  small tail below the requested minimum, despite holding the average.
- A reversible debug-only copy of Deep Dark's 88,768-byte chunk array into
  reserved RAM improved mean work but still had four >33.33 ms frames.
- A debug-only copy of its 178,638-byte point array produced 30.164 FPS,
  17.965 ms mean work, **32.833 ms maximum and zero >33.33 ms frames** across
  1,811 frames. The trial used the firmware RAM bump pool, verified byte-for-byte
  copies, and restored the pointers/pool cursor at release. It is not yet a
  permanent code feature. Next useful work is a bounded reusable stage point
  cache that preserves enough RAM headroom; points were more useful than chunks.

Hardware cycle history covers every measured frame in the latest captures
(no missing ring history). VM D captures marked `-contended` are rejected for
timing comparisons: an orphaned interrupted ASan Docker container contended
with the host. It was removed and the large temporary diagnostic log deleted.
Clean VM repeats report 59.996 FPS in Observatory and Deep Dark. Test launchers
now use unique container names, timeouts and `finally` removal to prevent leaks.

There are additional **unbuilt, hardware-unverified** source optimizations in
`src/graphics/visibility.c`: job-parameter snapshots, 32-bit chunk centers/extents
with exact 64-bit products, and vertex input snapshots. The frozen-reference
`tools/test_visibility.py` passes 4,000 cull/collection/transform jobs, covering
indexed/home/list paths, early exits and extreme planes. Build these only with
Docker and verify headlessly; the installed build D predates them. The standard release ZIP has been regenerated from the already-built D
artifacts without compiling the pending visibility edits. Regenerate it again
after those edits and any permanent point cache are built and validated.

Performance artifacts, immutable baseline/A/C/D ELFs, raw traces and summaries
are in `build/perf/2026-10-23-optimization/`. All test VMs are stopped and their
project-local scratch profiles/bundles have been removed. No Retro-Go firmware
was built or modified. GWProv's stale-active-mailbox recovery fix and its 24
regression checks remain in the sibling checkout.

## Performance and visual audit after player-recorded route

The user rejected the old synthetic movement as representative: it traps Mario
in a corner with the camera pointing down. Retain the old measurements only as
costs of those specific workloads. The accepted GWProv timeline is
`build/perf/player-route-20261023-165706/route-02.tl`, with its immutable initial
profile beside it. It has 148 input events and a clean quit at 51.731 guest
seconds; exclude startup and the final pause from gameplay timing. Temporary
recording VM files are removed. Hardware access remains relinquished.

`build/perf/2026-10-23-audit/findings.txt` records the full analysis and next
instrumentation; `windows.json` reconstructs interval costs from old counters.
The expensive Deep Dark opening averages 28.424 ms rather than the misleading
whole-run 17.434 ms: a 41.4% reduction is required even for that opening average
to fit 60 FPS. Its worst individual frame is about 35 ms.

**Discrepancies confirmed in the pre-E audit (now fixed in E/F; installed hardware D still predates them):**

- C texture sampling transposes U/V compared with both the asset compiler and
  original GBA rasterizer, for both mip levels.
- `geometry_collect_leaves_c` takes axis-coordinate minima against `facing`
  instead of starting with the first vertex's axis coordinate; this can retain
  triangles the original collector rejects.
- C chunk culling omits original backface-cone rejection. Deep Dark contains
  1,440 enabled cone records among 2,774 chunks. C also omits inherited plane
  masks that avoid repeating satisfied frustum tests.

`python3 tools/audit_render_contracts.py` initially reproduced these failures.
After E/F, all seven independent mip/axis/backface/cone contracts pass.
The scalar differential references now explicitly incorporate the corrected
contracts; they are not frozen originals. The visibility test uses valid nested
BVHs to test inherited plane masks against an independent all-plane traversal.

Other concrete candidates: ordinary triangles all take the GNW slow emitter;
Deep Dark scans 356 objects each simulation tick to find four group-switch
watchers; all reachable build-D projection-LUT entries (indices32..65535)
exactly equal `1048576 / index`, allowing an exact hardware-divider experiment
instead of the 256 KiB XIP table lookup. Savings need measurement. Missing
geometry may involve the silent 2,048-entry draw-arena limit, but no prior
capture measured saturation, so this remains unconfirmed. The default view
mode also deliberately uses flat colours for small/distant triangles and LOD.

## Live visual testing: E, F, G (2026-10-23)

Keep **one visible GWemu instance** running and replace it with each validated
update until the user says to stop. Its profile is `tmp/smg-live`; current PID
and build are in `tmp/smg-live-session.json`. Preserve the user's `gwemu-*.png`
screenshots there. Hardware remains relinquished. The accepted route/baseline
is immutable and must not be overwritten by this interactive session.

E fixed the U/V transpose (the user confirmed Mario improved), triangle-axis
minima, cone culling and plane-mask inheritance. It also built the previously
pending visibility snapshots/wide arithmetic and added `smg_geometry_stats`
capacity/culling/emission/raster counters. F additionally restored original
collector backface rejection and fixed the packed-pose cache: new B could reuse
and overwrite a slot still referenced by new A after a key transition. Cache
hits now stay within their requested input slot. `tools/test_pose_cache.py`
reproduces the old overwrite and passes after the fix. User confirmation of the
sleep animation remains pending.

Docker verification: 48,000 raster cases, 4,000 visibility jobs, 40,000 figure
path cases, seven independent render contracts and the pose-cache regression
passed. E/F archived ELFs, screenshots and passive diagnostics are in
`build/perf/2026-10-23-audit/`. F's user-play sample at 46,857 rendered frames
has **zero** arena drops, visible/point-list/point-pool capacity hits or far-pass
capacity skips; arena peak 326, visible peak 680, point-pool peak 917. These
cumulative, non-atomic reads do not establish frame timing or original-GBA
visual equivalence. Missing surfaces in these views are not explained by
buffer saturation.

G repairs an asset defect in Observatory flower beds: all 36 flower triangles
in the main OBJ replace ground geometry; none has another non-flower triangle
under its centroid. Their PNGs have transparent gaps (276/276/287 zero-alpha
pixels), exposing sky. Flowers and surrounding grass share world-aligned UVs
(maximum discrepancy below 0.00001 UV). Stage-scoped `texture_underlay` entries
in `assets/assets.cfg` bake opaque grass beneath these three textures, before
lighting/palette quantization/mip generation in the shared C compiler. The
palette, geometry and runtime format stay unchanged; near/far Observatory
assets and packed texture pools change. Other packed stage/figure blobs are
unchanged. Partial alpha is composited with integer rounding. Mismatched sizes,
nonopaque backing and duplicate declarations fail explicitly. The real-asset
ASan regression `tools/test_texture_underlays.py` reproduces the original holes
and checks both repaired mip levels plus synthetic blend endpoints/rounding.

G adds the persisted **Scene detail** Retro-Go pause option, default **High**.
It exposes existing Full/High/Default/Near/Short presets: High changes the near
mesh fraction from 0.5 to 0.75, texture-to-flat threshold 14000 to 28000, actor
range 3200 to 5000, and figure depth 50 to 80. Full disables flat-small/distant
texturing and far-mesh substitution, useful for distinguishing LOD defects.
The raster stays 160x120 and HUD/output 320x240. Increased detail is not yet
hardware-performance validated. Render-rate settings remain independent.

G: SMG.bin 80,961 bytes, RAM code/data 78,752, BSS 420,344. Normal GWProv flash
installation into the VM profile passes the independent ELF audit with zero
pointer and non-pointer differences in both sidecars. No firmware rebuilt.
The user confirmed the flower repair. High-versus-Full cliff comparison remains
pending; High alone still shows the distant mesh defects.

Still unresolved: Honeyhive cliff holes, ground painter ordering, richer
textures/LOD, sleep-animation user confirmation and representative 60 FPS.
The user's Honeyhive comparison is archived as `honeyhive-wii-reference.png`
and `gwemu-2026-10-23-17-40-38.png`. Its incorrect space background is already
present in `build/constructed/assets/stages/honeyhive/sky.png`: native
`procedural.py` and Rust `procedural_images.rs` deliberately reuse the same
procedural sky for Honeyhive, its tower and Observatory. It is an asset issue,
not evidence of a bad runtime sky pointer. Restore the appropriate ISO sky
through both converter paths; do not substitute an invented bitmap.

The browser/WASM artifacts and standard release ZIP must be rebuilt after
changing the shared asset compiler/config. The native VM bundle is generated
separately with `package_native_bundle.py`; do not assume the release ZIP
matches that bundle. Current converter rebuild/verification status is recorded
in the follow-up below when complete.

### Confirmed painter-order defect and approved depth prototype H

The user held the Honeyhive view in `gwemu-2026-10-23-18-00-10.png` stationary
for capture. `build/perf/2026-10-23-audit/ordering-g/` contains a coherent QMP
stop/read/resume snapshot, full 1 MiB AXI RAM, camera, stage references, draw
chains and framebuffer-related arrays. VM was resumed in `finally`. The
`far-g/` and `building-g/` earlier captures are passive and not atomic; the
latter lacks metadata after a DWARF lookup failure, so use `ordering-g` for
reliable draw-entry analysis. Preserve G's archived ELF and relocation bases
when reading this snapshot; later installs move flash contents.

Two independent diagnostic scripts are available: `audit_scene_projection.py`
projects the reconstructed OBJ with a depth buffer; `audit_arena_projection.py`
replays captured stage draw chains using installed flash and captured texture
cache. Both use floating-point interpolation and omit actors; they are visual
diagnostics, not bit-exact runtime references. They output per-pixel face/entry
IDs. The arena script needs the matching flash image and relocation asset base.

At raster pixel (70,66), captured ground entry 106 is at z≈4329 while wall entry
119 is at z≈6372. The painter chain draws the wall later, visibly on top of the
closer ground. Entries 108/120 reproduce the same error at (71,81). This is
inherited farthest-vertex bin sorting, not texture conversion or arena overflow.
The far Honeyhive OBJ also has visible holes independently of rasterization:
its 590 triangles replace roughly 6000 detailed triangles. High's near cutoff
is `lut_zdrop[32]=24256`; missing detailed cliff triangles have depths around
29753–39067 and are replaced by that coarse mesh. Depth buffering will not
recreate missing far-mesh surfaces. Honeyhive's original placed sky is
`HalfGalaxySky` (object 14 in HoneyBeeKingdomGalaxy), not the procedural starfield;
that archive is not yet in the selected extraction manifest.

The user explicitly chose **prototype a depth buffer and measure it**. H keeps
the DrawEntry ABI at 32 bytes and stores reciprocal depths in a parallel array.
It adds a 160x120 uint16 depth buffer, 2048x3 uint32 triangle depth values, and
a bounded 256-entry deferred overlay queue. Cutout texels only write depth when
opaque; additive, shadow and water-tint overlays test depth after opaque drawing
and do not write it. Deferred entries snapshot their texture descriptor and
additive lookup pointers. Queue peak/drop counters extend geometry ABI to v2.
Depth is carried through stage, subdivided/clipped, figure fast/clipped and
ocean surface paths. The **Depth buffer** pause option is persisted; its default
is Off while this remains an unmeasured hardware prototype.

`test_depth_raster.py` uses actual captured entries 106/119, reproduces the old
wrong result, verifies the depth result in either submission order, checks
transparent holes and deferred/occluded shadows, and runs ASan/UBSan. Existing
48,000 depth-disabled raster cases and 40,000 figure path cases pass; figure
tests now also compare carried depths. Optimized host fixture: off 4.111 µs,
on 5.071 µs (~23% extra for two triangles plus clears). This is **not** a whole
game or STM32 timing estimate. GWemu DWT readings remain zero; VM wall timing
cannot prove target hardware speed. Hardware remains relinquished.

H builds with SMG.bin 82,997 bytes, RAM code/data 80,788 and BSS 498,724:
78,380 additional BSS bytes over G, including the deferred effect state.
The asset XIP remains 31,080,816 bytes with SHA-256
`4a16dd27c9f5787db39336203169ab39122792b83911ad9804c1e553fa771d8c`.
The rebuilt linked WASM converter matches those bytes end to end from the ISO
in Chromium (116.52 s) and Firefox (116.09 s), both with 8 MiB input ranges.
The standard WASM release bundle was regenerated for H; detailed logs are
`wasm-g-*.log` and `release-h.log`. Runtime H installation and VM measurements
are still in progress at this checkpoint.

### Crash during mission selection: fixed in I (2026-10-23)

The user triggered a hard fault in H while entering mission selection, stage 9.
The depth prototype was **disabled**. Preserved RAM and fault state are in
`build/perf/2026-10-23-audit/crash-h/`. CFSR `0x8200` indicates a precise bus
fault with valid BFAR `0x04000050` (GBA BLDCNT). The exception frame at
`0x2001f9f8` contains PC `0x922a3022`, mapping through H's code base to
`0xdec1b242`, inside `scenario_select_set_fade.constprop.0`.

I guards the GBA blend-register operations in mission selection and the
star-get banner, plus the remaining DMA/display writes when an opening wipe
finishes. This follows the existing GNW guards in `hud_set_fade`,
`wipe_after_flip`, and `wipe_vblank`. It does **not** implement those GBA fade,
blend, or hardware-window effects in the software compositor; that remains
visual work. Scene progression, palette updates, and wipe state still run.
`tools/test_scene_mmio.py` extracts the production functions and exercises
340 mission/comet/fade combinations, star alpha values, and both wipe endings.
Docker ASan/UBSan pass for GNW with the GBA addresses unmapped, and for simulated
GBA registers with register-result assertions.

I: SMG.bin 82,989 bytes; RAM code/data 80,780; BSS 498,724. Asset content is
unchanged. Normal GWProv installation and the exact-relocation audit pass with
zero differing words; no repair or firmware modification was needed. Both the
native bundle and standard WASM release bundle were regenerated. Current VM is
`gwemu:2816729` (GDB 1235), recorded in `tmp/smg-live-session.json`; H's crashed
instance was stopped. The initial H timing run was invalid (launcher, zero
frames). The benchmark now refuses writes until fault-free gameplay advances,
and rejects timing windows with no advancing frames.

The I depth toggle smoke run completed all six 5-second windows with advancing
frames (~59.8–60 VM FPS), zero CFSR/HFSR, no dropped geometry or depth effects,
and an overlay queue peak of 2. It leaves depth **On** for the current session
(the persistent default remains Off). Every window had changing camera/scene
state, so these are **not valid stationary performance comparisons** and cannot
establish depth overhead or STM32 FPS. Evidence: `depth-i-vm.json`/`.log` and
`i-depth-on.png`. Removed the 115 MiB temporary native-install staging directory;
kept the live profile, user screenshots, crash evidence, and release artifacts.

### Observatory platform winding repair J (2026-10-23, validated)

The user's `gwemu-2026-10-23-18-30-10.png` shows staggered platforms around the
Observatory kitchen. `invisible-i/` preserves a stopped-and-resumed GWProv RAM
capture, camera and screen with depth enabled and no fault. The independent
full detailed OBJ projection reproduces the missing side faces with backface
culling, and restores them when allowing backfaces. Thus this view's missing
faces already exist in the simplified source mesh with reversed winding.

Within object `kitchen`, welded manifold-edge adjacency identifies five
otherwise consistently wound components and twelve minority reversed faces.
Six common edges run in the same direction before the repair; none do after.
Faces 2847/2986 are the large missing wood-textured quad in the screenshot.
The source AstroBaseKitchen material WoodMat_v/mokume02 uses cull mode 2.
The simplified mesh has also changed the original wall coordinates, so this
is a targeted consistency repair of the authored simplified geometry, not a
claim of triangle-for-triangle matching to the Wii mesh.

The shared `asset_construction/stages/observatory.pak` now reverses those twelve
faces in its existing `f.v` and `f.t` delta streams. No new recipe/asset format
or decoder branch is needed. `observatory-winding-fix.json` beside it records
the exact face/line changes and before/after SHA-256s. The native reconstruction
regression `tools/test_observatory_winding.py` reads the real extracted ISO
archives, proves exactly those twelve face lines changed, verifies all other
lines (positions, UVs, materials included) against the original whole-file
hash, and checks edge consistency. It passes. Far-mesh geometry is unchanged.
The independent repaired projection restores 660 previously uncovered pixels
at 160x120 and removes zero covered pixels; `invisible-i/platform-before-after.png`
shows the original/repaired source projections. These are diagnostics, not
runtime screenshots. Native asset rebuilding completed in Docker; runtime J
and WASM rebuilds/verification are underway. The visible VM remains I until
J is validated.

The Docker approval service initially refused the rebuild due to its own usage
limit, without running the command. A later reviewed retry succeeded; this is
no longer a blocker. No hardware accessed or firmware rebuilt.

J runtime verification is complete. `platform-j-headless/screen.png` is an
actual headless GWemu frame rendered through the packaged homebrew at the
exact captured camera. GWProv temporary breakpoints stopped at `game_draw_frame`,
replaced only the pointer-free Camera data, and stopped again at the next
`smg_game_frame` entry after presentation. The camera matched the capture
exactly. This deliberately isolates world rendering; actors/player remain in
the newly started game's state, so it is not a gameplay replay or performance
measurement. The missing platform wall is visible, CFSR/HFSR are zero, and
capacity/drop counters are zero. The diagnostic profile was cleanly stopped
and removed. `runtime-before-after.png` shows the two runtime views.

Normal installation into both disposable and visible profiles passed the ELF
relocation audit with zero differences. The single visible instance is now
J, `gwemu:2837342`, GDB 1235, with depth enabled for this session. Record
`tmp/smg-live-session.json` as the current authority if it later changes.
J has the same 82,989-byte homebrew, 80,780 RAM code/data and 498,724 BSS sizes
as I; the asset XIP remains 31,080,816 bytes. There are no added triangles or
runtime memory allocations. The corrected XIP SHA-256 is
`c74bcee765e8b9b148ca9d90b6f82020d93df90b881c6f59213d88c3d56ec7b8`.
Chromium 153.0.8010.12 and Firefox 155.0 full ISO conversions both pass
byte-for-byte native parity (134.95 s / 119.51 s, 8 MiB input ranges,
538.826 MiB WASM memory). The browser harness completed successfully and
removed its temporary reports and HTTP server. All converter variants and the standard release
bundle were rebuilt; logs are `wasm-j-build.log`, `wasm-j-browser.log` and
`release-j.log`. Temporary reconstruction/install directories are removed.

A follow-up welded-edge audit of the rest of the Observatory source finds six
remaining inconsistent shared edges in object `ufo`; other objects including
the repaired kitchen have none. These are candidates for a later targeted
visual check, not automatic evidence that every such face should be reversed.
See `observatory-winding-candidates.json`.

### Palm atlas UV repair K and ground interpolation audit (2026-10-23)

The user confirmed the J platform improvement, then identified invisible tree
surfaces and requested ground texture stability next. `trees-j/` captures the
current Observatory palm view with depth enabled. Both source OBJ projection
and captured-arena replay show alternating transparent gaps through the trunk.
Its 32x32 `t_SolidPalm.png` atlas combines opaque bark and alpha-cutout leaves;
the authored simplified OBJ uses world-space box UVs over the whole atlas,
incorrectly placing leaf holes on the trunk.

All 48 proxy palm triangles use positions matching the original AstroBaseB
SolidPalmMat_v vertices (within 0.0001 world unit). K restores their original
atlas UVs. Shared box-UV entries sometimes joined different source coordinates,
so the final repair reuses 12 existing UV entries for the 12 distinct original
UV coordinates and updates the 48 faces' UV indices. Geometry vertex indices,
winding, counts, materials and all other OBJ lines stay unchanged. Existing
`f.t`, `vt.res`, `vt.dec`, `vt.nz` recipe streams express this without decoder
or format changes. `observatory-palm-uv-fix.json` records all 60 changed lines
and input/output hashes. `test_observatory_palm_uv.py` decodes the ISO-backed
recipe, proves the change scope by whole-file hashes, validates every corner
against the original model and samples atlas alpha: zero trunk holes, 4770
transparent leaf samples retained. The earlier winding regression passes by
undoing this later UV-only repair before checking its own original baseline.

A first trial used zero-based prediction keys against a one-based UV predictor,
then exposed the shared-UV conflict. The strict original-UV regression caught
both before runtime installation. Its initial compiled assets were discarded
and rebuilt from the corrected recipe; `assets-k-invalid-uv.log` is historical,
not evidence for the final build. The final native asset build passes; SDK
runtime and WASM builds/validation are underway. Current visible VM remains J
until K is checked headlessly. Preserve the `trees-j` camera capture and J ELF.

Ground: the current rasterizer interpolates U/V affinely. It subdivides a face
only when max_z-min_z > 4*(max(min_z,0)+768), allowing severe perspective error.
In this captured floor, entry 6 covers 5008 pixels in the lower screen half and
has a 2.79:1 vertex depth ratio. Its affine centroid differs from U/Z,V/Z
interpolation by 5.08 and -6.55 source texels; entry 4 differs by 8.61/-5.36.
This confirms a cause of camera-dependent growth/morphing independently of
resolution or texture replacement. `audit_arena_projection.py --perspective`
provides a diagnostic using captured reciprocal depths (same affine mip choice
and painter ordering; not a production renderer or quality/performance verdict).
`trees-j/ground-depth-spans.json` and `arena-perspective-projection.png` preserve
the evidence. Next rendering work should prototype bounded-span perspective
correction and compare against exact per-pixel division, then measure overhead;
do not claim STM32 timing from these images or replace textures to hide warping.

K is now installed and validated. The near and wide headless runtime screenshots
are in `tree-k-headless/`; both show a continuous bark trunk with leaf cutouts.
For the final screenshots the saved camera was injected before **three**
consecutive draw frames, allowing the LCD/framebuffer presentation to catch up.
The first single-frame captures showed the prior view and were replaced. Camera
state at a breakpoint alone does not prove that QMP's displayed frame is from
that same draw. Both final captures have zero CFSR/HFSR and no capacity/drop
counter hits. Headless VM was stopped and its scratch profile removed.

K changes only 573 bytes in the Observatory blob within the 31,080,816-byte
asset XIP; all other asset bytes match J. SHA-256:
`78bf5b15ad8272dfe3c759972136bde3c4745558d78303de1b0ed7d148703668`.
RAM/code sizes remain the same as J, with no added faces or runtime allocations.
Native installation and exact ELF relocation audits pass in headless and live
profiles. All WASM converter variants were rebuilt. Full ISO browser conversion
passes native byte parity in Chromium 153.0.8010.12 (125.07 s) and Firefox 155.0
(117.60 s), both 8 MiB ranges and 538.826 MiB WASM memory. The release ZIP is
regenerated. Logs: `assets-k.log`, `build-k.log`, `wasm-k-build.log`,
`wasm-k-browser.log`, `release-k.log`. Shared-UV reconstruction uses the existing
recipe format; no browser-specific correction. Temporary tree directories and
browser reports/server were cleaned. The visible VM is K, `gwemu:2852976`, GDB
1235, with depth enabled; `tmp/smg-live-session.json` records it. Hardware remains
relinquished. The tree fix currently covers the reported Observatory palm;
other tree materials/stages need their own evidence before changing UVs.

Ground prototype preparation: `tools/audit_perspective_spans.py` compares exact
floating-point per-pixel perspective UVs with horizontal endpoint interpolation
on the captured stage triangles. `trees-j/perspective-span-model.json` has the
results. Fixed 8-pixel spans require 7036 endpoint attribute divisions versus
24818 per-pixel attribute divisions (71.6% fewer, **not** a CPU-time claim).
Worst error is 0.200 source texel, p99 0.0269; 0.032% of samples exceed 1/8
texel. Four-pixel spans have worst error 0.0358, p99 0.0104 and no samples over
1/8 texel. This is one captured scene, including overdraw, without alpha/mip
visibility filtering. It excludes setup costs and fixed-point rounding.

Next: implement an optional perspective-mapping runtime prototype with bounded
horizontal spans (shorten to four or fewer pixels where needed), validate its
sampling against exact perspective interpolation and retain existing coverage,
UV wrapping, mip strides and alpha/depth semantics. Reuse carried reciprocal
triangle depths, but generate them when perspective is enabled even if depth
testing is off. Test clipped/subdivided, figure and overlay paths; keep the
32-byte DrawEntry ABI. Compare via the accepted player timeline from disposable
profiles and then eventual hardware cycles. Existing QMP DWT zeroes and browser
converter timing do not establish rendering speed. Production ground mapping
has **not** changed in K; the current improvement is the tree asset fix.

### Floor support failure: tracing L and precision fix M (2026-10-23)

The user reported falling through ordinary plaza/path surfaces. Ground texture
prototype implementation was deferred before making production changes to it.
K's live capture `collision-k/` was in Honeyhive, paused in Retro-Go's menu.

L adds an eight-entry ring of standing-support-loss incidents, retained across
respawns. Each record includes stage/scenario/hidden parts, old/new feet and up,
the exact ground ray, selected hit (if any), and the actual candidate indices.
A volatile sequence is cleared before writing and published after a compiler
barrier; the global volatile sequence commits the slot. It adds 2920 B of BSS
(including sequence/alignment), records only failed standing-support queries,
and does not deliberately change movement/collision decisions. The large-drop
query is recreated only on a failed standing step for diagnostic capture.
`smg_collision_incidents` and `smg_collision_incident_sequence` are available
through GWProv's typed Python console. Flags bit 0 is ground_hit, bit 1 is the
**previous** has_floor_below hint, not an independent current-floor verdict.

`tools/replay_collision_route.py` ran the accepted route from a disposable copy
of its immutable baseline with only the final quit removed in a scratch copy.
The pre-publication-barrier L binary (`l-prepublication.elf`) produced 1210
rendered frames, seven incidents, no life loss or CFSR/HFSR fault. Exact replay
found no static floor in those seven ray segments; they are not proof of the
user's reported bug and may include ordinary ledges/steps. Final L publication
ordering was then built and boot-checked. The final headless check initially
stayed at **frame 2 in Retro-Go's startup pause**, not a CPU fault or launcher.
B resumes it, as the accepted timeline's first input does. After that it
advances with zero faults. Do not silently count a paused startup as gameplay.

`tools/watch_collision_vm.py` then passively observed visible L for 300 seconds
(no halts, resets or input), recording 47 incidents (44 Honeyhive, 3 Observatory).
The initial ring had already overwritten 15 events before the watch began;
subsequent events were preserved. Evidence: `live-collision-l/report.json` and
`.log`. It completed and closed its QMP connection. This captured a concrete
runtime miss at sequence 20, Honeyhive floor triangle 3299, tick 6069:
feet (-4216,-599,-3620), ray from z=-3570 to -3681. Candidate count 90 includes
the valid floor. Independent full-mesh geometry confirms the intersection.

The runtime computed each plane dot-product component with Q14 truncation,
then tested the rounded distances. For this face it obtains d0=111,d1=0 and
rejects the ray, although the unrounded end distance is -8589/16384≈-0.524.
M retains int64 dot products through the signed crossing test and ratio14
(common Q14 scale cancels). It includes endpoint contact but excludes coplanar
segments. The same captured ray now hits 3299. The ray is shared by support,
look-below and camera queries; those benefit from the corrected intersection
semantics too. No blanket collision-candidate increase or catch-all floor was
introduced. Missing geometry is still possible and needs separate evidence.

`tools/audit_collision_rays.py` compiles the production gather/ray code in
Docker against actual compiled stage data and an independent double-precision
triangle oracle. Dynamics are excluded in these fixtures. Before M:
Honeyhive 69 misses/54320 valid floor probes; Observatory 12/50867. All misses
in these ray fixtures had their source triangle in the candidate list. Wider
body queries reached the 128-entry cap in 21 Observatory probes, but none of
those cap cases caused a floor miss here; Honeyhive did not cap. An experimental
wide cross-product-only variant worsened some thin-face results and was not
applied. M gives Honeyhive **0/54320** and Observatory **1/50867** (thin tilted
face 2193); the residual is unresolved. `--contract` verifies endpoint, start
contact, coplanar rejection, no crossing, one-sided and outside-triangle cases.
`--strict-incidents --incidents .../live-collision-l/report.json` asserts that
all 44 captured Honeyhive rays agree with the independent geometric floor
oracle after M (sequence 20 is the repaired hit; the others lack floor within
their recorded ray). This does not prove the entire map is hole-free.

M's accepted-route replay advances 1208 rendered frames with zero faults and
no life loss. It has zero support-loss incidents, but **its player/camera path
diverges from L**, so that count is not a valid A/B improvement claim. Both
have meaningful traversal; `route-l-vs-m-states.json` compares nearest sampled
simulation ticks and finds up to 1070.25 world units position difference.
`route-m/final.png` is the final pause screen. Use these runs for liveness and
divergence evidence, not a hardware performance or equivalent-workload verdict.

M: SMG.bin 82989 B, RAM code/data 80780 B, BSS 501644 B. Assets are byte-identical
to K (SHA-256 `78bf5b15ad8272dfe3c759972136bde3c4745558d78303de1b0ed7d148703668`).
The standard native and WASM-installer release bundles were regenerated around
M's runtime; no converter asset change/rebuild was needed. Hardware remains
relinquished and no firmware was built. M was deployed to visible PID 2881308,
verified advancing with depth enabled and zero CFSR/HFSR faults, and subsequently
replaced by N below. The passive M watch completed without new Honeyhive corner
incidents; its retained Observatory records do not explain this distinct gap.

### N: source geometry missing at the Honeyhive rock corner

The user's `tmp/smg-live/gwemu-2026-10-23-20-56-36.png` identifies a real gap
between the simplified soil/wood cliff meshes, not a transparent texture or a
backface-only error. Both culled and two-sided source-mesh projections reproduce
it. Evidence and the matching M snapshot are in
`build/perf/2026-10-23-audit/rock-hole-m/`. The original Wii model covers the
gap. Restored 18 original `Soil02_v` triangles (nine visible source faces plus
their shared-edge neighbors) from ForestHomePlanet shape 14, object placement
219, to **both near and far** Honeyhive meshes. Existing OBJ bytes remain an
unchanged prefix. Positions, winding and UVs come from the ISO; the material
uses the existing soil texture. No full-detail model replacement or artificial
catch-all floor was added.

`asset_construction/stages/honeyhive-corner-fix.json` records source hashes,
placement, triangle IDs and output hashes. The existing recipe encoding and
converter formats are unchanged. `tools/asset_construction/restore_honeyhive_corner.py`
reproduces a candidate from the archived pre-repair recipe in `rock-hole-m/`;
it refuses a mismatched baseline. `tools/test_honeyhive_corner.py` reconstructs
the production recipe and verifies the unchanged original prefixes and all 18
ISO-derived triangles/UVs in each mesh.

The production collision gather/ray code hits **18/18** normal-oriented probes
through the restored surfaces; the archived old stage hits only **1/18**.
Commands: `tools/audit_collision_rays.py --regather --require-hit --incidents
build/perf/2026-10-23-audit/rock-hole-m/restored-surface-probes.json`, adding
`--stage-file .../rock-hole-m/stage_honeyhive-before.bin` for the negative
control. Preserve that old blob when replaying M triangle-index fixtures:
the current compiled stage now contains N's additional faces.

Native assets and SDK runtime were rebuilt in Docker. N retains M's RAM/code
sizes; asset XIP is 31,083,728 bytes. Normal GWProv installation passes the ELF
exact-relocation audit with zero differing pointer or non-pointer words.
`rock-hole-n/screen.png` verifies the filled corner at the captured M camera
after three presentations in a disposable headless VM. That check injects a
camera and is **not** a gameplay replay or a performance comparison. CFSR/HFSR
are zero. Its stage-introduction overlay differs from the user's gameplay HUD.

Visible N is `gwemu:2908991`, `tmp/smg-live`, GDB 1235, with depth enabled and
advancing frames, zero CFSR/HFSR (`n-live-health.json`). Both the old visible VM
and the disposable headless VM were stopped. `tmp/smg-live-session.json` tracks
the single visible VM to keep updated. The WASM converter was rebuilt for N
and the release bundle regenerated. Headless browser conversion passes native
byte parity in Chromium (113.20 s) and Firefox (115.04 s), using 8 MiB ranges
and 538.92 MiB extractor memory. Both produce SHA-256
`d2f388f05ca079a6cae7d9669e45f59ca7c7753e9a070646aa4d9ed551414296`.
Reports are under `wasm-n-browser/`. This repairs the reported corner,
not every missing face in the simplified levels. Ground perspective correction
and broader performance work remain pending. Hardware remains relinquished.

User subsequently requested no more visible instances: stop N's visible
preview and disable automatic visible launches/refreshes. Future tests are
headless unless explicitly requested otherwise. Preserve `tmp/smg-live` as
an installed profile; `tmp/smg-live-session.json` records its stopped status.

### O: ordinary stage triangle dispatch

Added a GNW stage fast path in `src/graphics/engine.c` for faces that need no
clipping, depth subdivision, animated UVs, water material remapping, under-water
bin routing or packed far-texture lookup. It reuses transformed/projected
vertices and directly submits the same bin, winding, material, UVs and depth.
Cached textures, alpha cutouts and flat-distance/small-face rules are retained.
Complex cases keep the existing general emitter. The fast/slow counters count
handled input faces, including rejected faces, not only submitted triangles.
No geometry limits, texture quality, detail setting or memory pools changed.

`tools/test_stage_paths.py` Docker/ASan fixture compares production fast dispatch
with the unchanged production general emitter over 40,000 jobs (160,000 input
faces). It checks complete arena entries, linked-bin heads/order, reciprocal
depths, subdivisions, attempts and drops. Includes near/far cut boundaries,
home/delta slot remapping, negative/wrapped UVs, nonzero animation scroll,
water modes 1/2/3, cache hits/misses, packed far textures, clipping, split faces,
alpha/flat modes and full-arena saturation. Result: 89,671 fast / 70,329 slow,
17,547 emitted, 261 saturation drops; exact equivalence. This is a dispatch
regression, not an independent assertion that all original geometry is correct.

Initial Docker SDK build O passes: RAM code/data 81,544 B (+764), BSS 501,640 B
(-4 due to layout/alignment), no new static cache or buffers. Matching initial
ELF archived as `o-initial.elf`. Runtime comparisons used disposable copies of
`build/perf/player-route-20261023-165706/baseline-profile`, using accepted
route-02 with only its final quit removed in a scratch timeline. N and O are
installed normally with identical N asset bytes. Hardware remains relinquished;
GWemu timings will not establish hardware speedup or sustained 60 FPS.

Both accepted-route runs advance 1208 rendered frames without CFSR/HFSR faults.
O handles 436,737 input faces fast and 63,433 slow (87.3% fast). N and O diverge
in player/camera state at all 50 common sampled simulation ticks; final images
also differ. N has zero recorded support-loss incidents, O 21, but these are
different traversals and cannot establish a physics regression or improvement.
The route comparison excludes startup/final pause using play-scene samples
between elapsed seconds 9 and 47. Its 3.409 vs 4.340 guest game ms/frame are
**not comparable speed measurements**. Evidence: `stage-fast-n-vs-o.json`,
`route-n/`, `route-o/`. Do not claim a speedup from these recordings.

Final O adds volatile `g_stage_fast_enabled` (default true) for GWProv-console
A/B in one build/scene. The switch is sampled once per stage emission call,
not once per face. Its forced-general path is covered by the same fixture:
84,391 fast / 75,609 slow, same 17,547 emitted and 261 drops. Final RAM code/data
81,620 B (+840 over N), BSS 501,644 B (unchanged). `o.elf`/`o.bin` now match
this final build; initial route O's metadata points to `o-initial.elf`.
Normal installer exact relocation audit passes with zero differing words.
`tools/benchmark_stage_vm.py` alternates switch order in stationary windows,
checks camera/player/scene/detail/depth stability, records geometry deltas and
restores the switch on exit. Stationary Observatory/Deep Dark diagnostics are
complete. They are not substitutes for traversal or hardware measurements.
Observatory handles 83.1% of faces fast, with identical 384 arena attempts/frame
and no drops. One camera-changing window is invalid; remaining 3-second guest
timings vary in both directions (fast 4.000/4.356, general 4.722/4.100 ms/frame
in the two stationary paired rounds), so no reliable speedup is established.
Deep Dark uses **zero** fast faces because O excludes all water-material jobs.
Its moving intro invalidates most early windows. The later stationary pair
is 4.744 fast-enabled versus 4.389 forced-general ms/frame, but both actually
use the general face emitter. This is baseline/noise evidence, not a gain.
All tests have zero faults and no arena drops. Summary:
`stage-fast-o-ab-summary.json`. Stage cycling initially hit the same entry
breakpoint repeatedly without executing a frame; the corrected helper advances
to `game_draw_frame` and then to the next entry between Select inputs.

### P: water material selection in the stage fast path

Deep Dark's zero-fast result motivated sharing `emit_through_water` and its
texture remapping with the fast path, exactly as in the general emitter.
This permits ordinary water-material faces without duplicating any water
classification rules. Special water-bin routing, animated UVs, clipping,
subdivision and packed far lookups still fall back. Docker ASan equivalence
passes all 40,000 jobs including three water modes and cached remapped textures.
Final fixture also exercises negative home/delta slot offsets: 92,124 fast /
67,876 slow, 17,465 emitted, 255 capacity drops, exact entries/bin/depth output.
Docker SDK build passes: RAM code/data 81,672 B (+892 over N), BSS 501,640 B
(-4 alignment). No new static geometry buffers. Matching `p.elf`/`p.bin` archived;
normal installer relocation audit has zero differing code/assets words.
The normal WASM-installer release was regenerated with P's runtime and N's
unchanged converter/assets. No converter rebuild or repeated browser asset test
was needed for these runtime-only changes.

Headless Deep Dark P handles 34,470 fast / 25,200 slow faces per 90-frame window
(57.77% fast, versus O's zero). Five windows have 311 arena attempts/frame;
one interval has 307.58 because its sampled frame and geometry counters straddle
a frame boundary. No arena drops or CFSR/HFSR faults. All fast-enabled windows
fail the strict camera-stability check, even after a 25-second intro wait,
so no stationary timing pair or speedup is claimed. Raw guest game times range
3.933–4.256 fast-enabled versus 4.200–5.689 forced-general ms/frame, with timing
noise and differing camera states. They are not STM32 cycle timings.
Evidence: `stage-ab-p-deepdark.json`, `.png`, `stage-fast-p-ab-summary.json`.

All test VMs have been stopped; disposable profiles, native bundle staging and
scratch timelines under `tmp/stage-fast` have been removed (~702 MiB). Accepted baseline
and original recording remain untouched. Archived route metadata points to its
preserved replay timeline. No visible VM was started; the user's stopped live
profile still contains N. Final release artifacts contain P. Hardware remains
relinquished. No sustained-60-FPS result yet.

Next performance steps: packed far textures still force the general emitter;
reuse its raw-texture/average/alpha selection in the fast path if equivalence
tests pass. Bounded stage-vertex reuse, projection division and Deep Dark's
switch-index cache remain open, along with hot code/data placement and raster
setup. The ground-perspective rendering defect remains confirmed and unfixed.

### Q: packed far materials through ordinary stage emission

The fast path now uses the general emitter's packed-far raw-texture lookup and
average/alpha rules instead of falling back solely because a stage has packed
far textures. Opaque averaged faces retain the existing flat fallback with a
null texture; missing raw alpha or non-averaged textures retain the existing
rejection. No geometry/detail/texture format changes. An independent eight-case
raw/average/alpha material contract was added to `tools/test_stage_paths.py`.
Docker ASan passes that contract plus all 40,000 equivalence jobs: 100,513 fast /
59,487 slow, unchanged 17,465 emitted and 255 deliberate saturation drops.

Docker SDK Q passes: RAM code/data 81,640 B (-32 from P), BSS 501,640 B (same).
Matching `q.elf`/`q.bin` preserved. Normal GWProv installation has zero differing
relocated code/assets words. Headless Deep Dark handles **99.70%** of input faces
fast (59,490 fast / 180 slow per 90-frame window), versus P's 57.77%. All four
windows pass within-window camera/player/scene stability, with identical 312
arena attempts/frame and zero drops/faults. Guest game times are inconclusive:
round 0 general 4.311 vs fast 4.867 ms/frame; round 1 general 5.033 vs fast 4.856.
They vary in both directions and are not hardware timing. Evidence:
`stage-ab-q-deepdark.json`, `.png`, `stage-fast-q-ab-summary.json`.

### R: reuse switch-object candidate indices for Deep Dark watchers

Factored the existing 14-entry immutable-stage switch-object cache preparation
out of `switch_objects_tick` and reused it in `group_switch_objects_tick`.
Deep Dark has 356 source objects, four group watchers and four other switch
objects: steady-state watcher updates visit **eight cached rows**, filtering
four watchers, instead of all 356. Source order is unchanged. Scenario bits,
one-shot/GONE state and switch dependencies are still evaluated each tick.
Lists exceeding 14 candidates fall back to the complete scan. No extra cache
storage and no assumption that other stages have exactly four watchers.

`tools/test_switch_object_cache.py` compiles production switch/group rules in
Docker with ASan and compares cached updates against full source-order scans.
24,242 exact comparisons cover all switch values/order/generation, object state,
chip hints, scenario changes, persistent one-shots, an explicit dependent
watcher chain, stage changes and 1,500 overflow-fallback stages. All ten actual
stages are tested using object arrays resolved from the linked SDK ELF: raw
stage .bin files may have pooled/external object placeholders and are not safe
to interpret directly. Temporary header/object-only fixtures preserve the
linked object bytes and provide their local relative offset.

Docker SDK R passes; RAM code/data 81,640 B and BSS 501,640 B unchanged from Q.
The watcher loop is XIP code; no hot-memory placement was changed. `r.elf` and
`r.bin` preserved, native and WASM-installer release bundles regenerated.
Normal installation relocation audit passes. Headless cache transition checks
pass; Honeyhive initializes lazily after its intro allows actor/switch
processing, so a one-second post-load check was premature. Wait for its first
switch update rather than treating that as a failed invalidation.

Runtime `Deep Dark → Honeyhive → Deep Dark` checks confirm source pointers,
ascending candidate indices and types rebuild correctly: eight/four/eight
candidates, with Deep Dark watcher indices 42, 93, 95, 281. Frames advance and
CFSR/HFSR remain zero at every check. Evidence: `switch-cache-r-runtime.json`
and `.log`. Test helpers and matching ELF are archived. No visible instances
were launched and hardware was untouched. All test VMs have been stopped.

`r-linker-memory.json` measures 158,096 bytes (154.39 KiB) of RAM_EMU remaining
after linked code/data/BSS. RAM_EMU is 741,376 bytes (724 KiB), following the
firmware's 307,200-byte framebuffer reservation. This is **linker headroom**,
not a runtime allocator measurement. SDK `*_get_free_size()` queries are
available for RAM_EMU/ITCM/DTCM/AHB and should be recorded after startup before
placing caches or code in those pools. No firmware reservation was changed.
The SDK documents that these queries report the largest currently allocatable
span, not necessarily total free bytes; allocations through mem_ctl zero data.

Next: bounded stage-source-vertex reuse and projection-division measurements,
then remaining raster setup and memory placement. Ground perspective correction
is still pending. Hardware is still relinquished, so sustained 60 FPS remains
unproven. Current release artifacts contain R; stopped user live profile is N.

Completed Q/R scratch (`tmp/stage-q`) has been removed; archived evidence,
accepted baseline/recording, release artifacts and stopped live profile remain.

### S/T: measured pools and bounded stage-source vertex reuse

S adds `smg_memory_stats` and `smg_memory_request` to the debug console. The
homebrew publishes SDK FREE_SIZE queries after startup; changing the request
word refreshes them outside frame-work timing. The sequence counter protects
publication. No allocator INIT or allocation is performed by these queries.
`tools/capture_memory_vm.py` uses GWProv Python bindings to request and archive
one consistent sample with the matching ELF and linker headroom. Results are
for the installed VM firmware/profile, not measurements of the relinquished
hardware device.

S after startup: RAM_EMU 157,776 bytes, ITCM 65,536, DTCM 110,592, AHB largest
span 86,580. Runtime RAM_EMU equals linker headroom for this fixed-buffer core.
`memory-s.json` preserves startup and refreshed samples, zero faults. S SDK
code/data 81,960 bytes, BSS 501,640. `s-memory.elf`/`.bin` archived.

T allocates a **16 KiB, 2,048-entry source-coordinate cache** through SDK
`dtc_malloc` after checking at least another 16 KiB remains available. It uses
no allocator reset, direct firmware address reservation or DMA buffer. If the
allocation fails, rendering retains the uncached path. The game API also rejects
undersized or misaligned supplied buffers. Current measured cache address is
0x20000000; DTCM largest remaining span is 94,208 bytes (92 KiB). RAM_EMU remains
157,272 bytes, ITCM 65,536, AHB 86,580 (`memory-t-cache.json`). Thus the cache
uses the separate DTCM pool instead of consuming 16 KiB of the AXI working area.

Each eight-byte entry holds a uint16 source-point tag and the exact three int16
coordinates. Main-stage immutable points only: far/figure/mutable scratch
transforms bypass it without evicting the main stage. The direct-mapped cache
persists across frames and invalidates on stage-source changes or sleep/wake.
It caches **source coordinates**, never camera transforms or frame-pool slots;
all matrix, clip and projection calculations still run normally. Index 65535
bypasses the reserved empty tag safely. Source point counts range from 1,281 to
29,773; copying every Deep Dark source point would require 178,638 bytes and
exceed current AXI headroom. No asset, geometry limit or draw quality changed.

`smg_vertex_cache_stats` separately records hits/misses/bypasses/invalidations;
`g_stage_vertex_cache_enabled` is a volatile debug A/B switch. Existing geometry
ABI stays at version 2. `tools/benchmark_stage_vm.py --control vertex` records
cache and geometry deltas alongside camera/player stability, and restores the
switch on exit. `capture_render_vm.py` includes the cache descriptors/counters.

Docker ASan `tools/test_vertex_cache.py` verifies cached/uncached transformed
positions, clip bits, projected screens and untouched slots across 12,004 jobs.
Includes source changes, hash collisions, both destination delta signs, changing
matrices, mutable-source bypass, invalid supplied buffers and empty-tag index.
A warm source is mprotect(PROT_NONE) during a hit-only transform: it succeeds,
proving source loads are avoided. The existing 4,000 transform/visibility
regressions pass with caching unavailable, and independent asset/GBA render
contracts still pass. None of these is a hardware speed benchmark.

SDK T: code/data 82,456 bytes, BSS 501,648, plus the 16 KiB runtime DTCM
allocation. `t-cache.elf`/`.bin` and normal relocation audit preserved; zero
code/assets relocation differences. WASM-installer release regenerated with
T runtime; converter and asset bytes remain N. Native test bundle uses the
normal installer and exact relocation declarations.

Headless view probes: Observatory cache-enabled windows have 45,090 hits and
3,060 misses (93.64% hit rate); Deep Dark moving views vary from 79.43% to
95.19%. Far/non-stage work is counted separately as bypasses. Invalidations
stay zero within each view window; cycling to Deep Dark renders intermediate
stages, so cumulative invalidations are not simply two. Cache address/source
eligibility and zero CFSR/HFSR verified in `vertex-t-runtime.json`.

Guest timings do not prove a speedup: Observatory's stationary enabled/general
pair is 4.489/3.648 ms/frame (enabled slower); other windows have camera drift.
Deep Dark windows also move. GWemu does not model STM32 flash/DTCM costs well
enough to judge whether avoiding source loads pays for lookup overhead. This is
an instrumented, correctness-validated cache candidate pending hardware A/B.
The accepted route replay from a fresh disposable baseline copy completes
1,209 advancing rendered frames, retaining four lives and zero CPU faults.
Play-scene samples between elapsed seconds 9 and 47 exclude startup/final pause:
384,132 hits / 25,737 misses (93.72%) and zero source invalidations. All geometry
capacity-hit/drop counters are zero; arena peak 411, visible peak 690. The 12
support-loss incidents all agree with the independent static collision oracle:
no floor intersects their recorded ray segment. They do not establish a new
collision miss or prove the entire map hole-free (`route-t-collision-audit.log`).

All 118 common sampled simulation ticks differ in camera/player state from
archived N; do not compare their timing as equivalent work. Final screenshots
are preserved alongside state samples; framebuffer hashes alone are insufficient
because pause overlays also differ. Evidence: `route-t/`,
`vertex-t-route-summary.json`, `vertex-t-ab-summary.json`.

Test VMs are stopped. Accepted baseline and original timeline remain untouched;
the replay's quit-free timeline, reports, matching ELFs and scene helper are
archived. Completed scratch was removed (~547 MiB). No visible launches or hardware
access. Current release contains T; the stopped user live profile remains N.
Projection-division benchmarking, remaining raster setup/hot-code placement
and ground perspective correction are still pending. Hardware A/B is required
to decide whether the cache is faster on STM32 and to establish sustained 60 FPS.

### U: exact projection division instead of the XIP table fetch

Verified the currently linked T asset table, not just build D's assumption:
all **65,504 reachable entries** (indices 32..65535) equal floor(1048576/index).
The 262,144-byte table is in `.xip_smg_assets`, SHA-256
`f25e4e92296a7b4b074bc5991cf8e1577c097299c6870bc911ae850e8a057a93`.
U's GNW `project_to_screen` uses unsigned division by that clamped index.
The minimum z=128, right shift by two, upper clamp, signed product shift by 14,
subsequent screen scaling, rounding and packing are unchanged. GBA code retains
the table path. No projection precision or texture-perspective change is claimed.

`g_projection_divide_enabled` defaults true and permits same-binary GWProv A/B.
The table and its reciprocal-area alias remain in assets for reference; **no
256 KiB storage saving** is claimed. `tools/test_projection_divide.py` checks
363,232 projections in each mode at both 160x120 and 320x240 against an independent
wide-integer rounding/packing oracle. Includes all denominators and their four
z values, clamp boundaries/negative z, signed XY and far z. A protected-table
(PROT_NONE) call succeeds in divider mode, proving no table fetch is required.
Figure 40k, stage 40k and cached-transform 12,004 integration fixtures also pass.
The transform fixture prefix now enables the actual GNW projection branch.

Docker SDK U: RAM code/data 81,744 B (-712 from T), BSS 501,648 (same).
`u-projection.elf`/`.bin` and target disassembly archived. Projection remains
inlined in the transform routine and uses M7 **UDIV**, without a projection
function call. Normal installer exact-relocation audit passes, zero differing
words. Headless Deep Dark's four windows are stationary and fault-free, but
guest timings vary both directions: table/divider 4.756/5.022 ms per frame,
then divider/table 4.267/4.378. This is not a reliable speedup or hardware result.
`projection-u-summary.json`, `projection-u-deepdark.json` and `.png` preserve
results. The disposable U VM was stopped before the next runtime test.

### V: exact shared reciprocal for raster setup

Up to six wide integer divisions per triangle (four UV, two depth gradients)
now share a lazy Q63 reciprocal of the absolute signed area. Zero numerators
return zero without creating a reciprocal; flat/depth-disabled cases likewise
avoid it. With R=floor(2^63/d), floor(n*R/2^63) is at most one below n/d for
0<=n<2^63. A remainder check corrects that one-step underestimate, then restores
the signed quotient. Signed-16 screen/UV inputs and scaling bound renderer
numerators below 2^63. No floating point, approximate UVs or rounding change.
The 128-bit multiply/shift uses four portable 32x32 products; target disassembly
confirms UMULL-based arithmetic and unsigned wide division for reciprocal setup.

`g_raster_setup_recip_enabled` defaults true; false retains signed division for
A/B. The switch is sampled once per triangle. `benchmark_stage_vm.py` supports
`--control projection` and `--control raster`; captures also record both flags.
No asset/layout/draw-entry, coverage, ordering, alpha, mip or geometry limit
change. Ground affine warping remains unfixed.

Docker ASan/UBSan `tools/test_raster_setup_div.py`: **1,400,512** exact signed
ratios agree with native division, including **599,640** reciprocal correction
cases, integer boundaries, signed denominators and a separate __int128 product
oracle. Pixel regression: 24k triangles match the pre-optimization scalar
reference byte-for-byte at each render resolution. Depth regression compares
600 four-triangle groups (all materials, winding, mipmaps, zero gradients)
in both modes, including entire pixels and depth buffers. Captured overlap,
transparent cutout and deferred shadow assertions pass. Independent GBA render
contracts also pass. Host fixture timings are not STM32 results.

SDK V passes: RAM code/data 84,328 B, BSS 501,648 (same), existing 16 KiB DTCM
cache unchanged. Matching `v-raster.elf`/`.bin` and ARM disassembly preserved.
The normal WASM-installer bundle was regenerated around V; N converter and asset
bytes are unchanged. Native bundle/normal relocation audit passes with zero
code/assets differences. No Retro-Go firmware builds/changes, hardware access
or visible VM launches.

Headless V Deep Dark with depth enabled: round 0 is stationary in both modes
with matching initial camera/player/detail context, measuring original/shared
setup **5.567/5.067 guest ms/frame**. Round 1's enabled window has camera drift
and is invalid. This is one matched diagnostic pair, not a repeatable hardware
speedup. `raster-v-summary.json` records both within-window and paired-context
checks. Neither view is a substitute for the accepted traversal. Faults and
geometry-buffer losses are zero. The A/B control is restored to true afterward;
`raster-v-runtime.json` verifies projection/divider/cache controls are enabled.

Runtime memory: RAM_EMU largest span 155,400 bytes (151.76 KiB), ITCM 65,536,
DTCM 94,208, AHB 86,580 (`memory-v.json`). No further allocation or pool reset
was introduced by these arithmetic changes.

Fresh accepted-baseline route V completes 1,209 rendered frames with four lives,
zero CPU faults, and zero geometry capacity hits/drops (arena peak 406, visible
peak 695). Active play samples between seconds 9 and 47 have cache 403,559 hits /
31,051 misses (92.86%), no source invalidations. All seven common sampled ticks
diverge from T in camera/player state: do not compare their frame times as an
equivalent workload. Nine support-loss rays agree with the independent static
collision oracle: no floor intersects those segments. This does not prove the
whole map hole-free. Evidence: `route-v/`, `arithmetic-v-route-summary.json`,
`route-v-collision-audit.log`.

All test VMs are stopped; matching ELFs/binaries, native fixtures, disassembly,
images, reports and the quit-free replay timeline are preserved. Completed
scratch was removed (572750765 bytes). Current release is V; stopped live profile is
still N. Goal remains active: hot-code placement and ground perspective
correction remain, and actual hardware measurements must establish sustained
60 FPS. Hardware remains relinquished until explicitly granted.

### W: rasterizer in the SDK ITCM region

The default port build now places only `engine_raster_inner` in the SDK's
64 KiB ITCM core region at address zero. This uses the existing GWHB v1 second
load segment and firmware reservation mechanism; no Retro-Go firmware or dist
spec change. `make -C retro-go-sd docker SMG_ITCM=0` selects the AXI reference;
`SMG_ITCM=1` (default) selects ITCM. Placement is a generated linker include,
not a compiler flag, so both builds use the same compiled rasterizer input.
The include writes only when its contents change; an unchanged rebuild does
not compile or link again. Docker passes project-local `tmp/` as TMPDIR for
SDK segment extraction, whose temporary files are removed by the packer.

Loaded-pointer patching now accepts aligned, file-backed AXI and optional ITCM
sites, including legitimate address zero. BSS and out-of-range sites are
rejected. Known fixed-address veneers remain unchanged; only pointed values
inside the code/asset sentinel ranges are rebased. The final-link exact-site
check exposed linker veneer reordering after introducing ITCM: the generated
loaded-code site set now includes every recognized veneer literal, including
fixed-target ones, so movement of a fixed-target veneer cannot leave a hole.
Recognition still requires the linker symbol/type/size and exact instruction
pattern; opaque numeric words are not scanned. ASan/UBSan bounds fixtures pass
with ITCM enabled and disabled, including fixed pointers and Thumb relocation.

Matching archived `w-axi.elf` and `w-itcm.elf` show respectively 84,840 and
77,956 bytes RAM code/data. ITCM occupies **6,920 bytes**, including two fixed
RAM-call veneers; RAM BSS is 501,652 bytes in the ITCM build. Runtime largest
allocatable spans: RAM_EMU **161,768**, ITCM **58,616**, DTCM **94,208**, AHB
**86,580** bytes. This frees 6,880 bytes of AXI compared with the matching
fallback build; ITCM usage does not consume the DTCM vertex cache reservation.

Normal GWProv installation passes the sidecar relocation audit with zero
word differences. A read-only full ITCM comparison passes in Observatory and
Deep Dark: all 6,920 bytes match the expected relocated ELF, both asset literal
pointers are updated and fixed veneers preserved. A temporary GDB breakpoint
hits `engine_raster_inner` at PC=0, confirming execution from ITCM. Advancing
frames and zero CFSR/HFSR accompany both readbacks. Evidence:
`itcm-w-readback.json`, `itcm-w-deepdark-readback.json`,
`itcm-w-execution.json`, `memory-w-itcm.json` and `itcm-w-layouts.json` under
`build/perf/2026-10-23-audit/`.

Fresh-baseline accepted route W advances 1,209 frames, retaining four lives,
zero faults, no observed support-loss incidents, and zero geometry capacity
hits/drops. Arena peak 413, visible peak 687; active-play source cache hit rate
93.54%. All 21 common sampled simulation ticks differ from V in camera/player
state, so this route is not an equivalent-work timing comparison. Zero observed
incidents do not establish a hole-free map. Reports, final screenshot, quit-free
timeline and summary are retained in `route-w/` and `itcm-w-route-summary.json`.

Current WASM-installer release contains W runtime (SMG.bin 87,085 bytes) and
unchanged N converter/assets. Both disposable test VMs are stopped and
529,287,486 bytes of scratch removed; the stopped user profile remains N.
No visible instance or firmware build was launched. Hardware was not used for
profiling/provisioning; a broad GWProv device-list check also queried its status
and should be avoided while it is relinquished. Placement correctness is
verified; hardware speedup and sustained 60 FPS remain unproven. Ground affine
texture warping remains open. Goal stays active.

### X prototype: bounded perspective texture spans (default off)

Added GNW-only perspective sampling for RM_TEX/RM_ALPHA, controlled by volatile
`g_perspective_enabled` (false by default). At frame begin, its active snapshot
collects reciprocal depths into the existing triangle-depth array even when
Depth buffer is off. No new heap or per-triangle allocation. Flat, additive,
darken and tint sampling retains its previous behavior; coverage, triangle
ordering, material/mip selection and depth-test rules are unchanged.

The rasterizer interpolates unnormalized q, U*q and V*q barycentric numerators.
Each horizontal span starts at no more than eight covered pixels; endpoints
use rational division. The chord-error bound
`abs(U1-U0)*abs(q1-q0)/(4*min(q0,q1))` (likewise V) determines whether to shorten
the span, requiring at most one model UV unit = **1/16 source texel**. One/two
sample spans use exact endpoints. Q16 integer stepping inside each span adds
less than 0.001 UV unit quantization allowance. Double calculations are per
span, not per pixel. Target disassembly confirms VDIV.F64; double-to-int64
endpoint/step conversions currently call `__aeabi_d2lz`. Existing affine UV
setup is still performed, and this is a quality prototype, not a cost-optimized
implementation. Projection's original vertex rounding remains unchanged.

Docker ASan/UBSan `tools/test_perspective_raster.py` checks **16,074,460**
span samples against independent long-double rational interpolation (worst
0.998013 model UV units) at each resolution. Actual production raster output
also passes exact barycentric coverage and bounded texel selection for 452,280
covered samples at 160x120 and 1,720,518 at 320x240, including signed/wrapped
UVs, varying/constant q and both mip levels. The oracle permits the defined
bounded texel disagreement near boundaries; it does not claim pixel identity
with per-pixel perspective. Affine 24k-triangle regressions and independent GBA
contracts pass. Depth/cutout/deferred-shadow fixtures pass with perspective
collection enabled and disabled; 600 four-triangle groups match setup modes.
The repeated depth fixture now resets persistent test material/texel state.

SDK X passes final-link exact tables and two-segment packing: RAM code/data
79,032 B, BSS 501,648 B, ITCM 6,920 B. Archived matching `x-perspective.elf`,
`.bin`, and ARM disassembly. Normal native-bundle installation boots headlessly;
full ITCM readback equals relocated ELF, frames advance, faults zero. Normal
WASM-installer release remains W; X is not published as a new release.

Headless Observatory on/off/on/off windows are internally stationary and both
pairs match initial camera/player/scene/detail/depth context. Guest game cost
is **5.056/8.067 then 9.078/5.456 ms per frame**, ordered as tested. These are
VM diagnostics, not hardware cycles or sustained 60 FPS evidence. The initial
quality correction has substantial cost and must remain default off while
optimizing. `benchmark_stage_vm.py --control perspective` supports the switch;
all controls now restore their initial value in finally rather than forcing on.

Captured affine/corrected views have identical camera/player state and show
plaza paving lines continuing across triangles more consistently. They differ
in 61,796 display pixels; this measures change, not proof of correctness. Both
Depth buffer off and on run without faults; all 384 corrected-view triangles
have reciprocal-depth triples. Capture flags/errors and timing pair checks are
in `perspective-x-summary.json`, images/memory in `perspective-x-affine/`,
`perspective-x-corrected/` and `perspective-x-corrected-depth/`. Native rational
fixtures provide the interpolation evidence, rather than screenshot difference.

The own headless VM is stopped and 324,516,230 bytes of scratch removed. No
visible launch, hardware access or firmware build. Accepted traversal has not
yet been replayed with X correction; neither map-wide correctness nor hardware
speedup is established. Next reduce redundant setup/endpoint conversion cost,
measure span-size distribution on captured real scenes, then test the accepted
route and expensive Deep Dark opening. Ground warping is now prototyped, not
resolved for the default build. Goal remains active.

### Y: reduce perspective setup and conversion cost

Perspective mode now skips the four affine UV-gradient divisions, retains the
same derivative-based mip decision, and computes rational attribute gradients
once per triangle. Rows advance those gradients rather than rebuilding them.
Endpoints share one q reciprocal for U/V; fixed span-length reciprocals remove
two step divisions. Q16 conversion splits whole/fractional UV units into native
signed-32 conversions with unsigned wrapping. This preserves negative UVs and
two-pixel steps exceeding signed-32 Q16 range without a double-to-int64 call.
Target ITCM disassembly has **zero `__aeabi_d2lz` references**, two VDIV.F64
instruction sites and native VCVT.S32.F64. Setup still has int64-to-double helper
calls and per-triangle FPU save/restore; further cost reduction is possible.
The analytic 1/16-source-texel error target and default-off control are unchanged.

Extended Docker ASan/UBSan/float-cast-overflow fixtures: 200k Q16 conversions
agree with the original wide conversion, and **16,102,557** rational span samples
per resolution stay within 0.998744 model UV units. Production pixel fixtures
check **861,388 / 2,824,664** covered samples at 160x120 / 320x240, now including
variable-depth alpha holes and depth writes. Independent rational bounds and
exact coverage pass; alpha-zero samples leave depth untouched. Affine 24k x2,
original GBA contracts, depth/shadow fixtures, stage40k and figure40k pass.
Stage tests explicitly verify near clamping and depth-only/perspective-only/
both/neither collection, then exercise perspective collection independently of
depth in fast/general comparisons.

New `tools/audit_perspective_spans_native.py` runs the actual production helper
on independently selected integer-barycentric coverage rows from saved scenes.
It validates each sample against long-double rational UVs and counts spans,
including overdraw. Observatory: 27,386 samples, mean span **4.94 px**, 11,636
q divisions, max error0.993590 UV units. Deep Dark snapshot: 12,583 samples,
mean4.99, 4,487 divisions, max0.971290. Honeyhive trees capture: 12,409 samples,
mean6.88, 3,524 divisions, max0.907327. These are work counts, not hardware timing
or an expensive-opening profile. Histograms in `perspective-y-scene-spans.json`.

SDK Y passes exact tables/pack: RAM code/data **78,928 B**, BSS501,648,
ITCM **6,504 B** (-416 vs X). Remaining SDK largest spans RAM_EMU160,800,
ITCM59,032, DTCM94,208, AHB86,580; no new heap allocation. Normal native bundle
boots and full ITCM readback matches the relocated Y ELF, with advancing frames
and zero faults. Matching `y-perspective.elf`, `.bin`, ARM disassembly and
`y-engine.c` archived. Standard WASM-installer release remains W.

Observatory's first Y pair drifts and is invalid. Second on/off pair is
stationary with matching context: **6.678/4.656 guest ms/frame**, about2.02 ms
extra for correction. Its scene context also matches X's stationary pair,
whose extra cost was3.62 ms; this suggests reduced VM overhead but is not a
repeatable hardware speedup. All four Deep Dark windows drift, invalidating
their timing comparisons; raw data is retained. No CPU faults or geometry
capacity losses. Affine/corrected/depth screenshots and state are preserved.

`replay_collision_route.py --perspective on|off` can set the debug flag once
gameplay initializes and records the applied value. Y's fresh accepted-baseline
replay, correction enabled throughout active play, advances **1,208 frames**
with four lives, zero faults and zero geometry drops/capacity hits. Arena peak408,
visible peak684; active9..47s cache94.12%. All23 common sampled ticks diverge
from W in camera/player state, so no equivalent-work FPS claim. Six support-loss
segments all match the independent static floor oracle with no intersecting
floor; this does not prove the map hole-free. Evidence in `route-y/`,
`perspective-y-route-summary.json` and `route-y-collision-audit.log`.

Both own headless VMs stopped,529,277,529 scratch bytes removed. Replay timeline,
reports, captures, matching binaries/source and native evidence retained. No
visible launch, hardware access or firmware change/build. Perspective remains
default off pending lower cost and hardware testing. Ground texture correction
and sustained hardware60FPS remain incomplete; goal stays active.

### Z: separate affine and perspective machine-code paths

Y saved floating-point registers and reserved its full mixed renderer stack even
with perspective disabled. The rasterizer body now has two constant inline
specializations behind the existing dispatcher: `engine_raster_inner_affine`
and `engine_raster_inner_perspective`. Both use the same source algorithm;
material/depth/coverage/mip/UV rules and the default-off control are unchanged.
The SDK linker selector includes `.text.engine_raster_inner*`, keeping the
specializations and dispatcher inside its reserved ITCM region. No firmware or
asset format change.

Target disassembly proves **zero floating-point instructions** in the affine
function and dispatcher. Affine stack use is **256 bytes** including saved
registers, versus Y's488; perspective uses360 versus488. The dispatcher tail
branches to each specialization. This removes unnecessary affine FPU work; it
does not establish a hardware FPS gain. Code duplication costs4,376 ITCM bytes:
Z occupies10,880B, leaving **54,656B**. RAM code/data78,936/BSS501,648; remaining
largest spans RAM_EMU160,792/DTCM94,208/AHB86,580. No new heap allocation.

Docker independent perspective/cutout/depth oracle and GBA contracts pass,
with the same bounds/sample counts as Y. Affine24k x2 and depth/shadow fixtures
pass. New `tools/test_raster_specializations.py` compares complete pixels and
depth buffers against frozen validated Y for1,500 four-triangle groups at each
resolution: both perspective/depth/setup switches, all modes, winding, UV wraps,
mip choices, absent depths/textures, degenerates and preexisting occlusion.
All byte-identical. The Y reference checks specialization equivalence only;
it does not replace independent rational or original-GBA correctness fixtures.

Normal bundle installation/sidecar audit gives zero pointer or numeric-word
differences. Full10,880B ITCM readback matches the relocated ELF; frames advance
with zero faults. Temporary GDB breakpoints hit both specialized functions inside
ITCM under their respective flag values (perspective PC0, affine0xff8). Captures
with affine/perspective/depth modes pass without errors. Matching ELF/bin/source,
disassembly, instruction/prologue report, readback, execution and pool snapshots
are archived under `z-specialized*`, `raster-z-*`, `itcm-z-readback*`, `memory-z*`.

Stationary Observatory Z's first pair drifts and is invalid. Second on/off pair
matches context and measures6.300/4.300 guest ms/frame; Y's matching scene gave
6.678/4.656. These small cross-run differences are noisy, not repeatable hardware
speedup evidence. Correction's extra diagnostic cost remains about2ms. No CPU
faults or geometry capacity losses. Y's accepted-route replay remains the latest
representative traversal; Z was validated through exact pixel/depth equivalence
and runtime integration, not another equivalent-work traversal claim.

Standard WASM-installer local bundle now contains Z, with unchanged N converter
and assets. Perspective correction remains default off. The own headless VM is
stopped and324,621,015 scratch bytes removed; user profile remains stopped N.
No visible launch, hardware access or firmware build/change. Goal active:
sustained hardware60FPS and usable default ground-perspective correction remain
unverified/incomplete. Further work should target measured hot data/cache costs
and remaining perspective overhead while hardware remains relinquished.

### AA (2026-10-24): DTCM placement of the existing stage texture cache

`tools/audit_texture_working_set.py` counts integer-barycentric covered texture
samples using the original exact mip threshold and matching captured descriptors.
The existing54-slot cache holds mip0 only; mip1 remains at its original address.
Captured Observatory/Deep Dark/Honeyhive views send **88.47% /87.40% /94.96%** of
covered texture samples through cached mip0. Counts include alpha reads and
overdraw before depth rejection; they are not cache-line misses or bus timings.
This supports moving existing bytes rather than enlarging/changing the cache.

The port requests **55,296B (54KiB)** from the SDK DTCM allocator after its16KiB
source-vertex cache, only when another16KiB remains available. Allocation,
alignment or size failure retains the original AXI backing. No per-texel branch:
stage descriptors bind directly to the chosen buffer during cache fill/restore.
Both mip pointers/texture bytes,54-slot weight ranking, UVs and rendering are
otherwise unchanged. Mip1 is not newly cached. AXI backing is retained for
fallback and same-binary comparison; no RAM saving is claimed.

Volatile `g_texture_dtcm_enabled` requests A/B migration once at a game-frame
boundary; `benchmark_stage_vm.py --control texture` supports it and restores the
initial flag. Migration/restore rebuilds static texels from original assets.
Marble's shared zero glass texture retains both mip pointers at the selected
zero slot. Stage replacement resets those dynamic bindings. SleepWake restores
the texture cache alongside invalidating source vertices; the restoration
algorithm is tested, but actual hardware wake behavior remains unverified.

Docker ASan/UBSan fixture `tools/test_texture_cache.py`: **4,590** full-texture/
descriptor checks across every weighted slot, DTCM/AXI toggles, both mip bindings,
invalid/aligned/undersized/NULL buffers, guard words, full-screen/wake-style
restore, dynamic zero glass and stage replacement. Stage40k and source-vertex
12,004 integration fixtures also pass. GNW copies use CPU memcpy, not DMA into
DTCM; original GBA cache layout is preserved.

SDK AA: RAM code/data79,060B, BSS501,660B, ITCM10,880B. Runtime largest spans:
RAM_EMU **160,656**, ITCM **54,656**, DTCM **38,912**, AHB **86,580** bytes. SDK
DTCM texture base0x20004000; the remaining38KiB exceeds the16KiB reserve. Normal
bundle sidecar relocation audit is0diff. Matching `aa-texture.elf`/`.bin`, source
snapshots and memory/placement reports retained. Standard WASM-installer local
release now contains AA with unchanged N converter/assets.

Matched Observatory AXI/DTCM captures have identical camera/player state,
identical **281 cached stage draw entries**, identical54x1024texel bytes and
unchanged mip1 pointers. All54 mip0 pointers bind to the correct per-slot DTCM
address in the active capture. Deep Dark reload likewise has the cache active,
no capture errors and zero faults. New DTCM byte snapshots are supported by
`capture_render_vm.py`; evidence in `texture-aa-placement.json` and captures.
Observatory first timing pair drifts; second matched DTCM/AXI pair measures
3.811/3.500 guestms/frame, so **no VM or hardware speedup is established**.
Physical placement is proven; its effect on hardware cache contention/latency
still needs actual measurements.

Corrected `point_pool_peak` reporting: previous updates covered only the leaf
claim path. Main direct-index reservations and the combined main/far prefix now
also update it, without changing any limits or allocation. This is reserved
address-prefix high water, not distinct visible/transformed vertices. Observatory
peaks at **3,245/4,096 slots**, Deep Dark at **3,486/4,096**. Earlier zero readings
were incomplete telemetry, not evidence of an unused pool. Capacity-hit counters
remain zero in these captures and the accepted route.

Fresh accepted-baseline AA replay advances1,209frames, retains4lives, observes
zero support-loss incidents/faults/geometry drops. Arena peak413, visible688,
point-pool3245; active9..47s source-cache hit90.86%. All17 common sampled ticks
differ from Y's camera/player state, invalidating equivalent-work timing claims.
Final runtime snapshot confirms DTCM enabled/active; no per-sample texture flag
was recorded by the route sampler. No incidents does not prove a hole-free map.
Reports/final image/quit-free timeline are in `route-aa/` and summary files.

Both own headless VMs are stopped;529,438,179 scratch bytes removed. No visible
launch, hardware access or Retro-Go firmware build/change. Goal remains active:
actual sustained60FPS and default ground-perspective correction are incomplete.

### AB prototype (2026-10-24): reuse rational span endpoints; new floor repro

Corrected spans can now share the rational UV at an internal boundary, avoiding
its division as the next span's start. The final span uses its last covered
pixel; no endpoint is extrapolated beyond a triangle where q might cross zero.
Internal spans still satisfy the same analytic1/16-source-texel error bound,
including two-sample spans whose endpoint lies beyond the second sample.
Each row resets its endpoint cache. `g_perspective_reuse_enabled` defaults true
inside the still-default-off perspective renderer; false retains Y's policy.
`benchmark_stage_vm.py --control spans` requires perspective already enabled.

Native Docker independent oracles pass both modes:32,220,087 rational samples
per resolution, max0.999425 model UV-unit error,2,000,424 covered samples at
160x120 and7,907,612 at320x240. Includes alpha/depth writes, signed UV limits,
1:8,388,608 depth ratios, and full-range skinny/offscreen triangles. Affine24k x2,
legacy-policy Y pixel/depth equivalence, depth/shadow and independent GBA
contracts pass. Frozen-Y specialization regression explicitly disables reuse;
new sampling policy is checked against independent rational/coverage oracles.

Captured production-helper work counts, legacy/shared q divisions:
Observatory11,636/10,585 (-9.0%), Deep Dark4,487/3,316 (-26.1%), Honeyhive
3,524/2,236 (-36.5%). All error bounds pass. This proves less division work,
not faster frames. Matched stationary VM pairs vary both ways: legacy/shared
7.122/7.256 then shared/legacy6.611/6.922 guestms/frame. No consistent VM win
or hardware speedup established. SDK RAM79,060/BSS501,660, ITCM11,048 (+168).
Affine remains FP-free/256-byte stack; perspective stack416 (+56 vs Z).
No out-of-line span or double-to-int64 calls in target shader. ITCM readback
matches, faults zero, existing DTCM reserve unchanged. AB ELF/bin/source/ARM,
legacy/shared captures, work counts and diagnostic reports are archived.
Standard local release remains AA; AB is a prototype.

Accepted AB replay with perspective/reuse enabled completes1,208frames and
four lives, no CPU faults or geometry capacity losses. Peaks arena411,
visible687, point-pool3245. Of68 common sampled ticks,65 differ from AA in
camera/player state, so the traversal is not an equivalent-work timing pair.
Final snapshot confirms perspective, reuse and DTCM enabled. Seven recorded
support-loss segments produced a **new actionable collision failure**:
sequence7 at tick2336 intersects Observatory triangle55, but runtime misses it.
The candidate list contains55; regathering does not fix the miss. Six other
segments have no geometric floor intersection.

For triangle55, relative vertices are(180,324,-45),(-231,-416,-55),
(-204,861,-55), normal(-414,9,16379). Current unrounded plane crossing yields
Q14 t7287 and rounded point(0,0,-49). The first edge should have negative sign
(unrounded triple product **-472,320**); component-wise Q8 products in
`v3_cross` instead produce **+423**, disagreeing with the other two edges and
rejecting the floor. The existing `--wide` diagnostic alone restores hit55 and
passes all seven segment/oracle comparisons. No runtime collision fix has been
applied yet. `--debug-edges` was corrected to use the current unrounded plane
calculation; its previous rounded arithmetic masked the failing edge sign.
Repro: `floor55-regression.json`, `route-ab-collision-debug-current.log`,
`route-ab-collision-wide.log`. The normal strict incident audit currently fails
on sequence7, intentionally preserving the unresolved defect as evidence.

**Next priority:** remove component-wise truncation from port ray edge signs,
validate floor55 plus full Observatory/Honeyhive ray contracts and overflow
bounds, then runtime-test. This is a confirmed floor miss, not a span-rendering
regression or draw-arena loss. A readonly hot-code audit also finds5,740B of
profiled visibility/transform routines still in AXI, with ample ITCM capacity
(`hot-ab-placement-audit.json`); placement is a later opportunity.

Both own headless VMs stopped;529,442,684 scratch bytes removed. Reports,
matching binaries/source and quit-free timeline retained. No visible launch,
hardware access or firmware change/build. Goal remains active; floor55,
default ground correction and actual sustained hardware60FPS remain incomplete.

### AC (2026-10-24): exact ray/triangle edge signs

Fixed port ray inclusion after the floor55 replay failure. Component-wise Q8
cross products could reverse an edge sign; testing the rounded contact point
also missed thin triangles even with wider products. GNW rays now use exact
oriented volumes of the ray origin/direction and each triangle edge. Matching
nonnegative/nonpositive signs establish line inclusion; all-zero volumes reject
coplanar lines and degenerate faces. Existing one-sided plane crossing, normal,
Q14 time, rounded returned contact, attribute filtering and candidates remain.
Original GBA edge calculations retain their branch.

`col_ray_edge_sign` uses unrounded64-bit crosses and a bounded fast sum for
ordinary local coordinates. Sparse ray directions skip unused components. A
two-limb signed accumulator handles larger triple products without overflow,
including full signed32 coordinates/directions; no floating point or __int128
is required on target. The independent host oracle uses __int128 instead.

Docker ASan/UBSan `tools/test_ray_edge_sign.py`: **1,223,970** exact signs match
the independent oracle, including INT_MIN/MAX inputs, cancellation, winding,
zero directions and both arithmetic paths. Tracked regression fixture
`tools/tests/collision_floor55.json` now hits55 with the production code.
All seven AB incidents agree with the static oracle; previous V/Y incident
sets pass too. Full real-stage floor fixtures now report **0/50,867 misses**
for Observatory and **0/54,460** for Honeyhive, with zero missing candidates or
capped queries. This resolves the previously recorded thin-face floor miss as
well as55. Fixtures test selected floor rays, not every point or dynamic object.

Contact contracts pass endpoint/start contact, no crossing, one-sided/outside
rays, thin-edge contact and coplanarity even under a quantized stored normal.
Intermediate rounded-contact experiments still missed5 Observatory/2 Honeyhive
probes, which motivated testing inclusion on the ray line itself; those logs
are kept as `collision-ac-rounded-*`. Final production audits require no
`--wide` diagnostic substitutions. No geometry/assets or collision tolerance
was enlarged to hide the misses.

Docker SDK AC passes final exact relocations/packing. Layout remains
RAM79,060/BSS501,660/ITCM11,048; the helper resides in the code sidecar.
Normal installed sidecar audit is0diff. Matching `ac-collision.elf`/`.bin` and
source, arithmetic/contact/regression/full-floor logs are archived. Standard
WASM-installer local bundle now contains AC and unchanged N converter/assets.
Perspective remains default off; corrected/shared mode was enabled for testing.

Fresh accepted-baseline AC replay completes **1,209 frames**, retains four lives,
and records **zero support-loss incidents, CPU faults or geometry drops**.
Arena peak412, visible695, reserved point prefix3245. Active9..47s source-vertex
cache93.02%. Of72 common AB ticks,71 differ in camera/player state: no equal-work
FPS comparison. Final runtime confirms corrected/shared rendering and DTCM cache
enabled. The exact ray replay plus native floor probes provide the floor55
regression proof; zero observed gameplay incidents alone is not a hole-free-map
claim. Evidence: `route-ac/`, `collision-ac-route-summary.json`,
`collision-ac-runtime.json`, `collision-ac-floor55.log` and contract/grid logs.

Own headless VM stopped;324,652,517 scratch bytes removed. Matching reports,
ELF/bin/source and quit-free recording retained. No visible launch, hardware
access or Retro-Go firmware build/change. Goal active: sustained60FPS still
needs hardware evidence; default ground perspective remains incomplete. Further
measured hot-code placement and deterministic workload comparisons remain useful.

### AD (2026-10-24): visibility/transforms join the rasterizer in ITCM

Linker placement now includes the seven profiled visibility/transform routines:
`geometry_cull_chunks`, `geometry_collect_leaves_c`, `collect_visible_chunks`,
`collect_visible_triangles`, `engine_transform_points`, and both rank-bit
collectors. Their bodies total **5,740B**. No game source or compiler flag changed;
expanded/reference/restored builds reused the same `main.o` with no C compile.
`SMG_ITCM_VIS=0` selects the previous raster-only layout for comparison;
`SMG_ITCM=0` retains the all-AXI selection. Both placement flags are forwarded
through Docker and the generated linker include writes only on change.

Expanded SDK layout: RAM code/data **73,328B** (-5,732 vs raster-only reference),
BSS501,656 (4B alignment reduction), ITCM **16,816B** (+5,768 including veneers).
Runtime largest allocatable spans: RAM_EMU **166,392**, ITCM **48,720**, DTCM
**38,912**, AHB **86,580**. DTCM allocations/reserve unchanged. Segment bounds,
final exact-site verification and stock two-segment GWHB packing pass; no SDK
or firmware code changes. Same-object raster-only reference and restored build
are archived (`ad-raster-reference.*`, `ad-visibility.*`); restored payload
matches the first expanded payload byte-for-byte.

Headless installed image: all16,816 ITCM bytes match the expected relocated ELF.
Temporary breakpoints hit the chunk wrapper/culler, triangle collector, leaf
collector (PC0), unmasked rank collector and transform routine at their ITCM
addresses. The masked rank collector is placed/read back but was not reached by
those Observatory breakpoints. Deep Dark reload/capture passes with zero faults
or capture errors. Exact execution/readback/layout/pool evidence:
`ad-execution.json`, `itcm-ad-readback.json`, `ad-placement-layouts.json`,
`memory-ad.json`, `ad-deepdark/`. Normal installed sidecar audit is0diff.

Baseline firmware-code check: bank1 is byte-identical. Bank2 differs only in
the normal GWLB FrogFS-length field and its CRC after provisioning (six changed
bytes, both records valid); every other bank2 byte is identical. This is the
existing GWProv `patch_layout` metadata update, not a firmware code edit/build.
Details and unchanged game-object hash in `ad-profile-input-audit.json`.

Fresh accepted-baseline replay with perspective/shared rendering enabled:
**1,208frames**, four lives, zero CPU faults or geometry capacity losses.
Peaks arena406/visible689/point-prefix3245; active9..47s vertex-cache93.19%.
Six support-loss segments agree with the independent static oracle: no floor
intersects them. Of84 common sampled AC ticks,83 diverge in camera/player state,
so no equal-work FPS comparison. No sustained60FPS or hardware speedup is claimed:
ITCM placement is verified, but hardware cache/latency effects remain unmeasured.
No repeat of noisy stationary VM timing was used as a replacement for that proof.

Standard WASM-installer local release now contains AD, unchanged N assets and
converter, with perspective default off. Both own headless VMs stopped;
529,466,477 scratch bytes removed. Matching binaries/ELFs, reference layout,
reports, images, probe scripts and quit-free recording retained. No visible
launch or hardware access. Goal active: hardware60FPS and usable default ground
perspective remain incomplete; deterministic workload comparison is also needed
before treating traversal timings as comparable.


### AE (2026-10-24): repeatable recording-derived game workload

Added `tools/replay_game_frames.py` and `tools/compare_game_replays.py`.
Two fresh disposable copies of the accepted baseline, provisioned with unchanged
AD through GWProv, run headlessly with fixed RTC. GDB intercepts each game API
entry, supplies recorded controls and nominal 30Hz Q8 time, forces game rendering,
then waits for the matching return before advancing. This avoids an IRQ-related
single-step failure that previously skipped a call. Every call verifies the
expected game-time increment. Only volatile startup auto-pause bits are cleared;
no firmware/game code is patched or rebuilt.

Protocol `api-return-v2` consumes the first B down/release in the firmware UI:
source inspection confirms that dialog waits for release before returning.
The gameplay epoch is **8.220s**, ending before final pause at48.303s. Two zero-input
warmup calls precede quantized playback. All **72 gameplay presses** are represented
at30Hz; the immutable original recording SHA is
`254a8d453e88b26439d00d5a13ca326409a855a581e5b28099dab7da057bcece`.
This is a separate controlled API cohort, not proof of equality to native input
polling or the recording's original camera trajectory. Earlier v1 repeatability
reports are explicitly superseded: they erroneously passed startup B to Mario.

Both complete v2 runs execute **1,205 render calls**, Q8 time10282/tick2409,
retain four lives and match exactly at all **201 sampled boundaries** (player,
camera and game context), final context, all geometry counters and the final
76,800-byte indexed framebuffer hash. No CPU faults or geometry-buffer losses;
peaks arena409/visible684/reserved-point-prefix3245. Comparison rejects missing
boundary samples and incomplete render counts. Evidence: `replay-ae-v2-a/`,
`replay-ae-v2-b/`, `replay-ae-v2-repeatability.json`,
`replay-ae-v2-control-integrity.json`, `replay-ae-v2-summary.json` and archived
`ae-v2-*.py`. Final image equality covers indexed bytes, not a palette comparison
or every rendered frame. Perspective stays default off in this cohort.

Host durations92.69/92.58s are **not FPS**. Firmware caller skipped-blit counts
62/72 differ despite forced game render calls, so this harness cannot establish
whole-pipeline scanout or hardware performance. A normal guest-timeline trial
with GWemu baseline instruction timing completed only one frame in65 host seconds;
inspection found firmware `cpumon_common`/WFI, DWT disabled and no fault. Precise
stall cause remains unproven; that failed trial supports no speed comparison.
Normal timeline playback was already guest-clock driven. Its optional coherent
snapshot now pauses/resumes QMP while reading, but need not land at a frame boundary.

Both owned VMs automatically stopped with exit0. Removed **529,461,344B** of owned
scratch after retaining reports/logs/source snapshots. No visible instances,
hardware access or Retro-Go firmware code changes/builds. Release remains AD.
Goal active: deterministic workload validation is now available; sustained60FPS
still needs hardware evidence, and usable default ground perspective remains open.


### AF (2026-10-24): opt-in M7 packed vertex transforms

Added a DSP prototype in `engine_transform_points`. Pack the XY matrix
coefficients once per job, then use three scalar MUL plus three signed dual
multiply-add (`SMLAD`) instructions per vertex, replacing nine scalar MUL/MLA
instructions. Coordinates still use aligned halfword loads; six-byte source
stride does not permit assuming word alignment. An initial memcpy experiment
produced a per-vertex veneer call and was removed before runtime validation.
The existing once-per-job input snapshot copy remains.

The packed path requires signed16 coefficients and each row's coefficient L1
sum<=65535. This bounds all signed dot-product intermediates for every signed16
source coordinate. Wider/scaled matrices retain the scalar path, with unchanged
Q8 rounding, translation, clipping, projection and destination remapping.
`g_transform_dsp_enabled` is **false by default** pending hardware measurement:
packing checks and the larger function may offset arithmetic savings, especially
for small figure jobs. The candidate replay used a build with it enabled; final
SDK build/ELF verifies its default initial value is zero. The standard local
release zip still contains byte-identical AD, checked directly inside the zip.

Docker ASan/UBSan: **2,469,144** packed dot products match independent int64
arithmetic;823,048 accepted/392,952 rejected matrices cover signed32 coefficient
bounds and full signed16 coordinates. Independent visibility/transform fixture
passes4,000 jobs including both wide-coefficient and L1 guard fallbacks.
12,004 cache/remapping comparisons now use the scalar uncached baseline against
packed cached transforms; protected warm-source, invalidation and mutable bypass
checks remain. Source extraction now uses an explicit transform-section terminator
so nested ARM preprocessor guards are included.

Both candidate-on and final-default-off Docker SDK builds pass exact relocation
verification and packing. Layout RAM73,332/BSS501,660/ITCM17,248, +432ITCM bytes
versus AD; transform function1,116->1,544B. No new memory-pool allocation.
Headless candidate execution probe reaches **PC0x3de2**, the first `SMLAD` in the
installed ITCM path. Full controlled accepted-recording replay matches AD at all
201 sampled states, final context, every geometry counter and final indexed
framebuffer hash:1,205render calls, tick2409/time10282, four lives, zero faults
or capacity losses. This is correctness evidence, not a hardware speedup.
Host93.63s vs92.69s does not establish FPS or regression; driver/firmware skipped
blits and debugger overhead vary. No whole-frame or every-image equality claim.

Evidence: `transform-af-{dsp,visibility,cache,build,default-off-build}.log`,
`transform-af-arm.txt`, `transform-af-layout.json`, `route-af-dsp/`,
`transform-af-replay-comparison.json`, `transform-af-summary.json`,
`af-transform.*`, `af-default-off.*`, and archived source/fixture snapshots.
The replay helper's optional `--probe-address` records a first-call execution
hit without changing game code. Own VM automatically stopped with exit0;
**324,657,800B** scratch removed. No hardware access, visible launch or Retro-Go
firmware code build/change. Goal active: DSP cost needs hardware calibration;
default ground perspective and sustained60FPS remain incomplete.


### AG (2026-10-24): selectable texture mapping and route captures

The pause menu now exposes **Texture mapping: Fast / Accurate**, using the
existing app settings API (`tex_mapping`, within its11-character key budget).
Accurate enables the bounded perspective renderer independently of the optional
depth test. Fast remains the initial default; the packed-transform prototype
remains off. New game getter/setter functions keep the port UI independent of
renderer-private state. This round compiled/linked the menu and verified runtime
modes through the debug control; interactive menu navigation and reboot
persistence were not exercised. The settings implementation uses the same path
as the existing FPS/detail/depth choices, without firmware changes.

Headless controlled accepted-route tests in fresh baseline copies: both modes
complete1,205render calls, four lives, no CPU faults or geometry-buffer losses.
Fast matches the previous AE/AD baseline exactly at all201 sampled contexts,
final context, all geometry counters and final indexed framebuffer hash.
Accurate matches the same motion/context/geometry but changes texture output as
expected. Five corresponding captures (calls120/300/600/900/1205) have identical
camera/player context and palette bytes; world previews change19,496..35,192
scaled pixels. No image-equality claim across different mapping modes.

`replay_game_frames.py` now supports explicit mapping selection and bounded
frame-number captures of stage arena/depths, indexed bytes, palette and a
world-only PNG. The world uses the upper-left160x120 region of a stride320
buffer; preview generation crops that region and scales it to320x240. An initial
preview incorrectly displayed unused buffer extent; those PNGs were replaced.
Native HUD/firmware overlays are composited separately and are not in these
world previews or earlier indexed-buffer hash comparisons. Full-resolution
menu capture dimensions are handled separately by the helper.

Docker production-helper audit of the five Accurate stage captures:
**118,093** covered pixels including stage overdraw,24,752spans,11,370rows,
**41,796 reciprocal divisions**. Max error0.991445 model UV units =0.0619653
source texels, within1/16texel. Captures exclude immediate actor triangles,
setup cost and hardware timing; no FPS inference. At least**5,674** divisions
are retries beyond one endpoint per span plus one initial inverse per row.
Testing the error bound before dividing is the next candidate. None of the
**1,138 textured stage triangles** has equal three-vertex reciprocal depths,
so a constant-depth shortcut offers no benefit in these views.

Docker SDK final exact-site/packing passes. Layout RAM73,612/BSS501,668/
ITCM17,248. Standard local WASM-installer bundle now contains AG; packaged
SMG.bin bytes match the tested build. Converter/assets were not regenerated;
current hashes are recorded. README's stale320x240 world/performance claims
were replaced with the current raster/output/options and verification limits.
Evidence: `mapping-ag-{build,install,config,local-bundle}.log`,
`route-ag-{accurate,fast}/`, `mapping-ag-{route-comparison,fast-comparison,
image-comparison,span-work,cost-summary,summary}.json`, `ag-mapping.*` and
archived source snapshots. Both owned VMs autostopped with exit0;
**529,481,107B** scratch removed. No visible launch, hardware access or Retro-Go
firmware code build/change. Goal active: quality control is usable in the port;
interactive persistence, difficult-scene hardware costs and sustained60FPS remain
unverified. Further work should reduce adaptive-span retries without relaxing
its error bound or reducing geometry/detail.


### AH (2026-10-24): experimental pre-division perspective-span rejection

Candidate clears the positive endpoint denominator from the existing chord-error
bound: `abs(un-u0*q1)*abs(q1-q) <= 3.996*min(q,q1)*q1`, likewise V. This tightens
the previous1-UV-unit budget by0.1% for rounding margin. Rejected trial lengths
need no endpoint reciprocal; the accepted endpoint still uses the hardware
F64 divide. Legacy control retains the previous bound/outputs. New private
`g_perspective_precheck_enabled` is **false by default**: reducing division counts
alone does not establish lower total cost, especially with few retries.

Docker independent rational/coverage/alpha/depth fixture passes both modes and
both endpoint-reuse settings at160x120/320x240:64,421,551span samples per resolution,
max0.999425model UV units across all modes,4,023,264/15,386,740production covered
samples. Captured legacy-helper reports match AG exactly. On the same five
accepted-route stage captures, reciprocals fall**41,796->33,617 (-19.57%)**,
with unchanged covered-pixel totals and max candidate error0.990226model units
(<1/16source texel). Two extra spans occur in frame120 due to the tighter bound.
However, preserved expensive Deep Dark capture improves only**3,316->3,232
(-2.53%)**. Extra multiply/bound work may outweigh that small reduction; no speedup
or FPS gain is claimed. Next experiment should hoist invariant curvature/error
terms into row setup rather than recomputing them at every span.

Candidate-on and final-default-off SDK Docker builds pass exact relocations and
packing. Layout RAM73,612/BSS501,668/ITCM17,648(+400 vsAG). Own perspective function
stack decreases416->392B (locals plus callee-saved GP/FP registers, not the entire
nested call chain). Helper remains inline with F64 VDIV; existing setup conversion/
integer-divide helpers remain. No new memory-pool allocations.

Full candidate-on controlled accepted replay completes1,205calls, four lives,
no faults/capacity losses. All201 sampled contexts, final context, all geometry
counters and final indexed world-buffer hash match AG Accurate. All five matched
world-only checkpoint PNGs and palette bytes are identical. This proves those
recorded comparisons, not every frame, HUD or physical FPS.

Shared GWProv daemon rejected launch because its source was stale while another
substantial device lease was active. No forced restart or hardware operation was
performed. Retried via supported project-local XDG runtime/config/state paths
with an empty remote adapter registry, leaving global target-lease configuration
unchanged. Own VM autostopped with exit0. Verified private daemon PID and empty
VM list before terminating only that owned service; no force-drain. The failed
initial launch log is retained. Removed**329,971,865B** scratch afterward.

Evidence: `span-ah-{oracle,build,default-off-build,captures,legacy-captures,
deepdark,deepdark-legacy}.log/json`, `span-ah-{arm,reference-arm}.txt`,
`route-ah-accurate/`, `span-ah-{route-comparison,image-comparison,summary,
private-service}.json`, source snapshots and both candidate/default-offELFs/bins.
Standard local release remains AG (zipped SMG.bin bytes verified). No visible
launch, hardware access or Retro-Go firmware code build/change. Goal active:
prototype cost, remaining rendering work and sustained hardware60FPS unverified.


### AI (2026-10-24): row-invariant perspective curvature prototype

Replaced AH's per-span numerator precheck with a row constant:
`C=max(abs(duq*q-uq*dq),abs(dvq*q-vq*dq))*abs(dq)/3.996`.
The numerator terms are invariant along a row. Trial distanceL passes when
`C*L*L <= min(q,q1)*q*q1`; rejected lengths need neither endpoint attribute
calculation nor reciprocal. The same0.1% tighter error budget remains.
Renderer samples the private control once per triangle and computesC once per
visible row. No additional persistent buffers. Legacy mode remains available;
`g_perspective_precheck_enabled` stays**false by default** pending M7 cost tests.
This replaces the AH implementation, which remains archived as a reference.

Independent Docker oracle expanded to full native row width and large triangle-
area scaling, including constant UVs with extreme depth variation to exercise
cancellation. Both modes/reuse settings pass**64,403,336 /128,352,456**span samples
at160x120/320x240, max errors0.999495/0.999282model UV units. Production covered
samples4,133,904/17,340,432 pass independent rational sampling, coverage and alpha/
depth checks. Disabled-prototype legacy specialization passes1,500four-triangle
groups/resolution against validated Y pixels/depth. This is optimization
regression evidence, separate from earlier original-GBA/asset-contract proofs.

All six preserved stage-capture reports exactly match AH precheck's span/division/
error statistics. Reciprocal reductions versus AG remain19.57% across five
Observatory frames,2.53% in preserved Deep Dark. Full enabled-prototype accepted
replay:1,205render calls, four lives, zero faults or capacity losses. All201sampled
contexts, final context, all geometry counters and final indexed world hash
match AG Accurate; all five checkpoint world PNGs/palette bytes are identical.
The debug replay helper now supports explicit `--span-precheck on/off`; report
verifies defaultfalse before override and true afterward.

Added `benchmark_perspective_spans.py`: pinned x86_64 host process CPU time,
Debian GCC12.2,20,000passes/capture, three Latin-square orders and checksums using
all five output fields. Initial reports omitted UV-step outputs, permitting
optimizer elimination; these reports are explicitly superseded. Corrected
complete-output medians (reference AG /prior AH /row AI):
Observatory frame300 **0.658114 /0.722646 /0.593072s**; Deep Dark
**0.253446 /0.275331 /0.246797s**. Row version uses9.88%/2.62% less host CPU time
than AG for those span kernels (17.93%/10.36% vsAH). This excludes pixel shading,
setup outside the helper, interrupts and memory placement; it is NOT an M7 or
whole-renderer speedup/FPS measurement. Raw runs retained, including variance.

Docker SDK exact relocation/packing passes: RAM73,612/BSS501,668/ITCM17,800,
+152ITCM vsAH. Perspective own stack416B (24more thanAH, same asAG; not nested
chain). Row helper inlines; default affine ARM path remains FPU-free. Standard
local release stays AG, with zipped SMG.bin byte comparison. Candidate ELF/bin/
source archived. Owned isolated GWProv VM autostopped0; verified private service
PID3207135 then terminated only that service after emptyVMlist. Shared daemon,
global hardware leases and hardware remain untouched. Removed**329,975,400B**
owned scratch; no visible instance or Retro-Go firmware code build/change.

Evidence: `span-ai-{oracle,oracle-scaled,legacy-specializations,build}.log`,
`span-ai-{captures,route-comparison,image-comparison,summary,private-service}.json`,
`span-ai-host-kernel-complete.json` (authoritative host benchmark),
`span-ai-{arm,affine-arm}.txt`, `route-ai-accurate/`, `ai-*`snapshots.
Goal active: M7 calibration and sustained60FPS still require eventual hardware;
this host diagnostic does not justify enabling the prototype by default.


### AJ (2026-10-24): guarded signed32 chunk-cull intervals

The common chunk-cull path now uses signed32 dot products with a job-level
eligibility check. For each plane, both coefficient triples must have L1<=32767;
plane offsets retain at least8,388,672units of signed32 headroom. Given signed16
chunk bounds, centers are bounded by65536 and extents by65535. Products, partial
sums, both halved-dot combinations, Q8 shifts and +/-64 tolerances fit signed32.
Both original halvings remain separate, preserving odd/negative rounding.
All other inputs retain the original wide path. Cone math, inherited plane
masks, hierarchy traversal, ordering and coverage remain unchanged.
`g_cull_narrow_enabled` defaults on; the separate `smg_cull_math_stats` counts
job eligibility. It adds no per-plane counter writes or dynamic allocation.

Docker ASan/UBSan: **720,830 narrow intervals** match an independent wide/floor
oracle;855,882jobs safely reject the narrow guard. Includes full signed16 bounds,
independent signed center/radius coefficients, L1limits, extreme offsets and
+/-64headroom. Existing4,000hierarchy/collection/transform jobs and independent
GBA/asset contracts pass, including UV orientation, axis/backface and cones.

Full controlled accepted route:1,205render calls, four lives, no faults or
capacity losses. All**2,410cull jobs** use the narrow path. All201sampled contexts,
final context, all geometry counters, final indexed world hash and five world
checkpoint PNGs/palettes match AG Accurate. First cull jobs at the five capture
points are preserved for future data-traffic profiling.

Actual ARM debugger-step comparison on one frozen initial Observatory main job:
640chunks/77ordered leaves. Original archived AI ELF: **38,256**transitions;
new narrow path:**32,740 (-14.42%)**. Planes, camera, depth limits and chunk bytes
match exactly, as does ordered leaf output. Excludes the common external memcpy
callee. This counts debugger step transitions inside the culler, not cycles or
whole-visibility/frame time. Switching off narrow math in the new binary takes
38,859steps; the archived-original check prevents that added control overhead
from overstating the gain. No hardware FPS or14%visibility-stage gain is claimed.

DWT baseline trials were rejected: raw deltas frequently aligned to280,000-count
timer periods; a separate interrupt-mask trial failed readback. Those invalid
reports remain explicitly marked, with no speed claim based on their medians.
`benchmark_cull_vm.py` now enforces calibration/stability for DWT and offers the
separate step-count mode, rejecting unexpected code excursions. Native bundle
packager accepts paired `--elf/--homebrew-bin` to reconstruct archived sidecars
and exact relocation tables without rebuilding any firmware. That path installed
and executed the original reference successfully.

Docker SDK exact relocations/packing passes. Layout RAM73,628/BSS501,668/
ITCM18,536; +16RAM/+736ITCM versusAI. Culler own stack512->728B, excluding nested
callees. Preserved firmware ELF reserves a20KiB stack/redzone window
(0x2001b000..0x20020000); peak call-chain stack was not measured. No firmware
reservation or heap-pool layout changes. Standard local WASM-installer bundle
now contains AJ; zipped homebrew bytes match the tested binary. Texture mapping
remains initially Fast; DSP and row-curvature experiments remain off.

Owned route/reference VMs stopped0. One completed step-test exec handle vanished;
its report was retained and exact PID absence plus refused private endpoint
verified termination, rather than restarting the test. Private recovery tried
to inspect an unrelated shared daemon when its own endpoint was dead; that
attempt was blocked by the foreign hardware lease. Started only the project-
local `gwprov.daemon --serve` directly, with empty remote-adapter config, avoiding
shared recovery. Finally verified empty owned VM list and terminated only owned
private PID3252849. Shared daemon/hardware untouched. Removed**1,077,558,819B**
owned scratch, including verbose control logs (hash/size summaries retained).
No visible launch or Retro-Go firmware code build/change.

Evidence: `cull-aj-{intervals,visibility,contracts,build}.log`,
`cull-aj-kernel{,-masked,-steps,-original}/`, `cull-aj-{arm,reference-arm}.txt`,
`route-aj-cull/`, `cull-aj-{route-comparison,image-comparison,summary,
private-service,control-log-summary}.json`, `aj-*`sources/ELF/bin. Goal active:
hardware60FPS remains unproven. Triangle collection and external-flash data
traffic remain important candidates; the culler is only part of visibility.


### AK (2026-10-24): optional SRAM chunk-table cache

All linked main+far chunk descriptors fit one128KiB slab; largest pair is
Good Egg130,848B, Deep Dark92,832B, Observatory22,496B. Indexed stages do not
store the conceptual20B leaf records; the table-size audit records that distinction.
The cache copies only immutable32B chunk descriptors, not triangles or visibility
results. Culling keeps the same code and indices, with no per-node cache lookup.
SDK reserves131,104B (128KiB plus32B alignment allowance) only when at least
32KiB remains free. Actual bindings are32-byte aligned. Failed allocation or
an oversized pair retains both canonical flash pointers; no partial-table bind.
Lower-memory/native-raster builds can therefore continue without this cache.

One allocation is reused across stages. Binding follows both main/far loads,
reconstructing canonical sources from headers even after a same-header reload.
Frame-boundary debug toggles rebind pointers; failed fits do not retry each frame.
Turning the cache off does not release its SDK bump allocation. SleepWake forces
a recopy alongside the existing texture restore. No asset/converter/dist-format
change or firmware modification. Hardware SleepWake itself remains untested.

Docker ASan/UBSan production fixture: **69 real-stage byte/binding checks** cover
all10linked pairs, null/misaligned allocation, exact/short capacities, overflowing
counts, stage changes, same-header reload, disabled fallback and corrupted-buffer
restore. Cached production culls succeed with canonical chunk pagesPROT_NONE,
proving warm traversal does not read original chunk storage. Source and cached
ordered leaf outputs match; buffer guards remain intact.

Live headless readback confirms main/far SRAM bytes equal canonical NOR bytes,
above loaded BSS0x240d77e8 and below RAM_EMU end0x24100000; slab starts0x240d7800.
Startup free spans after allocation: RAM_EMU**34,808B**, ITCM47,000, DTCM38,912,
AHB86,580. These are this preserved firmware/profile's startup allocator queries,
not hardware measurements or whole-pool totals. Core layout RAM73,784/BSS501,680/
ITCM18,536; only RAM_EMU receives the new allocation, other reservations unchanged.

Accepted recorded route completes1,205render calls, four lives, zero faults or
capacity losses. All201sampled contexts, final context, every geometry counter,
final indexed world hash and five world checkpoint PNGs match AJ. Cache fills
once (22,496B); all five captured main cull jobs use its SRAM address. Separate
three-call cold stage7Deep Dark cache-on/off smoke cohorts match states, geometry
and indexed image; copied92,832B readback is exact. Stage8Gusty Garden also passes
cache readback (104,192B). Stage IDs were verified in generated assets; the first
smoke's mislabel was corrected. These cold loads are explicitly separate from
the accepted recording trajectory and provide no gameplay/FPS benchmark.

Standard local WASM-installer bundle now contains AK; packed SMG.bin bytes match
the tested build. Asset and WASM SHA256s equal AG/N values. Source snapshots,
`visibility-ak-{table-sizes,summary,route-comparison,image-bindings,
deepdark-comparison,private-service}.json`, `visibility-ak-{cache,build}.log`,
`route-ak-cache/`, cold-stage directories and `ak-visibility.*`retain evidence.
Sparse/reflink-capable disposable copies were used; both profile trees and all
owned scratch removed (534,957,248logical bytes). All four test VMs stopped0;
only verified private service PID3271686 terminated after emptyVMlist. Shared
daemon/hardware untouched; no visible launch or Retro-Go firmware code build.
Goal active: SRAM placement is proven, hardware speedup/sustained60FPS are not.
A useful next target is visibility's direct anchor/axis-coordinate flash reads:
the existing DTCM source-vertex cache currently serves transforms only.

### AL (2026-10-24): visibility vertex-cache reuse measured, not promoted

Prototyped read-only reuse of the existing16KiB DTCM transform source cache in
triangle collection. Only warm main-stage coordinates qualify; cold ownership,
misses, conflicting tags, far/mutable sources, missing allocation and empty tag
65535 use canonical points. Collection never fills/evicts cache entries. No new
allocation. Instrumented build adds456B ITCM and8B RAM relative to AK; counters
and lookup branches themselves have a cost.

Docker fixtures pass4000 independent-reference collection/cull/transform jobs
with mixed cache states and12004 transform comparisons. Indexed anchor and
indexed/nonindexed extra axis-coordinate reads work with source pagesPROT_NONE;
cache bytes remain unchanged. Original texture/GBA rejection contracts pass.

Accepted1205-call route:38,561hits/14,421misses (72.78%), across960,906tested
triangles. Only about44eligible point lookups/frame: nonindexed leaf records
already embed anchors and often do not need extra point reads. All201sampled
states, geometry totals, final indexed image and five world checkpoint PNGs
match AK, no faults/capacity losses. Separate30-call cold Deep Dark stage7
cohort:15,629hits/42,836misses (26.73%),66,318tested triangles. On/off state,
geometry, final indexed image and world checkpoint match. This stationary
opening cohort is not a substitute for the accepted gameplay trajectory.

These counts prove reuse/correctness, not an ARM speedup. The low Deep Dark
hit rate and extra miss checks do not justify adding this to the release.
**Restored AK production source and SDK artifacts**; ELF and SMG.bin SHA256s
match their archived AK copies exactly. Standard local bundle stays AK. The
probe source/ELF/bin, fixture/replay snapshots, logs and comparisons remain as
`al-*`, `visibility-al-*`, `route-al-vertices/` and Deep Dark reports in the audit
directory. Reproduction needs the archived probe source and matching fixture/
replay snapshots; the current runtime intentionally has no probe control.

Three headless VMs stopped successfully, both owned private daemons terminated
after empty owned-VM lists. Removed330,324,081logical scratch bytes. Shared
GWProv service and hardware untouched; no visible launches or firmware builds.
Goal active: physical sustained60FPS remains unproven. Next investigate exact
signed32 facing products under a per-job camera-range guard: current collection
uses three64-bit products for each tested triangle, including embedded anchors.
Preserve each product's separate arithmetic right shift and wide fallback.

### AM (2026-10-24): guarded signed32 triangle-facing arithmetic

Collector now uses signed32 products when every camera coordinate fits signed16.
Anchors are signed16; leaf normals are signed16 and indexed normals are doubled
signed8. Thus |anchor-camera|<=65535, each product magnitude<=2,147,450,880,
and the sum of three separately shifted products fits signed32. Keep each
product's arithmetic >>8 before summation to preserve original rounding. Wider
camera jobs retain the previous64-bit calculation. Debug control
`g_collect_narrow_enabled` defaults true; `smg_collect_math_stats` counts narrow/
wide jobs. No new allocation, asset/converter changes or firmware modification.

Docker ASan/UBSan fixture `tools/test_collect_facing.py`:2,028,561exact cases
against independent int64 explicit-floor rounding, plus1,331camera guards
including full-int32 extremes. Visibility fixture compares4000complete jobs
against independent reference, exercising both guards/controls with boundary
and out-of-range cameras; original GBA rejection/texture contracts also pass.

`tools/benchmark_collect_vm.py` freezes initial Observatory collection inputs,
restores ABI/job/output buffers between paths, and counts actual ARM debugger
steps. Unexpected execution excursions are rejected; compiler-emitted memcpy
and veneers are counted, not excluded. Same77leaves, camera, canonical source
hashes, marks/bits/plist initial bytes and resulting outputs across builds:
AK83,153instructions; finalAMwide85,651; finalAMnarrow80,512. **3.176% fewer
instructions than AK in this call**, not the larger apparent gain against the
new control, not total visibility reduction, M7 cycles or FPS. First prototype
was80,549narrow; moving guard before local snapshot eliminates its109-instruction
copy and reduces own stack256->168B (AK160B). Final kernel has no helper calls.

Accepted1205-call route matches AK's201sampled states, all geometry counters,
final context/indexed image and five world previews.2409collection jobs narrow,
zero wide; zero faults/capacity losses. Paired separate30-call cold Deep Dark
stage7 on/off cohorts match5sampled states, geometry and indexed/world image;
60jobs narrow when enabled,60wide when disabled. These Deep Dark tests establish
correctness/guard eligibility, not a measured kernel speedup there. A matched
Deep Dark instruction cohort is a useful next measurement; sustained hardware
60FPS remains unproven and hardware remains relinquished.

Final SDK RAMcode73,800/BSS501,680/ITCM18,736 (+16RAM/+200ITCM from AK).
Collector's own stack168B is8B above AK; full call-chain high water unmeasured.
Firmware allocator/stack reservations unchanged. Standard local WASM-installer
bundle updated to finalAM; packed bin SHA matches tested build. Assets/WASM
SHA256s unchanged. Evidence `collect-am{,2}-*.log/json/txt`, kernel and route
reports, `am{,2}-*`source/ELF/bin/fixture/replay snapshots; final summary is
`collect-am-summary.json`. First prototype remains archived separately.

Nine owned headless VMs stopped; three private daemons stopped after emptyVM
lists. Two initial benchmark stop requests timed out; re-polled same endpoint
and retried those exact VM stops successfully. Final benchmark harness now
records bounded same-VM cleanup retries (first timeout, second stop0), avoiding
rerunning completed measurements. Shared daemon/hardware untouched. Removed
1,486,795,322logical scratch bytes; no visible launch or Retro-Go firmware build.

### AN (2026-10-24): Deep Dark collector instruction comparison

Extended `tools/benchmark_collect_vm.py --stage-index 7` to load a cold stage
at the first game API entry, restoring ABI registers before collection. This
is a separate kernel cohort, not the accepted gameplay route. Indexed home-
remapped jobs skip direct marks entirely: their29,773points exceed the direct
marks buffer. Harness now avoids snapshot/restoration of that unused pointer;
only actual outputs (rank bits and point list/job bounds) are restored/compared.

Initial Deep Dark main job:155visible leaves, indexed/home-remapped, camera
(3531,-19833,424). Both builds have identical job metadata, every canonical
source-array hash, initial output hashes, final bits/list/job bounds, fixed RTC
and baseline emulator binary/timing. Actual complete ARM debugger steps:
AK145,705; AMwide154,252; AMnarrow144,243. **1.003% fewer instructions than AK**
for this indexed collection call. Thus AM's3.176% Observatory result is not
representative of Deep Dark's gain. No faults or helper calls. This measures
neither total visibility cost nor M7 memory latency/cycles/hardware FPS.

The wide control's extra work is significant: measure a prototype separating
narrow/wide loops outside the triangle loop, with constant mode specialization,
to test whether branch/register overhead can be reduced. Do not infer the
larger gain against AMwide as an improvement over the original. Production
runtime and release remain the already validated AM2; no new build/conversion.

Two owned headless VMs and two private daemons stopped; shared GWProv and
hardware untouched. GWProv stop exhausted retries on the first VM and timed out
on the second. Verified exact executable/profile then gracefully SIGTERMed only
the owned processes, verified terminal state and subsequently empty private VM
lists. Harness now includes this ownership-checked fallback after one failed
normal stop. Completed measurements were retained, not rerun.

Diagnostic evidence suggests accumulated step events explain cleanup latency:
private logs contain298,495GDBsteps +298,500each STOP/RESUME in the two-mode run,
and145,705steps +145,710each event in the reference run. Daemon `_read_reply`
drains/logs QMP events while awaiting command replies, with a30s deadline;
logging captures stack traces per event. This is source-supported explanation,
not a timing-isolated GWProv diagnosis; no GWProv source modified. Verbose logs
were~883MB, summarized with counts/SHA256s before removal. Future long stepping
cohorts should avoid accumulating an entire run's events before a status query.

Evidence `collect-an-summary.json`, `collect-an-control-logs.json`, private
service/cleanup reports, `collect-an-deepdark-{current,reference}/`, runtime logs
and `an-*`harness/driver snapshots. Removed1,532,554,506logical scratch bytes.
No visible windows or firmware builds; hardware60FPS remains unverified.

### AO (2026-10-24): collection arithmetic mode selected outside the loop

Always-inline shared collection body receives a constant narrow/wide mode from
the guarded dispatcher. Compiler specializes both loops, removing per-triangle
mode selection and wide temporaries from narrow jobs. Wide fallback and all
facing shifts, axis rejection, ranks, ordered point lists and capacity rules
remain intact. Same guard/control/stats ABI; no new allocations or asset changes.

Docker checks pass4000independent-reference complete visibility jobs (including
both arithmetic paths/controls, boundary cameras and pool exits),2,028,561exact
facing cases/1,331guards under ASan/UBSan, and original GBA/texture contracts.
Actual frozen ARM calls have identical job metadata, canonical input hashes,
initial outputs and resulting bits/lists/bounds to AM2 and archived AK:

| Main collection call | AK | AM2 narrow | AO narrow | Reduction vs AM2 |
|---|---:|---:|---:|---:|
| Observatory77leaves, direct | 83,153 | 80,512 | 65,304 | 18.889% |
| Deep Dark155leaves, indexed/home | 145,705 | 144,243 | 124,626 | 13.600% |

Against AK the reductions are21.465%/14.467%. AOwide counts71,272/132,858 also
improve on AK; specialization/compiler allocation contributes beyond replacing
wide products. These are complete executed debugger-step counts for particular
collection calls, not whole visibility, M7 cycles/flash latency or hardware FPS.
Both calls run without helper calls; no faults. Same fixed-RTC baseline emulator
binary and timing as reference cohorts. Full-stage performance still needs HW.

Accepted1205-call route matches AM2's201sampled states, all geometry totals,
final indexed image and five world previews;2409narrow/0wide jobs, no faults or
capacity losses. Separate paired30-call Deep Dark on/off cohorts match5states,
geometry, final indexed image and world preview. Original geometry/detail retained.
Final SDK RAM73,800/BSS501,680 unchanged; ITCM19,648 (+912from AM2). Collector own
stack152B (AM2 168B, AK160B); full call-chain high water unmeasured. Firmware
reservations untouched; static hot-code increase fits the existing ITCM pool.

Standard local WASM-installer bundle updated; SMG.bin hash matches the tested
build. Assets/WASM hashes unchanged. Evidence `collect-ao-summary.json`, kernel
and replay/image comparisons, `collect-ao-{observatory,deepdark}/`, route/paired
Deep Dark reports, logs and `ao-*`source/ELF/bin/harness snapshots.

Harness now drains only the owned VM's QMP step events every5000instructions
through read-only status queries, asserting halted state and unchanged PC.
27/50queries in the two kernel cohorts bounded the backlog; both normal GWProv
stops returned0 on their first attempts (no process fallback). Supports the
previous queue/logging diagnosis; no GWProv source change. Five owned headless
VMs and two private daemons stopped after empty owned-VM lists. Verbose control
log counts/SHA256s preserved before deleting1,320,957,258logical scratch bytes.
Shared daemon/hardware untouched; no visible launches or firmware builds.

Goal active: sustained hardware60FPS unverified, hardware remains relinquished.
Next investigate batching per-triangle instrumentation counters per collection
call, preserving complete-call totals, to measure whether debug accounting is
forcing avoidable global-memory traffic/register spills. Treat it as a prototype
until instruction counts and accepted-route results support the change.

### AP (2026-10-24): collection counters accumulated locally

Collector accumulates tested/rejected triangle counts in uint32 locals and adds
both to the existing global stats at its common return, including capacity exits.
Capacity-hit accounting remains immediate. This reduces global-memory accounting
traffic through the inner loop; stats layout/version, modulo32 behavior and
completed-call totals are unchanged. **Sample at frame/completed-call boundaries**:
a halt inside collection sees prior totals for these two fields until return.
Contract documented in `graphics/perf.h`; collection behavior/ordering unchanged.

New Docker ASan/UBSan fixture compares20,000jobs against frozen AO's immediate
accounting, not used as an independent geometry oracle. Explicitly forced wraps:
4600tested,4524rejected,564capacity. Covers initial nonzero totals, empty jobs,
already-full lists and partial early exits, indexed/home paths, guards/controls.
Independent4000-job visibility fixture and original GBA/texture contracts pass.

Matched frozen ARM job/input/output hashes, same baseline binary/RTC/timing:
Observatory AO65,304 ->AP63,411instructions (**2.899% fewer**); Deep Dark
AO124,626 ->AP121,827 (**2.246% fewer**). APwide68,779/131,511; no helpers/faults.
Complete callee debugger-step measurements, not full visibility, M7 cycles or FPS.
SDK RAM73,800/BSS501,680 unchanged, ITCM19,624 (-24from AO), own collector stack
152B unchanged. No new allocations; firmware reservations unchanged.

Accepted1205-call route matches AO's201sampled states, every geometry total
(including the batched counters), final indexed image and five world previews.
2409narrow/0wide jobs; no faults/capacity losses. Paired separate30-call Deep Dark
on/off results match, and APon also matches prior AOon for all5states/geometry/
indexed/world images. Seven paired world preview comparisons have zero differences.
Thus the accounting reduction retains the existing complete-call measurements.

Standard local WASM-installer bundle now AP, packed bin hash matches tested ELF
build; asset/WASM hashes unchanged. Evidence `collect-ap-summary.json`, kernel/
replay/image comparisons, stage/route/DDdirectories, logs, `ap-*`source/ELF/bin/
fixture/driver snapshots and permanent accounting reference fixture. Five owned
headless VMs and two private services stopped normally after empty VM lists.
Control logs summarized with counts/SHA256s;1,303,951,764logical scratch bytes
removed. Shared service/hardware untouched; no visible launches or firmware build.
Goal active: hardware60FPS still unverified, hardware remains relinquished.

Next collect transform cache hit/miss and allocator snapshots for representative
route/Deep Dark work before changing its16KiB capacity or spending more DTCM.
The remaining free pool makes a larger source cache plausible; eligibility,
texture-cache priority, reserve/fallback and actual miss reduction need evidence.

### AQ (2026-10-24): transform-cache workload and pool measurements

`tools/replay_game_frames.py --vertex-stats` reads existing counters at startup,
every6completed calls and at completion. Separate report fields preserve normal
state comparisons; no runtime control/code/capacity changed, no build needed.
Cache statistics count immutable main-stage source coordinates. Far stages and
mutable figure/pose sources bypass it intentionally; bypasses are not misses.

| Cohort | Calls | Hits | Misses | Hit rate | Bypassed |
|---|---:|---:|---:|---:|---:|
| Accepted route | 1205 | 442,723 | 33,282 | 93.008% | 387,019 |
| Separate cold Deep Dark opening | 120 | 107,525 | 62,792 | 63.132% | 68,226 |

One invalidation per cohort. Deep Dark's6-call windows remain around60–69%hit
after warmup (later scene changes vary49.9–78.5%); misses are not just initial
cold fill. This does not distinguish compulsory/working-set/conflict misses.
It supports measuring a larger cache, not assuming a speedup. Route has2859
source points; Deep Dark29773. Eligible Deep Dark transforms average1419points/
call; the cache has2048direct-mapped8Bentries (16KiB).

Live startup pointers: vertex cache0x20000000, active54KiB texture cache0x20004000.
Both cohorts' read-only SDK snapshots: RAM_EMU34,792/ITCM45,912/DTCM38,912/AHB86,580B
free after reservations/allocations. Preserved firmware ELF confirms DTCM stack
redzone0x2001b000, bottom0x2001b020, top0x20020000. Another16KiB source cache
allocation would leave22,528B free,6,144B above the existing16KiB reserve in this
profile. These are VM/profile measurements, not hardware/other-firmware guarantees.
Next test4096entries/32KiB against2048entries with matching work; retain texture
cache priority,16KiB reserve and an allocation fallback. No cache growth yet.

Read-only sampling preserves all201accepted-route states, every geometry total,
final indexed image and five world previews against AP; same ELF. Deep Dark120
is a distinct extended opening cohort, not the accepted route. Its first5sampled
states and exact completed-call30state match prior AP30call smoke. No claim of
full120call image equivalence to an uncaptured baseline; runtime ELF is unchanged.
No faults. Main documentation memory/layout summary refreshed from these results.

Evidence `vertex-aq-summary.json`, per6-call window reports, replay/image
comparison, `route-aq-vertex-stats/`, `vertex-aq-deepdark/` and `aq-*`harness/driver
snapshots. Release remains AP. Two owned headless VMs and one private daemon
stopped normally after an empty VM list. Removed330,608,066logical scratch bytes;
control-log counts/SHA256s retained. Shared service/hardware untouched; no visible
launches or firmware builds. Sustained hardware60FPS remains unverified.

### AR (2026-10-24): source cache grows to32KiB with bounded fallback

Preferred4096entries/32KiB source-coordinate cache, fallback2048entries/16KiB,
then uncached. Setter validates alignment/capacity and invalidates on resize;
only two power-of-two tiers are accepted. Transform snapshots cache pointer and
mask once per job, so output writes cannot force per-vertex global reloads.
Immutable-main-stage eligibility, far/mutable bypass and empty-tag65535 stay
unchanged. Cache stores source XYZ, never transformed positions/destination slots.

SDK startup now allocates CPU-only54KiB texture cache first, then preferred
source cache, trying16KiB if the large request does not fit or fails. Every
request preserves16KiB free after firmware reservations; AXI/NOR fallbacks remain.
Current profile: texture0x20000000, source0x2000d800, DTCM22,528B free (6,144B
above reserve), same firmware stack window untouched. RAM_EMU34,736/ITCM45,904/
AHB86,580B free. SDK RAM73,844/BSS501,692/ITCM19,632: +44/+12/+8B versus AP.

Docker allocation fixture checks1,048,584capacity/failure combinations against
independent priority/tier/reserve/order invariants. Source-cache fixture passes
12,004exact cached/uncached comparisons, alternating16/32KiB capacities, resize,
partial-buffer canaries, source changes, conflicts and home/list remapping.
Both tiers render upper-half point indices from protected source pages on warm
hits with identical outputs; empty tags and missing/misaligned/short buffers
retain fallback. Independent4000-job visibility/transform fixture passes.

| Matched cohort | 16KiB misses | 32KiB misses | Miss reduction | 32KiB hit rate |
|---|---:|---:|---:|---:|
| Accepted1205-call route | 33,282 | 1,228 | 96.310% | 99.742% |
| Separate120-call Deep Dark opening | 62,792 | 29,422 | 53.144% | 82.725% |

Eligible totals and intentionally bypassed vertices are unchanged. Same-build
Deep Dark control calls the game's setter at the first API boundary, limiting
the existing32KiB allocation to16KiB active use. It reproduces AQ's16KiB counter
totals exactly and matches32KiB's20sampled states, geometry totals, final indexed
image and checkpoint. This isolates capacity from compiler/allocation changes;
it does not simulate actual lower-memory allocation pressure in GWemu (covered
by allocator fixtures). No wall-time/FPS or physical NOR-transaction speedup claim.

Full route matches AQ/AP's201sampled states, all geometry, final indexed image
and five checkpoints; separate120-call Deep Dark also matches the old build's
20states/geometry/indexed/world image. No faults/capacity losses. Additional30
checkpoint retained. Existing scalar transform default stays; DSP remains opt-in.

Standard local WASM-installer bundle updated to AR; packed bin hash matches
tested build, asset/WASM hashes unchanged. Evidence `vertex-ar-summary.json`,
route/Deep Dark/same-code comparisons, cache/allocation/visibility/build logs,
`route-ar-vertex-stats/`, Deep Dark reports and `ar-*`source/ELF/bin/API/harness
snapshots. Three owned headless VMs and two private services stopped normally
after empty VM lists. Removed536,016,200logical scratch bytes; control-log counts/
SHA256s kept. Shared service/hardware untouched; no visible windows or firmware
builds. Goal active: hardware60FPS and hardware speedup remain unverified.
Next measure complete ARM transform kernels with controlled warm cache state,
including the existing DSP option, before selecting another arithmetic default.

### AS (2026-10-24): packed transforms enabled above measured setup threshold

`tools/benchmark_transform_vm.py` captures a real initial main-stage transform
job, performs two unmeasured scalar warmups, then restores identical cache bytes,
owner, counters, full output buffers, job and ABI registers before each scalar/
DSP call. Source/list/home/delta inputs are fingerprinted; slots are range-checked.
Counts include the matrix guard and106instructions in memcpy/veneer setup.
Unexpected execution/interrupt excursions are rejected; read-only QMP drains
assert halted state and unchanged PC. No gameplay resumes after kernel repeats.

AR's ungated DSP: both captured matrices fit the exact signed16/L1 guard; packing
adds79setup instructions and saves9per vertex. Prefix0/1/4/8/16/32/64/128 and full
jobs show8vertices still lose7instructions;16vertices win65. Thus AS enables the
existing packed path by default only for **n>=16 and a valid matrix**, with scalar
fallback for short/wide jobs. Debug `g_transform_dsp_enabled` can disable it.
No matrix/source representation change or reduced rendering work.

Final AS complete ARM counts (two warmups, not a hardware timing measurement):

| Captured main transform | Scalar control | Packed control | Instruction reduction |
|---|---:|---:|---:|
| Observatory542vertices, direct | 67,897 | 63,096 | 7.071% |
| Deep Dark1289vertices, home-remapped | 165,457 | 153,933 | 6.965% |

Job/input fingerprints match the original AR cohorts. Final0/1/8/15prefixes have
identical scalar counts with the control on/off;16/32/full benefit. Whole point
and screen output buffers match in every pair; cache work identical: Observatory
542hits/0misses, Deep Dark1079hits/210misses. Guard setup/helper costs included;
no cycles, physical cache/NOR latency, whole-stage or whole-frame FPS claim.
Own transform stack232B; no full call-chain high-water claim.

Docker2,469,144packed products match an independent int64 oracle,823,048accepted/
392,952rejected matrix cases.12,004cache/transform comparisons (both cache tiers,
protected warm source/remapping) and4000independent visibility/transform jobs pass.
Full accepted1205-call route matches AR's201sampled states, every geometry count,
cache totals, final indexed image and five world checkpoints. Paired separate
120-call Deep Dark DSPon/off cohorts and prior AR match20states/geometry/cache/
indexed/world images; no faults/capacity losses. Runtime replay flag
`--transform-dsp on/off` makes this repeatable without changing defaults/builds.

Final SDK RAM73,844/BSS501,684/ITCM19,720 (+0/-8/+88from AR). Startup free spans
RAM_EMU34,744/ITCM45,816/DTCM22,528/AHB86,580B; cache tiers/reserves/firmware stack
unchanged. Standard local WASM-installer bundle updated; packed bin hash matches
tested build. Asset/WASM hashes unchanged. Evidence `transform-as-summary.json`,
initial/prefix/final kernel reports, comparisons, replay/DDdirectories, logs,
and `as-*`source/ELF/bin/benchmark/replay/driver snapshots.

Nine owned headless VMs and three private daemons stopped normally after empty
VM lists. Removed3,990,937,496logical scratch bytes; verbose control logs were
summarized with event counts/SHA256s before deletion. Shared daemon/hardware
untouched; no visible launches or Retro-Go firmware builds. Goal active:
sustained hardware60FPS remains unverified and hardware remains relinquished.
Next audit immutable point-home table sizes/access sites for AHB placement:
large indexed stages still read those mappings during transforms/emission,
while86,580B of this profile's AHB pool remains unused by this homebrew.

### AT (2026-10-24): optional AHB point-home mapping cache

Production-layout Docker audit covers19main/far blobs: eight main-stage mappings,
none in far stages, all fit64KiB. Largest Deep Dark59,546B (29,773uint16entries).
Compiler deliberately appends unused points with homeFFFF;9,023such entries occur
across the tables. All335,730render-leaf point references have valid homes.
Cache copies every byte unchanged, including sentinels, without changing assets.

One reusable64KiB AHB allocation caches the current main table. Getter binds it
once per job into claim, transform and emission consumers; no per-point cache
lookup or remapping. Stages without a home table returnNULL. Disabled/missing/
misaligned/oversized caches return canonical pointers; frame-boundary controls,
load/same-header reload and explicit restore keep bindings valid. Failed fits
are not retried every frame. No Stage/assembly job layout or asset/dist change.

SDK preserves a16KiB largest free AHB span, queries again after allocation and
frees its own block if heap overhead/fragmentation breaks that reserve. AHB is
freeable, unlike the bump pools: shutdown unbinds then frees the owned allocation;
repeated shutdown is harmless. Wake reacquires if needed and recopies. Current
buffer0x30008dc8..0x30018dc8 lies outside preserved firmware's persistent first8KiB
and audio last8KiB (ELF audio start0x3001e000). No firmware/MPU changes. Native
heap tests cover7,077,942capacity/metadata/fragmentation/failure combinations and
actual production callback lifecycle with stubs.

91real-stage binding checks under Docker ASan/UBSan cover bytes/sentinels, null/
misaligned allocation, exact/short capacities, overflow, no repeated bypass,
stage/toggle/same-header reload and restore after clobbering. Actual production
transform and emission helpers run with original home pagesPROT_NONE; outputs
match canonical reads. This proves lookup avoidance at those consumers, not
hardware memory latency or FPS.

Live Deep Dark:59,546AHB bytes equal canonical NOR bytes; actual captured main
TransformJob.home uses the AHB address with cache enabled and canonical address
off. Stats fill once. Separate callback cohort verifies real SDK heap release:
free21,036 ->86,580B after shutdown, unchanged on a second shutdown, then21,036B
and an active/restored mapping after wake. This directly invokes callbacks in
GWemu, not physical sleep testing. Replay `--home-cache`, `--capture-transform-frame`
and `--home-lifecycle` retain repeatable diagnostics.

Accepted1205-call route matches AS's201sampled states, all geometry, final indexed
image and five checkpoints. Paired120-call Deep Dark on/off and prior AS match
20states/geometry/indexed/world image; no faults/capacity losses. Direct Observatory
has no table/copies; cache-off retains the startup allocation. Seven paired world
image comparisons are exact. Hardware speedup/sustained60FPS remain unverified.

SDK RAM74,192/BSS501,704/ITCM19,768 (+348/+20/+48from AS). Startup free spans
RAM34,376/ITCM45,768/DTCM22,528/AHB21,036B. Existing RAM/DTCM caches and reserves
remain intact. Standard local WASM-installer bundle updated to AT; packed bin
hash matches tested build and asset/WASM hashes are unchanged. Evidence
`home-at-summary.json`, table/cache/heap/build logs and comparisons, route/DD/
lifecycle reports and `at-*`source/ELF/bin/API/harness/driver snapshots.
Four owned headless VMs and two private daemons stopped normally after empty
VM lists. Removed536,091,516logical scratch bytes; control-log counts/SHA256s
retained. Shared service/hardware untouched; no visible launch or firmware build.
Goal active; hardware remains relinquished. Next audit hot emitter function
sizes/current placement against the45,768B unused ITCM budget before moving more
code. Current whole-frame costs still require eventual hardware profiling.

### 2026-10-24: Full-detail Freezeflame missing-ground reproduction

User reported an invisible ring-shaped ground surface in the visible AT VM
(`gwemu:3481321`). Saved the live API-boundary RAM/register state and camera in
`tmp/invisible-level/`; visible VM was resumed with its build/settings unchanged.
No hardware access. Host GWemu FPS is not hardware performance evidence.

A disposable headless VM with the archived AT ELF reproduced the exact camera.
The original live-snapshot replay reports 457 triangle-list overflows and one
point-pool truncation per frame. All four depth/mapping combinations produce
identical geometry losses. High detail removes those losses and restores ground,
but reduced draw distance is not the intended fix.

Captured collector rank bits contain 5,309 candidates before scenario filtering;
scenario1 needs 4,553 triangles and 3,860 distinct vertices. Whole-leaf point
allocation needs 4,924 slots for that vertex set, plus split/far reserves. Exact
trivial frustum clipping only removes37 triangles: this is not primarily an
avoidable off-screen workload. Current limits are4,096 visible triangles,
4,096 pooled vertices (including reserves), and2,048 draw entries.

Added compile-time capacity overrides with unchanged defaults. Docker SDK-only
prototype1 (points6,144 / visible6,144 / arena3,072) exposes the next limits:
1point-pool truncation,192 draw-entry drops. Prototype2 (6,400 /6,144 /3,584)
restores ground at the saved camera with all4,553 stage triangles submitted,
4,945 pooled slots,3,293 draw entries and zero capacity/depth-effect losses.
Camera-only comparison calls the real draw_scene function after normal stage
loading; simulation is held and actors are freshly loaded, so its screenshot
is not a pixel-identical continuation of the original live scene. Initial
camera/collector counts and original capacity losses agree with the snapshot.
Evidence and prototype ELF/packed bin: `build/perf/2026-10-24-invisible-ground/`.
Ground texture quality remains a separate issue; this only proves coverage at
one reported view, not all stages or trajectories.

Prototype2 adds122,368B static RAM relative to the AU placement build and leaves
about50KiB RAM before optional allocations. The128KiB visibility cache therefore
cannot allocate and correctly falls back to flash descriptors; the home/source/
texture caches are unaffected. No hardware timing claim. Pending user direction:
use correct coverage as the next baseline and recover performance, or keep AT
while redesigning geometry allocation to preserve the visibility cache. The
visible VM and standard release bundle remain AT. Default SDK artifacts are
rebuilt after the diagnostic so command-line prototype definitions cannot leak
into an ordinary incremental build. AU emitter placement remains independently
unvalidated; do not publish the current SDK artifacts as a validated release.

### 2026-10-24 AU: emitter ITCM placement validated independently

While the geometry-memory decision remains pending, completed the separate AU
placement validation with original buffer capacities. Eleven emitter routines
(7,006B function bodies) now live in ITCM through the SDK linker selection:
engine_push, clip_polygon, emit_split_vertex, engine_emit_clipped, emit_face,
emit_subdivided, emit_stage_face_bins, emit_stage_slow, emit_through_water,
emit_scroll_uv and engine_emit_stage. No emitter algorithm, asset format,
triangle order or texture policy changes. SMG_ITCM_EMIT=0 retains AXI placement.
The initial placement build used the identical main object as its AT control;
subsequent capacity-override definitions default to AT's original limits.

Default-capacity AU passes all1,205 accepted-route render calls against AT:
201sampled boundary states, complete geometry counters, final indexed image,
five world screenshots all exact. Separate120-call Deep Dark home-cache-on/off
cohorts each match AT's20states, geometry, indexed output and world image. No
faults; these default High-detail cohorts do not establish Full-detail coverage.
The Full-detail Freezeflame capacity prototype above remains a separate test.

Read the immutable ITCM image through the disposable VM's private GWProv/QMP
connection while controlled replay proceeded: all26,800loaded bytes match the
ELF after exact asset/code pointer relocations, zero different bytes/faults.
`tools/audit_itcm_vm.py` now exposes the same comparison as audit_loaded_image,
usable with a GDB or QMP memory reader at a halted boundary; its existing CLI
still additionally requires advancing gameplay. No code is patched by the audit.

Measured startup free spans: RAM41,344 / ITCM38,736 / DTCM22,528 / AHB21,036B.
Compared with AT:6,968more AXI bytes,7,032fewer ITCM bytes, other pools unchanged;
existing chunk/home/source/texture caches and headroom remain intact. SDK
RAM67,220 / BSS501,708 / ITCM26,800B. This establishes placement and equivalence,
not a measured hardware speedup or sustained60FPS. Hardware remains relinquished.

Evidence, comparisons, loaded-image audit, ELF/bin and source/driver snapshots:
`build/perf/2026-10-24-emitter-itcm/`. All three owned headless VMs and private
service stopped normally; empty owned VM list recorded. Removed331,175,112logical
scratch bytes. Visible AT instance, shared service, hardware and standard release
bundle unchanged. AU is validated for these replay cohorts but not published;
next release still depends on the geometry-memory decision. Goal remains active.

### 2026-10-24 AV: reclaim unused per-frame point reservations

This is a narrow allocation correction, independent of the pending permanent
geometry-memory architecture choice. Default geometry limits remain4,096points /
4,096visible triangles /2,048draw entries; release bundle and visible VM stay AT.
The GNW C emitter clips/subdivides on its own stack, not in g_frame_points.
Only Ocean Bowl uses additional pooled scratch: two69-point clipping polygons.
Initialize its active flag before collection and reserve138slots only when that
pass is active. Reserve far vertices only for a view that renders the far pass;
Full detail renders the main mesh and requires no far reservation. GBA paths and
their384-point reservation remain unchanged. Direct-mapped stages retain their
existing static safety margin and collection behavior.

GNW ClaimJob now appends slot_fast_limit without changing preceding offsets.
Use the compiler's worst-case leaf margin on the common path; near capacity,
preflight the actual distinct new leaves atomically. Admit exact fits and faces
that reuse already allocated leaves even if no slots remain. Stop at the first
face that cannot fit, preserving ordered-prefix admission; no priority sorting,
new point mapping, asset format or transparency change. No buffers grow here.

Docker ASan/UBSan:40,000production-claim versus independent atomic oracle cases
cover empty jobs, exact fits, duplicate leaves, full-pool reuse, capacity stops,
marks/maps/lists/counters and untouched backing guards. All111,955checked real
home-mapped stage triangles satisfy the compiler's fast-path leaf-size bound.
Accepted1,205-call replay matches AU's201states, all geometry, final indexed
output and five world screenshots exactly. Buoy Base120-call paired AU/AV runs
match20states/allgeometry/indexed/world image with the water disc active:
far reservation897slots, disc25vertices atbase2020; no capacity losses/faults.
Default AV startup free RAM41,360 / ITCM38,584 / DTCM22,528 / AHB21,036B;
SDK RAM67,204/BSS501,708/ITCM26,952B. Existing caches remain allocated.

**Correction to earlier Deep Dark summaries:** paired AT/AU reports already
contain113point_pool_capacity_hits in120API calls, although their images match.
Earlier blanket claims of no capacity losses in those cohorts were incorrect.
With AV the on/off home-cache cohorts each report112events, admit121,927ordinary
stage faces instead of100,504, and attempt36,134draw entries instead of26,461.
All20sampled player/camera states, final indexed image and captured world PNG
still match; geometry counts intentionally increase. Pool peak increases from
3,491to4,095including far vertices. Default geometry remains undersized for
this workload; screenshots alone did not prove coverage. More admitted geometry
can increase render cost. No hardware speedup claim.

Separate larger-buffer diagnostic now needs5,120points /6,144visible /3,584draw
entries to reproduce the user's Full-detail Freezeflame camera with all4,553
stage triangles submitted,4,945pooled slots and3,293draw entries, zero capacity
losses. Indexed image, palette, camera and every geometry counter are identical
to the preceding6,400-point prototype. Saves28,160B static RAM; total increase
relative to default AV is94,208B, startup RAM headroom78,256B. The128KiB chunk
cache still cannot allocate; memory-direction decision therefore remains pending.
A separate120-call Full-detail Freezeflame opening also has no capacity/fault
losses, but its2,612peak slots are much lighter than the saved user view and do
not establish whole-level coverage. tools/replay_game_frames.py now supports an
explicit --view-mode cohort, records actual geometry limits/reservations and
bounds capture reads against the ELF's arena size rather than assuming2,048.
The comparator distinguishes view-mode overrides.

Evidence: `build/perf/2026-10-24-point-budget/`, including native fixture results,
default/diagnostic ELF/bins, replay comparisons, active-water capture and exact
saved-camera comparison. Default SDK artifacts restored after prototype builds.
All tests were headless/private; hardware remains relinquished and visible AT
untouched. Goal remains active; no sustained hardware60FPS result yet.
AV cleanup: four default-capacity replays, one archived-AU Buoy Base control,
one Full opening replay and one saved-camera render all stopped; private-service
empty VM lists recorded. Removed1,307,208,410logical owned scratch bytes. Default
SDK packed bin is byte-identical to the archived/tested AV bin after restoration.
Note point_pool_peak currently records main/far claims, not temporary Ocean Bowl
scratch; the water capture's explicit base/count and138-slot scratch bound are
needed when assessing that shared pool. Release/visible AT remain unchanged.

### 2026-10-24: measure complete point demand before truncation

Added optional --capture-claim-frame to the controlled replay harness. Diagnostic
build flag SMG_CAPTURE_POINT_CLAIMS exposes the normally inlined allocator using
noinline/noclone, and the SDK placement selector keeps that diagnostic routine
in ITCM. The52-byte GNW ClaimJob layout is asserted. The flag is disabled in
normal builds. This diagnostic changes call placement, so it is not a timing
benchmark. The capture reads the candidate list/home/leaf-start/triangle tables
before claiming, then lets the real allocator return normally. An independent
Python atomic-admission model matches actual returned triangle count, allocated
slots and unique transformed-source count at every captured frame. It also
reports hypothetical capacities without modifying any runtime limit.

Captured Deep Dark High-detail calls1/30/60/90/120 of a120-call cohort. Required
whole-leaf pool totals (including774far-reserved slots) are5,567 /5,485 /5,320 /
5,338 /2,294. Compact per-source-vertex totals would be3,303 /3,267 /3,290 /3,258 /
1,615. Thus5,120points still truncates the first four sampled views, while6,144
fits them under current leaf allocation. These are five samples, not an entire
level maximum. Visibility-list capacity hits are zero throughout this cohort,
so the sampled counts cover its complete admitted candidate sets, not a list
already truncated at4,096triangles. At call1 current AV admits840of1,223faces,
using3,312main slots/1,770referenced source vertices; a5,120point pool would admit
1,078, while6,144would admit all1,223. This quantifies why matching images alone
was insufficient to validate the old geometry budget.

The diagnostic120-call replay matches normal AV's20sampled states, complete
geometry counters, final indexed image and world PNG; zero CPU faults. Only
80additional ITCM bytes in the diagnostic build (27,032total). Restored normal
SDK packed bin matches the archived AV bin byte for byte. Default capacities,
release bundle, visible VM, shared service and hardware unchanged. One private
headless VM/service stopped with an empty owned list;325,348,434logical owned
scratch bytes removed. Evidence `build/perf/2026-10-24-geometry-demand/`.

Read-only coordinate audit: Deep Dark's same candidate sets have only980 /966 /
968 /982 /333distinct int16XYZ positions, versus2,529 /2,493 /2,516 /2,484 /841
source IDs. Frozen live Freezeflame scenario1 has4,553candidate faces,3,860source
IDs but3,011distinct positions. Captured triangles/positions were checked against
the current asset blob before comparing. Exact-position reuse could reduce main
transform work further without changing UVs, textures, normals or face order,
but no aliasing/merging optimization has been implemented or timed.

Architectural alternative for review: repurpose the existing64KiB AHB home-table
cache as a mutable per-source-ID slot map; allocate only referenced vertices.
A small frame-local coordinate hash could additionally share transforms of equal
source coordinates. Existing FramePoint.pad and pre-transform position storage
may support its chains/source comparisons without increasing the16-byte point
record; an approximately2KiB bucket table fits current DTCM headroom while
preserving the16KiB reservation. This is a design hypothesis requiring fallback,
sleep/wake, stamp-wrap, collision-scratch and exact-render validation. The measured
coordinate lower bounds do not include hash/map management cost and are not an
FPS prediction. It addresses point-pool waste but not the independent visibility
and draw-entry budgets (the saved ground view still needs3,293draw entries).
Given the measured waste, compact allocation now has stronger support than
further point-buffer increases; permanent architecture choice remains pending.

Also audited actual combined main/far chunk-table sizes: Observatory22,496B,
Buoy Base37,120B, Deep Dark92,832B, Freezeflame116,320B and Good Egg130,848B (all
stage rows in stage-chunk-sizes.json). A smaller uniform cache would omit some
stages. Stage-specific or partial cache policies are further design alternatives,
not changes already made. Hardware60FPS remains unverified; access relinquished.


### 2026-10-24 ongoing AW: compact allocation and fly-in detail

User approved compact allocation plus larger buffers within MCU limits; the prior
architecture-choice blocker is resolved. Compact exact-position claiming now
works for home-mapped main stages using the existing64KiB AHB allocation, a2KiB
AXI hash table and unchanged16-byte FramePoint records. Source IDs/UVs/topology
remain separate; only identical source XYZ share transformed storage. Fresh
bindings initialize the point-slot map; frame-local bounds reject stale entries
at stamp wrap, stage changes and after collision scratch. Transform/emit jobs use
home!=NULL,delta==NULL for a direct slot map. Legacy leaf and small direct stages
retain their prior paths. Home-cache stats version2 reports compact mode; the
existing allocator ownership/shutdown/wake callbacks are retained.

30,000Docker ASan/UBSan cases match a linear oracle, including capacity stops,
duplicate coordinates/faces, stale mappings/stamps and one-bucket hash chains.
99real-stage cache/binding checks include actual direct-slot transforms and
emission with canonical home pages inaccessible, plus reset/restore. Six paired
120-call reference runs across Deep Dark/Buoy Base/Freezeflame match sampled game
states, all admitted faces, final indexed output and nine screenshots; no losses.
Deep Dark pool peak5,566->1,771; complete main transforms285,990->111,678(-61%).
This is reduced work, not a measured time/FPS gain. Saved missing-ground camera
matches the complete reference exactly with pool4,945->3,011 and3,293draw entries.

Combined candidate limits are5,120points/8,192visible triangles/3,584draw entries.
The SDK allocates a bounded partial chunk slab rather than giving up all caching
when128KiB does not fit, preserving32KiB AXI headroom. Tested candidate startup:
RAM32,792 / ITCM37,344 / DTCM22,528 / AHB21,036B free;39,040B chunk slab covers
Observatory22,496B. Large stages correctly use canonical flash descriptors.
Exact accepted1,205-call route and actual shutdown/wake callbacks pass; loaded
28,192ITCM bytes match the relocated ELF. Evidence2026-10-24-compact-points.
These code-only results are valid independently of the detail policy below.

IMPORTANT: the first cinematic prototype forced Full main geometry. Initial
120-call camera-tour tests passed, but480-call tests through actual arrival
flight exposed134,409draw drops and two point-pool events in Deep Dark. That
policy is rejected; its early local ZIP was removed. The visible AT VM and
hardware were never updated. Do not publish the archived Full-flight prototype.

The bounded replacement retains the existing main/far geometry and at least High
cinematic detail. Existing far assets discard most opaque pixels as average
colours. A backward-compatible optional far record now preserves that same low
average byte plus a detailed texture source offset; legacy readers still see
exactly the old average. C/Python tests cover50,000records and Rust feature-gated
packer tests pass. Native assets are being generated/packed from existing
constructed inputs, with no ISO extraction. The linked WASM compiler is being
rebuilt; asset/packer/browser/GWProv parity and bounded full-flight tests remain
required before publishing. This restores far texturing, not a new high-poly
far mesh. Do not confuse passing short intro shots with a complete flight test.

1,982,046,524logical owned runtime scratch bytes were cleaned after the first
round. User screenshot/small failing-state captures and durable reports remain.
Hardware remains relinquished; no firmware build/modification or visible VM
restart. The60FPS objective remains unverified on hardware.

### 2026-10-24: renderer/material audit after the visible AW test

Visible AW-pooled instance is gwemu:3738083, GDB1238, private service/config
under tmp/smg-play-aw. User requested an audit of continuing invisible surfaces,
wrong ordering and absent texturing. The window was captured without changing
its settings or its paused/running state. No hardware access or new VM window.

Confirmed findings (not all repaired):

* Deep Dark ground00_v_tev.png and sandGrand_v_tev.png have alpha0 everywhere.
  The original J3D materials in ScaleDownRelayPlanet/GhostShipCavePlanet have
  both alpha comparisons ALWAYS and blending disabled: they are opaque despite
  the TEV output alpha. Construction preserves that alpha and the asset compiler
  infers TRI_ALPHA from the PNG. Both collapse into empty material59, referenced
  by1,325 main-stage faces. Fix requires preserving source material opacity in
  native and WASM construction, including recipe/residual semantics; do not
  globally make alpha textures opaque.
* Beach Bowl beachwater.png has alpha64 everywhere. Its recipe dithers before
  box filtering, which averages the pattern away. Compiler alpha threshold128
  drops every texel;96 main-stage faces reference empty material220. Retain
  coverage at the final texture resolution rather than lowering all cutout
  thresholds. This is separate from Deep Dark's opaque-material mistake.
* Runtime emit_subdivided narrowed each UV sum to signed16 before averaging.
  Independent production-emitter regression reproduces constant UV30000 becoming
  -2768. Fixed by averaging in signed32.1,177 source main-stage faces have an edge
  whose UV sum can overflow; this is potential exposure, not a count of frames
  visibly affected. Four signed16 endpoint/large-constant cases now pass alongside
  the40,000-case stage-dispatch fixture. Docker SDK build/pack succeeds with
  unchanged asset bytes: RAM code77,452/BSS602,716/ITCM28,184B. The local installer
  bundle was repackaged. The running AW window was not replaced; it still has
  the pre-fix binary.
* The original compiler independently clamped uv*512 to signed16, losing repeat
  scale on faces whose constructed OBJ UVs exceeded that range (for example
  Freezeflame). The current `model_triangle_uvs` path translates all three
  quantized coordinates on an axis by the same whole 512-unit texture periods
  before narrowing. This keeps the face gradient and wrapped sample phase, with
  512 units of signed16 headroom for runtime UV scrolling. An exhaustive Docker
  contract over current constructed stage meshes is recorded below.
* Detailed far payloads currently activate only during arrival cinematics.
  Normal gameplay still uses the old average colours. High also deliberately
  flattens sufficiently distant/small main-stage faces. These are detail-policy
  limitations, not asynchronous loading failures; neither policy was changed by
  this audit. Far mesh density is still the existing simplified mesh.

Checks that passed: every mip0 and meaningful mip1 texel of all2,632 linked main
materials matches canonical compiler input; current Freezeflame's54 live DTCM
cache copies and mip1 relocation pointers also match. All ten main stages have
no zero-area faces or disagreement between stored normals and triangle winding.
These checks do not establish correctness of authored single-sided geometry.
Current VM uses High, depth testing and Accurate mapping. Its51,407 rendered
frames report zero visible/point/arena/far/effect capacity drops. At capture it
was paused:76 draw entries,53 textured and23 flat, no null textured descriptors.
Historical counters describe this session, not every possible stage/camera.
Captured wall/ground ordering, cutout depth writes and deferred effects fixtures
pass. Rational perspective tests pass at160x120 and320x240. These are bounded
checks, not proof that every reported ordering error is resolved.

Reproducible read-only scan: tools/audit_stage_materials.py --elf ELF
--original build/orig/files. Durable evidence: build/perf/2026-10-24-render-audit/
(materials.json, live-cache.json, live capture, UV before/after, depth/raster
contracts, perspective tests, SDK build and packaging logs). No hardware FPS
claim. Next converter changes must retain native/browser/GWProv parity.

### 2026-10-24/25: original Honeyhive seam geometry and real water blending

User's second corner screenshot is Honeyhive, not Beach Bowl. Saved position
(-5249,-7491,-8344), camera(-5779,-7758,-8041), scenario1, High/DepthOn/Accurate.
The independent reconstructed-OBJ projection shows the same hole even with
backface culling disabled. Original ForestHomePlanet shape14 (Soil02_v), object219
at source placement(30,-1780,-920), covers it with triangles4520/4566/4567.
Restored those plus their edge neighbours4519/4561/4565/4589 to both near/far
recipes. Existing OBJ prefixes, the previous18-face repair and collision flags
are preserved. honeyhive-seam-fix.json records provenance/hash chain; the corner
fixture now checks both ledgers against original ISO vertices, winding and UVs.

Independent saved-view gap248pixels->0; actual game draw_scene replay244open
pixels->0. The seven source faces also change adjoining surface geometry/UVs;
this is restoration of the original rock, not an invented floor. Mario had floor
collision at capture. Updated Honeyhive collision passes54,656 independent
floor-ray probes over6,005 collision triangles, with no misses, lost candidates
or capped queries. Evidence: build/perf/2026-10-24-honeyhive-seam/.

User rejected water dots and explicitly chose proper blending. Beachwater's
constructed PNG is grayscale with uniform alpha64. texture_alpha blend25 now
preserves solid colour texels in both mips and exports render flagTRI_BLEND_QUARTER
(bit4). The separate build marker uses ledges bit6, avoiding collision SURF_DEATH
which also uses bit4 in the build surface flags. TRI_ALPHA remains set so far
packing preserves raw pixels. No structure sizes or GWRG protocol change.
Stage-only policy; the five ground/launcher opacity repairs remain opaque.

RM_BLEND_QUARTER=6 uses a65,536-byte XIP palette lookup indexed by bg*256+fg.
Every entry is the nearest palette colour to25%source+75%background using exact
integer RGB distances; source0 preserves background. No runtime framebuffer or
lookup allocation. Quarter-alpha faces retain perspective sampling, defer until
opaque depth is complete, test depth without writing it, and use top-left edge
ownership to prevent double blending along shared edges. Other material modes
keep their existing coverage. Legacy ARM fallback added and assembled; stale
320x240 assembly constants were corrected to match current160x120 C defaults.
Legacy GBA console execution was not tested.

Tests: all65,536 actual-palette pairs match an independent NumPy oracle; linked
and installed table bytes match. Native ASan/UBSan fixtures cover shared-edge
ownership, transparent samples, foreground occlusion, no water depth writes,
both windings/depth/perspective controls and rational varying-depth UV samples,
at160x120 and320x240. Legacy24,000-triangle pixel differentials,40,000 stage jobs,
depth and rational perspective regressions pass. All ten stages pass3,365
controlled render calls with no geometry/capacity losses or faults; this baseline
inherits DepthOff. An additional720-call Beach Bowl arrival/gameplay run with
DepthOn peaks at92/256 deferred triangles, no drops/faults. The saved water view
uses49 deferred triangles; cutout dots are replaced by continuous blending.
Accepted route retains all201 sampled states and final context. No hardware FPS
measurement or claim; hardware remains relinquished.

Final native/Chromium/Firefox/GWProv asset bytes match:32,750,308 bytes, SHA-256
7a4d87c84eb5d4c54366715bdfdc31cedac03657f03a8485410c231a5993849c.
Both project output budgets remain40MiB. Standard WASM bundle and native test
bundle use exact relocation tables. Final evidence/source snapshots are under
build/perf/2026-10-24-water-blend/; browser/GWProv summaries and stage/depth
summaries are there. Current memory figures are at the top of this document.

Visible user VM updated to gwemu:3900452, GDB1238, native frontend, audiooff,
profile tmp/smg-play-aw/profile; tmp/smg-live-session.json is authoritative.
Galaxy/mission, Mario position and camera, detail/depth/mapping settings, lives
and collected-star bits restored through the debug API. This is portable debug
restoration, not a full savestate. Mario's physics are reset through mario_reset;
no stale asset/model pointers are copied. First view restore hit an inlined
helper and then a scalar/dictionary mismatch; corrected and recovered. The final
updater records the new PID and startup registers before restoring, uses direct
intro fields and explicit scalar/vector handling, and resumes safely on a
software restore error when no CPU fault is present. Keep the current VM running;
no automatic hardware access or additional visible test windows.

Remaining appearance limits: water uses the converted grayscale material;
Wii-specific tint, refraction and environment mapping have not been reconstructed.
Far mesh density is still the simplified authored mesh. Additional audit finding:
camera to_clip still uses1648/2472 (3:2), while the current world/LCD aspect is4:3,
which gives1.125 vertical/horizontal scale for square pixels. Upstream HEAD used
120x80; current port is160x120. No FOV/aspect-policy change was made in this pass.

### 2026-10-25: fragment work and overdraw baseline

Added optional `SMG_RASTER_WORK_STATS=1` compiler instrumentation, a separate
versioned `smg_raster_work_stats` debug symbol, and replay `--raster-work-stats`.
The seven material classes count covered/depth-rejected pixels, texture reads,
zero texels, colour/depth writes, perspective spans/pixels and actual reciprocal
evaluations, including span retries. Local counters publish after each triangle;
read at completed render boundaries. Counters wrap modulo2^32. This diagnostic
changes instruction counts: its host durations and counters are not hardware
cycles or FPS measurements. There is no runtime allocation or asset change.

Compile using Docker's SDK command with
`make all -W ../src/graphics/engine.c 'OPT=-O3 -fno-strict-aliasing -DSMG_RASTER_WORK_STATS=1'`;
force that same source prerequisite when restoring the ordinary build, since
Make does not track command-line compiler flag changes. The normal homebrew
and both XIPs were rebuilt and verified byte-for-byte identical to the preceding
water release; the diagnostic symbol is absent. Existing release bundle/visible
VM remain current and unchanged. No Retro-Go firmware builds or hardware access.

Docker ASan/UBSan independent per-pixel fixtures check2,000 cases at each world
resolution, all material modes, winding/offscreen bounds, depth and mapping
controls. Equal-depth acceptance, darken bias, transparent samples, actual writes,
shared-edge ownership, span counts and reciprocal counts pass. Independent
water contracts,24,000-triangle legacy pixel regressions at both resolutions,
and original GBA texture/axis/cone contracts also pass.

Seven owned headless GWemu runs use private GWProv and a disposable copy of the
accepted baseline profile. Three paired runs compare normal/diagnostic: the full
1,205-call accepted route,480-call Deep Dark opening and480-call Beach Bowl.
All361 sampled game/camera states, final indexed buffers, geometry totals and
13 captured world images match; no CPU faults or geometry/effect capacity losses.
The seventh run repeats Deep Dark with depth off. Perspective and detailed far
textures stay enabled; no resolution, geometry, culling or ordering tradeoff.

Whole-cohort diagnostic work (overdraw-inclusive, not unique screen pixels):

| Cohort | Covered pixels/call | Depth rejected | Perspective divisions/call |
| --- | ---: | ---: | ---: |
| Accepted route |24,902|0.792%|9,594|
| Deep Dark480 calls |25,540|3.291%|10,911|
| Beach Bowl480 calls |24,493|13.759%|11,028|

Deep Dark's first180 calls (six seconds of controlled game cadence) require
33,751 covered pixels,8,701 spans and14,869 reciprocal evaluations per call.
The busiest six-call windows reach15,782 divisions/call there and23,669 in Beach
Bowl. DepthOff has exactly the same spans/divisions in Deep Dark: perspective
mapping happens before per-pixel depth rejection. This limits the value of
simply moving that rejection earlier, especially on the accepted route.

Whole totals include the two labelled zero-input warmup calls; firmware startup
and the final pause are excluded. `summary.json` also includes separate active
and opening aggregates after the first six calls, excluding warmups. These are
controlled API workloads, not a claim of physical input/timing equivalence or
sustained hardware60FPS. Depth on/off images are intentionally not equated.

Ordinary opaque textures constitute97.65% of route and98.60% of Deep Dark raster
triangles. Next prototype: constant-specialize that common shader while retaining
the complete general path and exact pixels/depth. The captured stage-only rows
have no constant-q rows, so a dq==0 reciprocal-hoisting special case currently
has no supporting workload evidence. This bounded audit excludes immediate
actors and quarter-blend water; it is not a proof that such rows never occur.

Evidence/source snapshots: `build/perf/2026-10-25-raster-work/`, including
`summary.json`, per-scene work/window reports, replay/image comparisons, exact
normal-release hashes, build logs and independent contracts.
`tools/analyze_raster_work.py REPORT OUTPUT` reproduces the work/window summaries.
Hardware remains relinquished; no sustained60FPS claim.

All seven owned VMs and the private daemon stopped normally; no owned instances
remain. Removed400,076,562 logical bytes of disposable project-local scratch.
Durable reports/ELFs and the user's visible profile were retained.

### 2026-10-25: opaque raster specialization promoted

The common RM_TEX material now has constant-specialized affine and perspective
wrappers over the same raster source. This removes per-pixel material dispatch,
cutout/blend branches and unused state. All other modes retain the general path.
`g_opaque_raster_enabled` defaults true and remains a same-build debug control;
replay `--opaque-raster on|off` changes it only in the disposable guest. Resolution,
geometry limits, detail policy, ordering, coverage, UV/depth precision and asset
bytes are unchanged. No framebuffer or runtime working-buffer allocation added.

An initial compiler-generated XIP span helper was caught during ARM inspection.
The final helper is always inline; its 72-byte inverse-step table is in ITCM.
The packaging gate `scripts/check_raster_layout.py` checks both properties and
all five raster functions' placement, and demonstrably rejects the outlined
prototype. That prototype and its preliminary same-build measurements are kept
under `outlined/`; they are superseded by the direct previous-release comparisons
below. This distinction matters: adding variants changed general-path codegen.

Final binary: 118,605 B; RAM code/data 77,612 B, BSS 602,716 B, ITCM 38,784 B.
ITCM grows 9,744 B from the water release. Total AXI static footprint is unchanged;
startup largest spans are RAM 32,792 / ITCM 26,752 / DTCM 22,528 / AHB 21,036 B.
All existing cache ownership, pool reserves and firmware stack reservations stay
intact. Own-function DWARF SP/CFA maxima: perspective opaque 392 B versus the old
408 B; affine opaque 248 B versus 256 B. This is not a complete call-chain or
hardware stack high-water measurement. Loaded ITCM code/table bytes match the
exact relocated ELF in both kernel-stage cohorts.

`tools/benchmark_raster_vm.py` measures real cold-stage shader entries from
Deep Dark and Observatory, with a fixed first game-call delta of 8 Q8 units.
Old/new canonical screen/UV/depth/mip data, initial outputs and final
framebuffer/depth/counter hashes match. Six real triangles spanning 10–1,887
covered pixels use 2.66–22.80% fewer instructions with perspective+depth enabled.
All four mapping/depth combinations improve in these cases. Counts include
material dispatch and math helpers; they are baseline instruction work only.

Measurement controls are substantive:
- The preserved firmware leaves DWT disabled. The tool enables it only in the
  disposable VM, records initial controls, and never resets CYCCNT.
- The tested GDB PRIMASK write is acknowledged but fails readback. Kernel tests use
  verified NVIC/SysTick masks instead, then restore the original state.
- Debugger-endpoint counter deltas were unstable and were rejected. A Docker-
  assembled wrapper captures endpoints inside the guest, in an owned 160-byte
  SDK ITCM allocation. An empty bx-lr call determines wrapper overhead; full
  manual stepping of the first real shader independently matches the resulting
  count exactly. ABBA repeats are identical. The wrapper is absent from the
  release and its allocation disappears with the disposable VM.

Nine final headless replay runs total 5,175 controlled game calls. Normal-mode
accepted-route, Deep Dark and Beach Bowl comparisons use the previous water
release; an affine on/off pair also matches. Baseline instruction cohorts use
both the actual previous-release ELF and the final ELF, rather than treating
same-build general code as a historical baseline. All 612 sampled game/camera
states, final indexed outputs, geometry totals and 23 world-image captures match.
All final cohorts report zero CPU faults and no visibility/point/draw/far/effect
capacity losses. These are bounded tests, not proof about every possible view.

The baseline firmware loop initially stalled waiting for audio DMA inside
cpumon_common/common_emu_sound_sync while uwTick kept advancing; it was not a
game CPU fault. `--instruction-stages` is explicitly a CPU-only cohort: after each
game API return it sets the existing volatile skip_frames field to 1 to bypass
that audio wait. Every game render remains forced. No firmware code is changed;
normal-mode rendering validation retains the unmodified synchronization path.
This cohort does not measure audio/scanout throughput. Stage endpoints are taken
inside guest game code, excluding debugger stops and firmware startup/final pause.
Reported means exclude both warmup calls. Palette/scaling/HUD after API return are also excluded.
World contains the nested visibility/transform/emission/raster counters.

Previous release -> final mean baseline instructions per game render:

| Cohort | World | Raster | Reduction: world / raster |
| --- | ---: | ---: | ---: |
| Accepted route, 1,203 active calls | 3,799,036 -> 3,363,218 | 3,379,613 -> 2,943,795 | 11.47% / 12.90% |
| Deep Dark opening, 178 active calls | 6,279,206 -> 5,743,549 | 5,327,441 -> 4,791,783 | 8.53% / 10.05% |

Difficult windows still improve: route calls 279–284 save 10.31% of world work;
Deep Dark's highest-cost control window, calls 177–180, saves 6.97%. Unchanged
visibility/transform/emission/logic counts vary only slightly with guest interrupt
phase. These numbers cannot convert the historical hardware 28.4 ms window into
a new measured frame time. Physical cache/NOR/FPU costs and sustained hardware
60 FPS remain unverified; hardware remains relinquished.

Independent Docker fixtures pass with the final source: 3,000 four-triangle
scenes at both resolutions with diagnostic counters both on/off, exact pixels,
depth and work; original GBA texture-axis/collector/cone contracts; 24,000 legacy
raster cases per resolution; water/depth/edge-ownership cases; and 192.8 million
long-double-oracle perspective samples within the existing 1/16-texel budget.
The standard local WASM bundle is regenerated and its runtime/code hashes match
the tested build. Assets/WASM modules are unchanged. `bundle.json` binds these
artifacts; no ISO conversion or firmware build was needed.

Evidence/source snapshots: `build/perf/2026-10-25-opaque-raster/`. Key files:
`summary.json`, `previous-release-kernel-comparison.json`, per-scene instruction
reports, image/replay comparisons, layout rejection/pass reports, stack CFI,
final ELF/bin and contract/build logs. Reproduce window analysis using
`tools/analyze_instruction_stages.py CONTROL_REPORT CANDIDATE_REPORT OUTPUT`.
The user's visible VM was left running without a restart; its authoritative
session file still identifies the preceding water build.

Next exact prototype: perspective setup still calls __aeabi_l2d for its signed
64-bit area/edge numerators. Guarded 32-bit conversions can avoid these helper
calls while keeping the wide fallback and exactly the same double values. Inspect
current ARM code and measure before claiming a gain. Do not gain speed through
lower detail or reduced geometry, and do not equate instruction counts with M7
cycles or physical-device FPS.

Cleanup: all 33 owned VM runs and 12 private-service sessions
stopped normally. Removed 2,591,187,035 logical bytes
of owned project-local scratch across this round, including earlier rejected
measurement attempts. Reports/source/ELFs remain under build/perf; no user profile,
shared service, hardware backup or visible VM was removed.

### 2026-10-25: exact guarded setup conversions promoted

Perspective setup now checks whether its area and both initial barycentric edge
weights fit signed32. If so, it converts those exact integers directly to double
using native VCVT.F64.S32; otherwise it retains all three signed64 conversions.
Every signed32 integer is exactly representable in binary64. The guard uses
comparisons only, so INT64_MIN/MAX also safely take the wide path. No changed
arithmetic order, rounding budget, raster coverage, material behavior or assets.
`g_raster_convert_narrow_enabled` defaults true; replay accepts
`--raster-convert-narrow on|off` for disposable same-build A/B tests.

Docker ASan/UBSan fixtures check 1,002,744 integer triples, including all mixed
integer-boundary permutations and full signed64 values: 338,757 fast / 663,987
wide. Converted double bits match ordinary wide conversion exactly. At each
resolution, 2,500 four-triangle scenes preserve every framebuffer/depth byte
across mapping/depth/material/mip/winding/offscreen controls, with both opaque
and general shaders. Independent GBA texture/collector/cone and water/depth
contracts pass. The previous rational span algorithm and assets are unchanged.

Calibrated ARM kernel measurements use the actual prior opaque release and this
build, plus same-build convert off/on controls. All real screen/UV/depth/mip data,
mip alias relationships, initial outputs and final pixels/depth/counters match.
Each of six perspective cases saves 113–115 instructions against that release;
affine cases have identical counts. `kernel-guard-operands.json` records why each
real case enters the signed32 path. Static ARM output retains the three wide
fallback calls and uses native signed32 conversions on the guarded path.

Five final headless GWemu replays total 3,550 controlled game calls: the accepted
recording, 480-call Deep Dark and Beach Bowl runs, and CPU-stage recording/Deep
Dark cohorts. The same preceding-build cohorts are the references. All 592
sampled states, final indexed images, geometry counters and 21 world captures
match. No CPU fault or geometry/effect capacity loss. CPU-stage measurements
retain the documented baseline timing/audio-wait bypass; normal validation uses
the ordinary firmware loop. Means exclude both warmups, startup and final pause.

Additional mean baseline instruction savings versus the opaque release:

| Cohort | World renderer | Rasterizer |
| --- | ---: | ---: |
| Accepted recording, 1,203 active calls | 1.459% | 1.667% |
| Deep Dark opening, 178 active calls | 1.498% | 1.796% |

Direct comparisons to the earlier water release give cumulative world/raster
savings of 12.76%/14.35% on the recording and 9.90%/11.67% in Deep Dark. These
are measured model instruction counts, not physical M7 cycles, wall-time FPS or
a sustained-60-FPS verdict. `route-vs-water.json` and `deepdark-vs-water.json` preserve those direct
comparison cohorts; percentages are not added across successive optimizations.

The change adds 136 B of ITCM, with no additional working buffer. Current SDK:
RAM code/data 77,612 B, BSS 602,716 B, ITCM 38,920 B. Startup largest free spans:
32,792 / 26,616 / 22,528 / 21,036 B for RAM/ITCM/DTCM/AHB. Shader own-frame CFA
maxima are unchanged (392 B opaque perspective, 408 B general perspective,
248/256 B affine); full call-chain/hardware high-water is still unmeasured.
Loaded ITCM code/table bytes match the relocated final ELF. The normal bundle
is regenerated; assets stay 32,750,308 B with the same SHA-256. Neither firmware
nor converter modules were built/modified, and the user-visible VM stays intact.

Evidence/source snapshots: `build/perf/2026-10-25-raster-convert/`: contracts,
kernel comparison and guard operands, replay/image comparisons, instruction
reports, stack CFI, final ELF/bin, layout/build and bundle hashes.
`benchmark_raster_vm.py --control convert` selects the new same-build control;
`--reference-image` now explicitly means fixed release defaults.

Next exact candidate confirmed in current ARM output: per-pixel depth conversion
still compares the signed raw value with zero, then separately clamps the shifted
value to at least one. For every uint32 raw value, the existing expression equals
`(int32_t)raw >= 32768 ? raw >> 15 : 1`; the maximum-65535 clamp is already redundant
and optimized away. Prototype and measure that equivalent expression before
claiming a gain. Hardware remains relinquished and sustained 60 FPS unverified.

All 9 owned VM runs and both private services stopped normally; removed
757,325,146 logical bytes of disposable project-local scratch.
Durable evidence and the user's running VM/profile remain intact.


### 2026-10-25: equivalent perspective depth quantization

The per-pixel depth rule now uses `(int32_t)raw >= 32768 ? raw >> 15 : 1`
in perspective shaders. Negative/wrapped values and positive values below one
quantized unit map to one; positive signed values cannot shift above 65535.
This preserves the old three-step rule for every uint32 input. Affine shaders
retain the original expression: a universal replacement produced small GCC
scheduling regressions on tiny affine triangles. The perspective selection
specializes at compile time, without a new runtime control or buffer.

Docker ASan/UBSan tests cover all 131,072 quotient/sign buckets at four low-bit
positions plus one million random values. Independent mip/visibility contracts
and water/depth contracts pass at both world resolutions. Calibrated ARM kernel
comparisons use the previous conversion release, preserve canonical inputs and
output bytes, and show identical affine counts. All six perspective cases save
instructions with depth on/off; depth-on savings range from 0.82% to 1.55%.
These are model instruction counts, not physical M7 cycle or hardware FPS data.

Current RAM code/data 77,612 B, BSS 602,716 B, ITCM 38,872 B (48 B smaller).
Largest startup free spans RAM/ITCM/DTCM/AHB: 32,792/26,664/22,528/21,036 B.
The standard WASM bundle is regenerated from this final ELF/bin. Assets remain
32,750,308 B with the existing water-table SHA-256; converter modules and
Retro-Go firmware are unchanged. Evidence: build/perf/2026-10-25-depth-quantize/.

Next exact candidate: replace mip-dependent row multiplication with equivalent
UV masks in texture addressing. Validate both mip layouts independently before
measuring; do not trade away perspective precision. Hardware stays relinquished.

Five final headless cohorts total 3,550 controlled game calls. All
592 sampled boundary states and 21 world captures match the conversion release,
including indexed/palette bytes, final context and geometry counters; no CPU
faults or geometry capacity loss. Guest-internal baseline instruction means
exclude warmups, startup and final pause; the CPU-stage cohorts retain the
previous documented audio-wait bypass. Normal cohorts use the ordinary loop.
Incremental world/raster savings: accepted recording
1.037%/1.187%; Deep Dark
0.914%/1.099%. Direct cumulative comparisons against the water release are
recorded separately. These measurements do not establish hardware 60 FPS.

All 11 owned VM runs and three private services stopped normally; removed
769,613,608 logical bytes of this round's disposable scratch. The user's
visible VM and hardware were untouched. Summary, comparisons, build/bundle
hashes, native contracts and cleanup records are preserved with the evidence.


### 2026-10-25: exact texture address masks

Both mip layouts now use a common V shift and a row mask chosen once per
triangle: `((v >> 15) & row_mask) | ((u >> 20) & 31)`. Mip0's mask is 0x3e0
(32 rows at stride32); mip1's is 0x3c0 (16 rows at stride64). The column's low
five address bits never overlap the row bits. This removes the per-pixel mip
branch and row multiplication, preserving V rows/U columns, padding and full
uint32 UV wrapping. A compile-time assertion guards the layout dimensions.
No new control, approximation, buffer, material or asset change.

`tools/test_texture_address.py` checks all row/column pairs and every irrelevant
input bit, including sign/wrap bits, plus one million full32 random UV pairs.
Its independent division/modulo layout oracle and named U2/V7 checks guard
against the original transposition defect. A reference shader substitutes the
old row/stride calculation and shares the remaining raster code: 2,500 scenes
of four triangles preserve every framebuffer/depth byte at each resolution,
including general/opaque paths, all materials, winding, offscreen extremes,
aliased mips and depth/mapping controls. Docker ASan/UBSan checks pass, as do
independent original-GBA texture/axis/cone and water/depth contracts.

Actual previous and new ARM images were installed in a disposable profile.
Six real triangle inputs preserve canonical UV/depth/texture hashes, aliasing,
initial outputs and final pixels/depth/counters. Kernel controls cover depth
on/off and general/opaque paths. All measured cases improve: perspective
1.84–15.82%, affine 1.82–19.19% fewer baseline instructions. With normal opaque
and depth-on controls, perspective savings are 2.77/7.59/8.89% for Deep Dark's
small/medium/large triangles and 3.82/3.70/13.63% for the Observatory's.

Five final headless cohorts contain 3,550 controlled game calls. All
592 sampled boundary states, final indexed buffers, geometry counters and
21 world captures (including indexed/palette bytes) match the previous
release. No CPU faults or geometry/effect capacity loss. Guest-internal baseline
CPU-stage means exclude warmups, startup and final pause, retaining the prior
documented firmware audio-wait bypass outside the measured call; normal
validation runs retain the ordinary firmware loop.

| Cohort | Additional world instruction savings | Additional raster savings |
| --- | ---: | ---: |
| Accepted recording, 1,203 active calls | 6.893% | 7.904% |
| Deep Dark opening, 178 active calls | 5.714% | 6.883% |

Direct comparisons against the water release give cumulative world/raster
savings of 19.62%/22.05% on the recording and
15.83%/18.65% in Deep Dark. These are model instruction
counts, not physical M7 cycles, scanout throughput or sustained hardware FPS.

The final build uses 38,536 B ITCM (336 B less), RAM code/data 77,612 B and BSS
602,716 B unchanged. Largest startup free spans RAM/ITCM/DTCM/AHB:
32,792/27,000/22,528/21,036 B. Own-shader CFA maxima are 384 B opaque perspective,
400 B general perspective, 240 B opaque affine and 256 B general affine; three
are 8 B smaller. Full call-chain/hardware stack high-water remains unmeasured.
The normal WASM bundle is regenerated: SMG.bin 118,357 B, code XIP 1,987,836 B,
asset XIP 32,750,308 B with the same water-table SHA-256. No converter modules or
Retro-Go firmware were built or modified. Evidence and source snapshots:
`build/perf/2026-10-25-texture-address/`.

Follow-up operand audit over 13 preserved stage-arena captures (excluding
immediate actors) finds 5,221 of 8,035 nonzero depth-setup divisions fit signed32;
2,814 still require the wide path. A guarded native signed divide is a candidate
for measurement, preserving zero-numerator and wide-reciprocal paths plus the
INT32_MIN/-1 exception. Eligibility is not a demonstrated speedup. Rejected a
separate edge-type narrowing idea: ARM already has native edge conversions;
only the three intentional area/weight wide-fallback __aeabi_l2d calls remain.

All nine owned VM runs and two private services stopped normally; removed
757,533,755 logical bytes of this round's disposable project-local scratch.
The user's visible VM and hardware were untouched; hardware remains relinquished.


### 2026-10-25: user release review

User requested the latest verified build in visible GWemu and a release
acceptance list. The signed32 depth-setup prototype was archived unbuilt and
production engine.c restored byte-for-byte to the texture-address release.
Updated the existing native GWemu profile via ordinary GWProv bundle install;
restored current Honeyhive mission, position/up/front, camera, stars/lives and
render settings through the debug API. It is a portable view restore, not a full
savestate. Loaded ITCM matches final ELF (zero differing bytes). Live state is in
`tmp/smg-live-session.json`: gwemu:20487, GDB1238, 60FPS/High/DepthOn/Accurate,
audiooff (real game audio is still unimplemented). Leave this VM running for
user review; no automatic update/restart while they test. Hardware untouched.
Evidence: `build/perf/2026-10-25-release-review/`. Removed the temporary installer
ZIP after successful installation; retain the live profile and private service.
The acceptance table above records current blockers and proposed recordings.


### 2026-10-25: Mario eye proportions reported during release review

User screenshot gwemu-2026-10-25-09-16-26.png shows oversized eyes. The inherited
marioeye bake spec in textures.pak applies lowxf[-1.9,114,0.8,0.7,0] to its first
two layers (base skin and separate EyeballL/R projection). Scaling the sampling
head down magnifies projected eye features. These recipe files are unchanged
from the source project. Docker reconstruction of the original atlas matches
the current PNG byte-for-byte; r/202 residual changes zero channels.

Diagnostic rebakes retain all geometry/UVs and later moustache/eyelid layers.
Restoring both sampling scales to1,1 produces smaller eye features in the
orthographic source-OBJ comparison; changing only the eyeball layer leaves much
of the oversized white area. Evidence: build/perf/2026-10-25-mario-eyes/ includes
original/variants, recipe metadata, screenshot, scripts and the comparison.
This is an asset-bake prototype, not a live game screenshot or a shipped fix.
No constructed assets, converter or visible VM were modified. Own native-bake
scratch was removed. A production change still requires native/WASM recipe
parity and headless in-game validation; preserve the user's review instance.

Fresh-install depth/mapping defaults have separately been changed to1 in
smg_homebrew.c and Docker build passed. Runtime persistence/default validation
was deferred when this eye report arrived. They are not yet promoted into the
standard bundle or user's VM; build/perf/2026-10-25-render-defaults/status.json
records that pending work. Current visible VM remains texture-address release.


### 2026-10-25: validated eye bake, shutter bounds and fresh-install defaults

The shared textures.pak now restores1/1 sampling scales in the first two Mario
marioeye layers. Only that bake spec changed; all other pack entries/recipes,
geometry and UVs are preserved. The hash-guarded reconstruction tool is
`tools/asset_construction/restore_mario_eye_proportions.py`; the provenance ledger
is asset_construction/textures/mario-eye-proportions.json. Native regeneration
changes Mario/Bee Mario/Fire Mario eye images through their existing dependencies.
Both native and Rust/WASM consume the same recipe. There are no affected residual
streams to regenerate. Source-scale changes are deliberately limited to appearance;
no animation timing, mesh topology, clipping precision or render resolution changed.

User then reported a briefly visible large gate and inability to stand still.
Live capture identified Deep Dark scenario1, ShutterDoorE object307/actor17 near
(21794,-87910,-89). Gate state SD_WAIT is active and its collision remains present.
Offline reconstruction of the captured fixed-point camera gives its scene-sort
centre(-2431,-3416,-1064): outside CLIP_SIDES, while75/7/6 triangles of its three
material parts intersect the view frustum. It had no custom bounds callback.
The gate was selected for drawing but discarded by the centre-only scene check.

The gate now supplies one conservative composite radius through both actor
selection/draw range and the scene's FigureOps.view_bounds callback. It uses the
maximum extent of all three linked parts and the largest absolute scale, rounds
up7/4 of that axis bound, and adds a fixed-point margin. All parts share joint0
and all animation frames retain fixed scale/basis, so the current actor position
is their shared centre throughout opening. Rendering still uses the ordinary
per-face clipping/depth/material paths. The launcher/cannon's behavior is unchanged.
Docker ASan/UBSan checks every linked gate vertex at six signed/nonuniform scales,
all generated animation frames, callback ownership/dispatch and owner preservation.

Paired ordinary-install headless runs at the captured camera restore the missing
gate panel:5,302 world pixels change, palettes match, and no geometry/effect drops
or CPU faults occur. This is a controlled view reproduction, not a complete live
savestate or proof of all mission/animation transitions. The saved capture and
cull proof are in build/perf/2026-10-25-gate-slide/; runtime before/after images
and state are under build/perf/2026-10-25-mario-eyes/. The initial fixture omitted
updating Mario's model transform; its images are archived under attempt-1 and
superseded by the corrected fixture and closeup.

The player's actual zero-input position stayed identical in all eight passive
samples. Floor support was true, nearly level, with neither ice nor slippery
flags; action was MA_WALL_PUSH and ratio/stick were zero. Earlier captures show
movement between observations, so this only characterizes the sampled idle
interval. No physics change was made. The perceived wall-wait animation/camera
behavior remains for user review once the wall is visible; do not report it as
a fixed floor or sliding defect. The live VM's original run state was restored
after each diagnostic capture.

First-launch defaults in smg_homebrew.c now use1 for depth_buffer/tex_mapping.
Five actual boots verify missing settings -> On/Accurate and saved00/01/10/11
all survive real settings setters, commit and restart correctly. The first
persistence fixture put temporary strings inside callee stack space; it failed
to save the requested keys. Fixed the harness to pass immutable linked strings,
then reran successfully. No production firmware setting behavior was changed.
After removing only the disposable app config, the accepted1,205-call route ran
without forcing depth/mapping. All201 sampled states, geometry totals, final
context/indexed output and five captured indexed/palette/world images match the
previous release; no faults/capacity loss. A separate frontal view changes only
12 face pixels; a closer identical-camera comparison is also saved.

Native, headless Chromium, Firefox and GWProv production conversion outputs are
byte-identical. Chromium128MiB-range conversion106.88s; Firefox113.64s. GWProv
staging uses the established explicit10-trillion fuel budget. Asset size stays
32,750,308B; new SHA-256 is
82652fa86203fa6b24b03d595a54f8deda428898d0d825f558e12afe0731c89e.
No GWRG/processor ABI or output-budget change. The standard WASM bundle was
regenerated with the matching homebrew, code, converter and exact relocation
sites. Sizes/hashes are in this round's bundle.json and gwprov-parity.json.

Final RAM code/data77,644B (+32), BSS602,716B, ITCM38,536B unchanged. The bounded
chunk cache adapts its reservation; measured largest startup free spans remain
32,792/27,000/22,528/21,036B for RAM/ITCM/DTCM/AHB. No speedup/hardware FPS claim
for these correctness changes. Hardware remains relinquished and untouched.

All12 owned VM runs (including the failed harness attempt) and three private
services stopped normally; browser HTTP/container runs exited. Removed current
scratch464,638,494 logical bytes plus379,171,477B from the first attempt; small
native-test temporary directories clean themselves. The user's visible VM was
captured for diagnosis but never restarted/updated; it remains the texture-address
review build, with tmp/smg-live-session.json authoritative. Keep it running while
the user reviews. The new bundle is validated and ready separately.


### 2026-10-25: guarded native depth-setup division

Depth interpolation's two setup divisions now use native signed32 division when
both actual operands fit and the pair is not INT32_MIN/-1. Zero numerators still
return before touching the reciprocal. Other operands retain the exact shared
Q63 reciprocal path; disabling g_raster_setup_recip_enabled retains signed64
reference division. Affine UV setup remains unchanged. No new control, working
buffer, precision, resolution, assets or material-policy change.

`tools/test_depth_setup.py` checks800,317 ratios against independent signed
division, including signed32 boundaries/overflow, valid wide magnitudes below
2^63, zero/lazy/shared reciprocals and the disabled control. Its reference
shader substitutes the original wide depth-setup calls:2,500 four-triangle
scenes at each world size preserve every pixel/depth byte across all materials,
mapping/depth/setup/opaque controls, missing inputs, mip aliasing and extremes.
Independent mip/axis/cone, water and depth contracts pass under Docker
ASan/UBSan. Actual ARM output has native SDIV sites and retains wide fallbacks;
the hot span helpers remain inline and the5 raster functions/table stay in ITCM.

Calibrated kernels compare the actual eye/shutter/defaults release with this
build. All canonical screen/UV/depth/mip inputs, alias relationships and initial/
final pixels/depth/counters match. Native eligible operands are recorded in
kernel-guard-operands.json. Normal opaque/perspective/depth-on small/medium
samples save6.13%/1.18% in Deep Dark and5.43%/0.11% in the Observatory. Both
large samples remain wide and pay0.03%/0.02% extra instructions. Across all
sampled controls, codegen/guard effects range from0.64% more to6.13% fewer
instructions; do not describe every kernel or disabled-depth case as faster.

Nine new gameplay runs execute5,895 controlled game calls: four previous-release
cohorts and five candidates. The archived previous1,205-call normal route is
reused only after confirming the emulator file identity; it is marked as reused
and excluded from this round's process count. All five paired candidate cohorts
(3,550 calls) preserve592 boundary states, final indexed output/context, geometry
totals and21 indexed/palette/world captures. No faults or geometry/effect loss.
CPU-stage means exclude startup, both warmups and final pause; the established
baseline-mode firmware audio-wait bypass occurs outside the measured call.
Normal validation retains the ordinary firmware loop. No firmware code changed.

| Cohort | Additional world instruction savings | Additional raster savings |
| --- | ---: | ---: |
| Accepted recording,1,203 active calls | 1.244% | 1.442% |
| Deep Dark opening,178 active calls | 1.543% | 1.882% |

The highest-cost complete six-call world windows improve too: route
1.495% and Deep Dark
1.805%. Detailed stage/window data are saved. These counts do
not measure physical cache/divider latency, audio/scanout or sustained hardware
60FPS. Hardware remains relinquished.

ITCM grows352B to38,888B; RAM code/data77,644B and BSS602,716B are unchanged.
Measured largest startup free spans RAM/ITCM/DTCM/AHB:
32,792/26,648/22,528/21,036B. General perspective own-frame CFA grows8B to408B;
opaque perspective384B, affine opaque240B and affine general256B are unchanged.
Full call-chain/hardware stack high-water remains unmeasured; firmware's DTCM
stack reservation stays untouched. No diagnostic work counter is linked.

The standard WASM bundle is regenerated: SMG.bin118,741B and code XIP1,988,292B.
The32,750,308B assets retain their current eye-corrected SHA-256
82652fa86203fa6b24b03d595a54f8deda428898d0d825f558e12afe0731c89e.
Converter modules and firmware were neither rebuilt nor modified in this round.
Evidence/source snapshots: build/perf/2026-10-25-depth-setup/. All13 owned VMs
(4 kernels,9 gameplay runs) and two private services stopped normally; removed
811,529,769 logical bytes of disposable scratch. The user's visible
VM/profile was not accessed, restarted or updated.

Next candidate, not implemented: unsigned32 division over absolute operand
magnitudes might accept more cases and eliminate the signed overflow exception.
The bounded13-capture stage-only audit finds6,050/8,035 nonzero pairs fit unsigned32,
versus5,221 signed32, with1,985 still wide. It must preserve an int64 signed
result, zero and the fallback, and prove that abs/sign/guard costs pay off before
promotion. This eligibility audit is not a measured speedup.


## 2026-10-25: unsigned-magnitude depth setup

Promoted the measured follow-up to the signed-divider release. Both depth
setup divisions take the absolute operands, use native unsigned32 division
when both fit, and restore the quotient sign in int64. This admits magnitudes
through UINT32_MAX without signed overflow at INT32_MIN/-1. Zero numerators
still avoid reciprocal construction; larger operands retain the exact shared
Q63 reciprocal and correction. The existing setup control retains wide signed
division for comparison. UV setup and asset/converter formats are unchanged.
The existing valid domain excludes INT64_MIN and zero denominators.

Docker ASan/UBSan fixtures pass800,487 independent ratios, signed/unsigned32
boundaries, wide quotient cases, zero/lazy/shared inverse and disabled control.
At160x120 and320x240,2,500 four-triangle scenes each preserve complete pixels
and depths against the wide-depth reference. Independent original-GBA texture,
axis and cone contracts, all water-table pairs and depth contracts also pass.

Extended tools/benchmark_raster_vm.py with optional --unsigned-depth-sample:
it selects an actual covered triangle whose depth numerator fits unsigned32
but not signed32. Existing small/medium/large classes alone did not exercise
this range. Eight paired actual ARM inputs across Observatory and Deep Dark
have identical canonical inputs, initial output buffers and all final outputs.
Opaque perspective+depth cases save0.146..3.216%; unsigned-only cases save
2.482/3.216%. Affine with depth disabled regresses0.309..2.589%, a compiler
scheduling/code-generation tradeoff retained for the measured default-path gain.
These instruction counts include helpers and dispatch, not hardware cache cost.

The first candidate run accidentally packaged raw smg_core.bin instead of the
packed SMG.bin and timed out before game entry; no raster measurements were
produced. Its failed report and stopped-process evidence are retained under
attempt-raw-bin. The packer now rejects inputs without GWHB magic before writing
outputs; the raw-input rejection was verified. Candidate benchmarks and all
subsequent gameplay runs used the correct GWHB executable.

Five candidate cohorts reuse the immediately preceding signed release reports
only after matching emulator identity, ELF hashes and input/timing contracts.
Across3,550 calls,592 boundary states and21 indexed/palette/world captures,
player/camera states, geometry, final context and images all match; no faults
or capacity losses. Incremental world/raster instruction savings are
route1.204/1.399%, Deep Dark1.060/1.297%. The busiest full six-call world windows
also improve1.143/1.066%. Startup, two warmup calls and final pause are excluded.
Evidence and reference-report hashes: build/perf/2026-10-25-depth-udiv/.

ITCM39,456B grows568B; RAM code/data77,644B and BSS602,716B stay unchanged.
Largest startup free spans RAM/ITCM/DTCM/AHB:32,792/26,080/22,528/21,036B.
Own-function CFAs general perspective400B, opaque perspective384B, general
affine248B, opaque affine232B. These are not full stack or hardware highwater.
The five hot raster functions and72B span table remain ITCM; span is inline,
diagnostic raster counters absent and release controls enabled.

Normal WASM bundle regenerated: SMG.bin119,309B, code XIP1,988,292B,
assets32,750,308B with unchanged eye-corrected SHA82652fa86203fa6b24b03d595a54f8deda428898d0d825f558e12afe0731c89e.
Converter modules and firmware were not rebuilt. Bundle/ELF/root artifact bytes
were checked; exact relocation tables regenerate normally.
All10 owned VMs (including failed packaging attempt) and three private services
stopped;1,090,777,974 logical scratch bytes removed. Hardware and the user's
visible VM/profile were not accessed or updated. Hardware60FPS remains unproven.

An offline follow-up rules against caching wide setup denominators for now:
13 stage-only captures have1,502 wide-reciprocal triangles but1,490 distinct
absolute areas per frame. Even an unlimited per-frame cache saves at most12
setups before lookup cost. This excludes immediate actors and is an upper-bound
redundancy audit, not a cache benchmark. Next performance work should examine
remaining perspective span/coverage costs using measured workloads rather than
spending memory on this sparse reuse.


## 2026-10-25: guarded narrow scanline coverage

Promoted an exact32-bit scanline-bound path. The old renderer used64-bit edge
weights for all three coverage tests on every row. Packed int16 coordinates
bound horizontal edge steps to+/-1,048,560; a clipped row is at most320 pixels,
so relevant unsigned distances/products and ceil numerators fit uint32. A
per-triangle guard checks each biased edge weight at the first and last rows.
Since each weight is linear in Y, both endpoints fitting signed32 proves every
intermediate row fits. Eligible rows use signed32 comparisons and native
unsigned division; all other triangles retain the wide coverage routine.
Interpolation, depth, shading, top-left ownership and coverage rounding do not
change. Row accumulators remain wide, including their final unused increment.
The debug volatile g_raster_coverage_narrow_enabled defaults true; kernel driver
--control coverage compares both paths without changing public game settings.

Docker ASan/UBSan checks200,000 randomized signed32 scanline intervals against
an independent per-pixel inequality oracle, including INT32_MIN and zero steps.
Every row of200,000 randomized guard cases independently confirms the endpoint
proof, with both narrow/wide cases exercised. At160x120 and320x240,2,500
four-triangle scenes each preserve all pixels and depths against a shader
forced to wide coverage across material/depth/perspective/setup controls,
extreme screen coordinates, wrapped UVs and degenerates. Independent GBA
texture/collector/cone contracts, water-table/blended shared-edge ownership and
the captured wall/ground depth regression all pass. A conservative stage-only
capture audit finds4,018/4,018 eligible triangles; it excludes immediate actors
and is eligibility evidence rather than a speed measurement.

Eight paired actual ARM inputs match predecessor canonical inputs, initial
output buffers and all final pixels/depth/counters. Same-binary narrow/wide
outputs also match in every perspective/depth combination. Against the previous
release, seven of eight opaque accurate+depth cases improve0.662..5.170%;
the large Observatory case regresses1.085%, despite narrow being0.553% faster
than wide in this binary. All-controls range-1.296..+17.118%. Compiler scheduling
and larger code have costs; promotion relies on matched gameplay measurements,
not eligibility or same-binary control alone.

The5 candidate cohorts replay3,550 calls with592 sampled boundary states and
21 indexed/palette/world captures all matching the unsigned-depth release.
Player/camera, final context, geometry totals and images match; no faults or
capacity loss. Reference report hashes/ELF/emulator identity are recorded in
reused-references.json. Incremental world/raster instruction savings:
accepted route1.992/2.318%, Deep Dark2.992/3.671%. Busiest full six-call world
windows improve2.332/4.283%, respectively. Startup, two warmup calls and final
pause are excluded; these counts are not physical M7 cycles/FPS.

ITCM grows3,128B to42,584B, with five hot raster functions, both span helpers
and row guard inlined, and the72B span-step table in ITCM. RAM code/data77,640B
and BSS602,712B shrink4B each through layout changes. Largest startup free spans
RAM/ITCM/DTCM/AHB:32,768/22,952/22,528/21,036B; bounded caches retain their
minimum reservations. Own-function CFAs: general perspective384B, opaque
perspective360B, general affine232B, opaque affine224B, all smaller than before.
These are not whole-call-chain or hardware stack-highwater measurements.

Normal WASM bundle regenerated: SMG.bin122,433B, code XIP1,988,292B;
assets32,750,308B retain SHA82652fa86203fa6b24b03d595a54f8deda428898d0d825f558e12afe0731c89e.
No converter or firmware rebuild. Root/ELF/bundle bytes and default flags match
the tested candidate, diagnostic raster counters absent. Seven owned headless
VMs and two private services stopped;697,182,274 logical scratch bytes removed.
Hardware and the visible review VM/profile were not accessed or updated.
Hardware60FPS remains unverified. Evidence: build/perf/2026-10-25-coverage-narrow/.

Next hypothesis: eligible rows still maintain64-bit edge accumulators and
reconstruct the third edge each row. Maintaining narrow accumulators could
reduce that work, but added live registers, fallback branches and spills may
lose performance. Preserve exact guard/bias semantics, avoid signed overflow
on the unused final increment, and measure before changing the promoted path.


## 2026-10-25: narrow coverage row accumulators

Promoted the measured follow-up to guarded narrow coverage. Eligible triangles
now maintain all three biased edge weights as uint32 row accumulators instead
of updating two int64 values and reconstructing the third edge each row. The
same endpoint guard proves that each used value represents its exact signed32
weight. Each row casts it to int32 for the existing span-bound routine.
Unsigned updates also define wrapping on the unused increment after the final
row, avoiding signed overflow at the guard limits. Ineligible triangles still
use the wide accumulators/routine. Perspective, depth and UV interpolation,
material behavior and top-left shared-edge ownership are unchanged. Existing
g_raster_coverage_narrow_enabled controls both guarded bounds and accumulators.

Strengthened tools/test_coverage_narrow.py checks every eligible row's three
accumulators against independent int64 expressions during200,000 randomized
guard cases per resolution. The independent per-pixel inequality oracle also
checks200,000 signed32 scanline intervals. Docker ASan/UBSan passes these and
2,500 four-triangle full-scene comparisons against the wide-coverage shader at
both160x120/320x240, including extreme coordinates and fallback cases. Original
GBA texture/axis/cone contracts, water-table/shared-edge blending and captured
wall/ground depth fixtures pass. Source and logs are archived with this round.

Eight actual ARM kernel inputs match predecessor canonical inputs, initial
buffers and every output in all perspective/depth/control combinations.
Accurate+depth saves0.274..4.226% in six cases; Deep Dark medium/unsigned-depth
regress1.565/0.903%. All-controls range-1.711..+4.664%. More live accumulator
state raises register/spill pressure; a small source-level arithmetic reduction
alone would not justify promotion.

Five matched gameplay cohorts provide the deciding evidence:3,550 calls,
592 sampled states and21 indexed/palette/world captures match the prior
coverage-narrow release, including final context and geometry counters. No
faults or geometry-capacity loss. Incremental world/raster instruction savings:
accepted route0.832/0.971%, Deep Dark0.640/0.790%. Every full six-call world
window improves:route0.305..1.269%, DD0.237..0.920%. The busiest full windows
improve0.951/0.761%, respectively. Startup, two warmup calls and final pause are
excluded; emulator identity and exact predecessor reports are bound by hashes
in reused-references.json. These are not hardware frame-time measurements.

ITCM grows152B to42,736B. RAM code/data77,636B (-4), BSS602,716B (+4), net
RAM payload unchanged. Largest startup free spans RAM/ITCM/DTCM/AHB:
32,768/22,800/22,528/21,036B. Own-function CFAs general perspective424B (+40),
opaque perspective392B (+32), general affine256B (+24), opaque affine224B
(unchanged). These are not full call-chain or physical stack-highwater values.
The five hot functions/72B span table remain ITCM, span helpers inline, release
controls enabled and diagnostic raster counters absent.

Normal WASM bundle regenerated: SMG.bin122,581B, code XIP1,988,292B;
assets32,750,308B retain SHA82652fa86203fa6b24b03d595a54f8deda428898d0d825f558e12afe0731c89e.
Root/ELF/bundle bytes and loader placement were verified. No converter or
firmware rebuild. Seven owned headless VMs and two private services stopped;
697,289,664 logical scratch bytes removed. The user's visible VM/profile and
hardware were not accessed or updated. Hardware60FPS remains unverified.
Evidence: build/perf/2026-10-25-coverage-rows/.

Next hypothesis: the hot shader still accommodates both wide and narrow row
state, contributing to spill pressure. Outlining the rare wide-coverage path
into cold XIP code could allow smaller hot loops while preserving extreme
geometry coverage. It needs careful comparison against the current release:
a duplicated setup on rare fallback calls, extra code and changed register
allocation may outweigh the savings. Another candidate is specializing the
ordinary opaque perspective path by depth mode, subject to remaining ITCM
capacity. Neither hypothesis is implemented or measured here.


## 2026-10-25: opaque perspective depth specialization

Promoted specialization of ordinary opaque perspective rendering by depth mode.
The shader template now accepts a constant depth policy:-1dynamic,0off,1on.
The dispatcher already verifies a depth pointer and all three values before
selecting perspective, then snapshots g_depth_active once to select the opaque
on/off shader. Other materials and affine shaders retain dynamic depth tests.
This removes depth-control branches from the dominant pixel loop without
changing coverage, interpolation, quantization, transparency or depth ordering.
No new game setting or runtime data buffer. Existing g_opaque_raster_enabled
still permits comparison with the general shader.

Updated the shared native scene reference caller to pass dynamic policy-1;
otherwise its test behavior remains unchanged. Docker ASan/UBSan checks pass:
200,000 independent scanline intervals and200,000 per-row guard/accumulator
cases per resolution,2,500 four-triangle scenes at160x120/320x240 against the
wide/dynamic reference, all pixels/depth exact across switches, materials,
invalid/missing depths, extreme coordinates and fallback cases. Original GBA
mip/axis/cone contracts,65,536 water pairs/shared-edge ownership and captured
wall/ground/deferred-effect depth regressions pass.

Eight actual ARM kernel inputs match predecessor canonical inputs, initial
buffers and all outputs in every perspective/depth/control case. Reference
samples select coverage on and opaque on; candidate control toggles opaque
specialization, so its disabled counts are the general path, not wide coverage.
Opaque perspective+depth saves1.484..8.507%; across all cases savings0..20.054%,
with no regression. Prior reference ELF/report/emulator hashes are recorded.

Five gameplay cohorts at the established30Hz API cadence match3,550 calls,
592 sampled states and21 indexed/palette/world captures against coverage-rows.
Final player/camera/context and geometry counters match; no faults or capacity
loss. Incremental world/raster instruction savings:accepted route4.073/4.763%,
Deep Dark3.354/4.150%. Every full six-call world window improves:
route2.982..4.940%, DD3.199..3.792%; busiest full windows3.707/3.199%.
Startup, two warmup calls and final pause are excluded. These are instruction
counts; compositor/scanout and physical M7 cache/FPU timings are not measured.

ITCM46,552B (+3,816) contains six hot raster functions and the72B span table;
span helpers inline. RAM code/data77,636B and BSS602,716B remain unchanged.
Largest startup free spans RAM/ITCM/DTCM/AHB:32,768/18,984/22,528/21,036B.
Own-function CFAs opaque perspective depth-on384B and depth-off360B, smaller
than the former dynamic opaque392B. General perspective424B, general affine
256B and opaque affine224B remain unchanged. These are not physical stack
highwater or whole-call-chain bounds.

Normal WASM bundle regenerated:SMG.bin126,397B, code XIP1,988,292B;
assets32,750,308B retain SHA82652fa86203fa6b24b03d595a54f8deda428898d0d825f558e12afe0731c89e.
Root/ELF/bundle identities, specialized symbol/default controls and actual
loaded ITCM were checked. No converter or firmware rebuild. Seven owned
headless VMs and two private services stopped;698,094,768 logical scratch
bytes removed. Visible review VM/profile and hardware were untouched.
Hardware60FPS remains unverified. Evidence:build/perf/2026-10-25-depth-specialized/.

Next verify the target cadence before another optimization: replay the same
accepted timeline at60Hz API boundaries, independently for both immediately
preceding coverage-rows and current builds. Keep30Hz reports as separate
cohorts; do not compare their states/images with60Hz and call them identical.
Suggested60Hz jobs:full route2,407calls and Deep Dark360calls (six seconds),
with matching sampled states, captures, geometry and guest instruction stages.
The --fps option controls injected API delta/input cadence, not Retro-Go's
native scanout/audio pacing; document the distinction. Preserve immutable
baseline profiles/emulator identity and exclude startup/two warmups/final pause.
Physical hardware measurements remain necessary once access is granted.


## 2026-10-25: paired60Hz API-cadence verification

Completed the target-cadence follow-up without changing or rebuilding runtime,
assets, converter modules, bundle or firmware. Normally installed archived
coverage-rows and depth-specialized ELF/GWHB images through GWProv into a
private disposable copy of the accepted recording's preserved baseline profile.
Each build ran Deep Dark360 calls and the full accepted route2,407 calls with
--fps60 and guest-internal instruction stages. Four new headless VM runs total
5,534 game API calls; candidate totals2,767. Timeline, ELF/bin/report hashes,
preserved baseline image hashes and live emulator identities bind the evidence.

The60Hz candidate matches462 sampled player/camera states, final context,
complete geometry counter totals, final indexed output and all nine captured
indexed/palette/world views. No faults or geometry-capacity loss. Every full
six-call window (nominal0.1s at60Hz) improves. Incremental world/raster savings
against coverage-rows:route4.260/4.902%, Deep Dark3.356/4.150%. Busiest full world
windows improve3.607/3.031%. Window ranges:route3.197..5.245%, DD3.031..4.494%.
The source/bundle is the same already promoted depth-specialization release;
these percentages validate it at60Hz, not a second cumulative optimization.

Fixed tools/analyze_instruction_stages.py's opening endpoint:180 calls was
six seconds only at30Hz. It now uses6*conversion.fps; with60Hz input the opening
ends at360, excluding the first two warmup calls as before. Added cadence,
window-call count and nominal window duration to the report. Six-call windows
remain unchanged:they represent5Hz at30fps and10Hz at60fps. Reanalysis of the
existing30Hz Deep Dark pair proves active/opening/window values unchanged.
The tool's six-call windows should never be labeled10Hz for30Hz cohorts.

Added replay metadata recording injected API cadence separately from the
configured firmware target. All four runs report injected60Hz, configured
firmware30FPS, native_pacing_measured=false. The driver forces rendering and
controls API input/delta; the preserved firmware's audio-DMA wait is bypassed
outside measured calls in this CPU-stage cohort. This tests60Hz simulation/
render workloads while excluding native scanout/audio pacing. It does not prove
the complete firmware loop sustains60FPS. Source audit confirms smg_target_fps
controls the outer SDK pacing/audio buffer/frame-delta selection; the game API
receives its explicit delta. Do not combine30Hz and60Hz state/image cohorts or
compare host runtime as FPS. Startup, two warmups and final pause are excluded.

All four owned VMs and one private service stopped;403,618,200 logical scratch
bytes removed after verifying cleanup. Source engine, root runtime/assets and
standard bundle hashes remain identical to depth-specialized/bundle.json.
The visible review VM/profile and hardware were untouched. No Docker/native
build was needed for this Python tooling/verification round; supplied game
images are the prior Docker-built artifacts. Evidence:
build/perf/2026-10-25-cadence60/{summary.json,images.json,deepdark-work.json,route-work.json}.
Hardware sustained60FPS, native pacing and physical stack highwater remain
unverified. Continue performance work using60Hz paired cohorts as well as the
established30Hz regression set; physical-device measurements still require
explicit hardware access.


## 2026-10-25: Garden doorway/spire trap

User entered through the Garden doorway, reached a position by the central
spire, and reported absent walls. Captured the exact running VM without moving
Mario, then restored its run state. The user subsequently closed the VM;
verified process295208 absent, stopped its private daemon295179, retained the
profile/capture, and disabled automatic reopening. No hardware access.

Capture: Observatory0/scenario0, player(310,177,6248), grounded and wall-pushing,
with flat support and active wall contact. No geometry/point/visibility/arena/
effect capacity loss occurred in the captured run. Independent full-OBJ
projection fills the view with nearby spire/glossy and dome_garden/glossy_garden
faces; these surfaces exist in the authored mesh. This was not evidence that
asset files failed to load. The camera was constrained by surrounding geometry.
The captured camera is only374 units from its target, versus a900-unit desired
distance: the camera follow ray toward its desired position intersects an
authored Garden shell face at y=461. This explains the cramped view. The dome
mesh is the Observatory hub's exterior gateway shell, not an interior room;
walking through its visual doorway enters geometry the port does not model as a
playable room. The incomplete-looking walls therefore do not point to a missing
asset or render-capacity failure.
The exact path before capture was initially unknown; the user confirmed entry
through the Garden doorway. This is consistent with the reproduced trigger miss
and the menu fix below.

Garden actor/object36 is an active OBJ_DOME, args[1]=6, centre(192,19,6164),
radius170. Its generic spherical test uses Mario's body centre (+70 above feet)
and adds body radius60. At the captured point, distance250.208 exceeds230.
At the doorway standing body centre(548,0,6318), distance388.346 also exceeds230.
The authored doorway jambs are nearx547.8,y+/-158.7,z6243..6532.3. The recipe
repurposed an original AstroChangeStageCube centre as a fixed-radius dome
sphere, omitting its nonunit cube scale/shape. Coordinate transforms/parsing
are consistent; original Wii cube containment remains unverified. This is a
local correction to the existing homebrew trigger approximation.

Garden has no implemented room transition: only the Terrace(args[1]==0) uses
the AstroDome room path. Other dome touches fade and call galaxy_select_open.
The missing Garden menu activation let Mario enter the exterior shell and
reach the spire instead. Corrected the Garden trigger to(548,0,6318), keeping
radius/front/args/scenario and all mesh/collision data unchanged. It activates
on approach; it intentionally does not force a menu from the already-stuck
captured position. No Garden interior geometry was added.

The shared recipe correction changes row39 (runtime object36):
position text0.7521 0.0749 24.0801 ->2.1407 0.0000 24.6797. Patched both
observatory.stage/native (Rust/WASM literal path) and three residuals in
observatory.stage/ln.res (Python native line-delta path); patching only/native
would break clean native/WASM parity. Helper:
tools/asset_construction/repair_observatory_garden_entry.py, ledger:
asset_construction/stages/observatory-garden-entry-fix.json. It requires the
exact pre-fix pack and writes a candidate separately. Preserved source pack:
this round's reference-observatory.pak. Docker reconstruction proves every
other constructed Observatory output unchanged. An initial helper assertion
counted splitlines incorrectly; fixed it to preserve the trailing empty line.

Real packaged old/new GWProv headless tests start outside the doorway at
(900,0,6248), use60Hz game deltas and walk inward. Old image remains in PLAY,
against the spire atx403 after150 calls. Corrected image opens GALAXY_SELECT
at call55, x537, before becoming trapped. Actual dome-think probes verify
positive doorway activation and negative roof/side/captured-point cases.
Both VM runs have zero CPU faults; scene images and full state paths retained.
This is a controlled approach reproduction, not the user's missing timeline.

A full clean native reconstruction and asset compilation used the existing
smg-runtime-tests Docker image; plain SDK image lacked host cc and its failed
attempt log is retained. ARM SDK build used Docker. Complete asset-byte audit
shows only object36 position and its automatically copied default target change:
offsets1,128,224 and1,128,248, eight byte differences total. Initial audit allowed
only position and was corrected after checking StageObject.target initialization.
SMG.bin, .core_entry, .core_itcm and code XIP are identical to the validated
release. Asset size stays32,750,308B; new SHA-256:
afd1871c93b3fb7eb04a11e153ad368c4438010460cecee3a49da40eed8f4b86.
Code/data77,636B+BSS602,716B and ITCM46,552B are unchanged.

Accepted60Hz recording regression:2,407 calls,402 sampled states, final context,
geometry totals, final indexed output and five indexed/palette/world captures
match depth-specialized. No faults/capacity loss. This correctness change does
not claim a speedup. Browser full ISO conversion with128MiB ranges:Chromium
117.45s, Firefox114.65s; both outputs match native byte-for-byte. Production
GWProv conversion with the existing explicit10T fuel budget also matches all
three staged artifacts. Rebuilt converter modules and updated the standard
WASM bundle with matching exact relocation tables; no GWRG/processor ABI or
output budget change. No firmware build or hardware operation.

All three regression VMs and two private services stopped. Browser server/
containers and GWProv staging exited. Removed901,461,280 logical scratch bytes;
kept durable evidence, source ledgers and the user's closed profile. Current
bundle/runtime identities are bound in this round's bundle.json. Do not
reopen a review window until asked. Mario's face mesh remains unchanged; the
previous smaller-eye texture bake remains in the same asset output.

### Other dome entrance acceptance gaps

A read-only source/mesh audit finds the same cube-to-sphere approximation in the
five remaining domes. Their predicted minimum distances from current centres
to standing passage body centres exceed the230-unit touch radius:
Terrace313.63, Fountain282.84, Kitchen461.39, Bedroom296.30, Engine277.95.
Doorways were identified from coplanar vertical jambs, height289.28, middle
aperture317.44; body centres inset60 from jambs. These are bounded geometry
predictions, not confirmed runtime failures or proof of global unreachability.
No other trigger positions were changed. Add actual headless doorway-walking
checks, Kitchen first, then Terrace/Bedroom and Fountain/Engine, before applying
further metadata corrections. Preserve the distinction between the implemented
Terrace room and generic galaxy menus for the others.

Kitchen follow-up: the current headless build places runtime dome actor33 at
`(6830,-2696,2115)`. Calling its trigger at the authored aperture, with Mario's
feet at `(6383,-2708,2160)`, leaves the trigger state clear; the body-centre
distance is461.71, over the230-unit touch range. A first continuous approach
from `(5700,-2708,2160)` was not a valid doorway traversal: Mario fell from the
elevated ledge to z=825 before reaching the aperture. It remained in PLAY and
had no CPU faults or geometry capacity losses. This confirms the mismatch at
the aperture but does not establish that a normal route reaches that spot or
justify changing the trigger. Headless evidence is in
`build/perf/2026-10-25-dome-doorways/kitchen-runtime.json`; the owned VM and
private GWProv daemon stopped, and `tmp/kitchen-doorway-vm` was removed.

### REA direction

REA skill42 was applied using the installed6.5.0 CLI because registered MCP
tools were absent from this client. Existing Hopper import changed code bytes
at retained debug-relocation offsets; its original dossier was rejected.
Docker-produced stripped analysis ELF preserves all six loadable sections;
REA read-bytes then verifies all41,392 prototype ITCM bytes exactly. A usable
function dossier records seven static F64 divide sites and substantial SP
traffic; static sites are not dynamic counts or physical latency. Full Evidence
IDs, derivation, verification and limits are in rea-audit/findings.md. No REA
configuration or engine installation. This analysis-tool import issue did not
alter runtime files: the user VM's loaded ITCM matched its validated ELF.

Current60Hz reports still weight raster at86.3%/80.2% of world instructions
(route/Deep Dark). Older diagnostics count~9.6k/14.9k reciprocal evaluations per
call in different workloads. Prioritize measured compiled span/division/
conversion/spill hypotheses over more tiny setup edits; physical M7 calibration
is still required. The direct range-guard trial was not promoted after tiny/
mixed kernel gains. The outlined-wide prototype remains archived/unpromoted,
with fallback frequency, ARM behavior and gameplay measurement outstanding.

### 2026-10-25: promote bounded 16-pixel perspective spans

Promoted the span cap from8 to16. The rational endpoint test still halves
spans when needed to preserve the same1/16-source-texel error bound; coverage,
depth, material, ordering and geometry are unchanged. The reciprocal-step table
is136 bytes in ITCM (+64 bytes over the former9-entry table). The build-time
`SMG_PERSPECTIVE_SPAN_MAX=8` define remains available for A/B builds.

Docker ASan/UBSan fixtures pass at160x120 and320x240:64.4M/128.4M rational UV
samples, exact coverage and worst errors0.999495/0.999454 model UV units.
Long-double audits of eight captured route and Deep Dark views peak at0.995154.
Palette bytes are unchanged. Because nearest sampling can cross a texel boundary,
the rendered images are not pixel-identical: the largest captured frame changes
68 of76,800 indexed bytes, with at most272 RGB output pixels changed.

The performance control was rebuilt from the same source with `-O3
-fno-strict-aliasing` and span8. Both artifacts were installed through GWProv
into copies of the preserved baseline profile and replayed in headless GWemu:
the full accepted route at60Hz (2,407 calls) and Deep Dark stage7 (360 calls).
All402 route and60 Deep Dark boundary states, final contexts and geometry
counters match. Both report zero CPU faults, arena drops, visibility/point
capacity hits, far-pass skips and depth-effect drops. There are no changes to
asset XIP bytes or palette values.

Across the accepted route, world instructions fall3.51% and raster instructions
4.06%; in Deep Dark they fall1.10% and1.37%. The route's first six seconds
improve3.08% in world instructions. Two of400 complete six-call route windows
regress (worst0.76% world and0.99% raster); all59 complete Deep Dark windows
improve, by at least0.27% world and0.35% raster. Rendering is forced and frame
work is measured as guest instructions; these numbers do not measure native
firmware pacing, M7 cycles or hardware FPS.

Nearest-sample differences remain bounded and sparse. In the route's captured
views, the maximum is68/76,800 indexed pixels and245/76,800 RGB pixels; the
largest Deep Dark capture changes42 indexed and156 RGB pixels. Palette data is
identical. I reviewed paired native160x120 views: no lost/displaced geometry or
perceptible scene change. RAM code/data is78,388B, BSS602,716B, ITCM46,616B;
startup free spans are RAM_EMU32,784B, ITCM18,920B, DTCM22,528B and AHB21,036B.
The32-pixel cap passed the same Docker coverage and rational-UV oracle at
160x120 and320x240 (maximum error remains0.999495/0.999454 model UV units).
It was built with a264-byte ITCM step table (+128B over span16), installed as a
static test bundle through GWProv, and replayed headlessly on the full accepted
60Hz route and Deep Dark stage7. All402 route and60 Deep Dark sampled states,
final contexts and geometry counters match span16; both builds have zero faults
and capacity losses. Captured palettes match. Active-route captures change at
most48/76,800 indexed pixels; Deep Dark changes at most29. The final Deep Dark
image matches, while the route image does not.

After excluding the two warmup calls and final route pause, span32 reduces
average guest world/raster instructions by1.33%/1.54% on the route and
0.34%/0.42% in Deep Dark. Only376/400 route windows and56/59 Deep Dark windows
improve; worst regressions are0.88%/1.19% on the route. This small, mixed result
does not justify the extra sampling differences, so span32 was rejected and
removed. The production cap remains16. Reports, static bundle, candidate ELF,
images and restore build are archived in `build/perf/2026-10-25-span32/`. No
hardware timing or FPS claim is made.

This build audit also found the project default `OPT ?= -O3 ...` was assigned
after including the SDK, which had already set `OPT=-Os`. The project now sets
the default before the SDK include and forwards it through the Docker target.
The validated release is therefore explicitly built at `-O3`; changing build
flags still requires a clean SDK build because Make does not track flag changes.

Production artifacts: `SMG.bin` SHA-256
`73b72df71472c7a24a5dd1b182190357540c7dbd6d6c2130551cd2db0293714b`, code XIP
`e6cfa3352f9271a1140a43a34a3f2aca2c13c94ca5f458f9fd5efce75bbb84d6`, and
byte-identical converted asset XIP
`afd1871c93b3fb7eb04a11e153ad368c4438010460cecee3a49da40eed8f4b86`. The
release bundle is under `retro-go-sd/build/smg/local-bundle/`; same-source
builds, bundle inputs, per-frame reports and comparisons are archived in
`build/perf/2026-10-25-span16-production/`. All private GWemu instances and the
GWProv daemon stopped; `tmp/` scratch was removed. No visible instance or
hardware was used. Sustained physical60FPS remains unverified.

### 2026-10-25: perspective precheck on/off follow-up

Measured the existing optional reciprocal precheck on the production span16
binary, using the same60Hz route and Deep Dark headless GWProv/GWemu cohorts.
The ELF, recorded boundary states, final contexts, geometry counters and
captured images match exactly; there are no faults or capacity losses. The
route improves1.24% in world instructions and1.45% in raster instructions
(1.61%/1.89% over its opening); all400 six-call windows improve. Deep Dark
regresses0.051% in world and0.064% in raster instructions overall, with22 of59
windows regressing (worst1.12%/1.33%). All sampled indexed and RGB captures are
pixel-identical, including palettes. This mixed result does not justify
enabling the precheck by default; it remains opt-in. Full reports and paired
images are in `build/perf/2026-10-25-span16-precheck/summary.json` and its
captures. Headless instances/services stopped and this run's `tmp/` scratch was
removed. No visible VM or hardware was used.

### 2026-10-25: endpoint reuse instruction-stage comparison

Added `--perspective-reuse on|off` to `tools/replay_game_frames.py` and used it
to compare exact span-endpoint reuse on the current production binary. Replayed
the accepted route (2,407 calls) and Deep Dark stage7 (360 calls) at injected
60Hz in disposable baseline-profile copies. All402 route and60 Deep Dark sampled
states, final contexts and geometry counters match; there are no faults or
capacity losses. All 400 route and59 Deep Dark six-call windows use fewer guest
instructions with reuse disabled. Active world/raster work falls4.58%/5.33% on
the route and2.82%/3.52% in Deep Dark. These are emulator guest-work counts,
not M7 cycles or whole-frame FPS.

The change is visually sparse but not pixel-identical: among the captured route
frames, at most38/76,800 indexed pixels and138 RGB pixels differ; Deep Dark's
maximum is29 indexed and113 RGB pixels. Palettes match, and the paired frame
review shows no geometry displacement. The rational-span fixture previously
checked both reuse modes against its1/16-source-texel error bound. Reuse remains
enabled in production pending hardware timing: disabling it computes more span
start reciprocals, so emulator instruction savings alone do not establish an M7
win. Reports, captures and the replay command are in
`build/perf/2026-10-25-span16-endpoint-reuse/`. Owned headless VMs/private
GWProv daemon stopped and this run's project-local scratch was removed. No
visible VM or hardware was used.

### 2026-10-25: accepted-route raster setup reciprocal comparison

Used the new `--raster-setup-recip on|off` replay override to compare the exact
shared Q63 reciprocal against signed setup division on the same production ELF.
GWProv installed each run into a separate disposable baseline-profile copy;
the accepted route ran2,407 calls and Deep Dark stage7 ran360 at injected60Hz.
All402 route and60 Deep Dark sampled states, final contexts, geometry counters,
and every captured indexed/RGB pixel and palette match. Both runs report zero
faults, arena drops and geometry capacity losses.

The shared reciprocal reduces active world/raster guest work by0.75%/0.88% on
the route and1.13%/1.40% in Deep Dark. All400 complete route and59 Deep Dark
six-call windows improve; the smallest raster reductions are0.345% and0.769%.
This confirms the production-enabled implementation across the accepted route
and expensive opening, beyond the previous single stationary VM pair. It adds
no buffers or asset changes. This remains emulator guest-work evidence, not M7
cycles, whole-frame timing or a sustained60FPS result. The complete comparisons,
captures, install/replay logs and private-service cleanup record are in
`build/perf/2026-10-25-raster-recip/`. All owned VMs and the private GWProv
daemon stopped, and its project-local scratch was removed. No visible VM or
hardware was used.

### 2026-10-25: reject single-precision perspective span endpoints

Tested single-precision reciprocal/multiply endpoint math with the existing
bounded-span rasterizer. The independent long-double oracle passes at
160x120 and320x240: worst errors are0.999495/1.000337 model UV units against
the existing1.001-unit bound, with exact coverage. A clean Docker SDK rebuild
and standard bundle both pass; the rebuilt `SMG.bin`, code XIP and asset XIP
match the saved production hashes exactly.

The same-ELF headless GWProv/GWemu A/B is a regression. On the accepted route,
world/raster guest work increases0.575%/0.668%; every one of400 six-call windows
regresses. Deep Dark increases0.621%/0.773%, with all59 windows regressing.
Boundary states, final context, geometry counters and fault status match. The
captured images have at most1 indexed pixel (4 RGB pixels) changed; the route's
final indexed image matches. This is guest instruction evidence, not M7 cycles
or FPS. Removed the runtime switch and diagnostic implementation; production
continues using double-precision endpoint math. Evidence is retained in
`build/perf/2026-10-25-perspective-f32/summary.json` and its paired captures.
The rational-span fixture passes after cleanup. No visible VM or hardware was
used; the failed candidate is not in the rebuilt package.

### 2026-10-25: reject reciprocal-only span reuse

Tested carrying the previous endpoint's reciprocal and repeating the UV
multiplies at the next span, to reduce output drift while still avoiding a
division. The rational-span oracle and coverage checks pass at160x120 and
320x240, but the candidate is slower in headless GWProv/GWemu. Against
recomputation on the same ELF, accepted-route world/raster guest work rises
4.78%/5.61%; all400 six-call windows regress. Deep Dark rises2.82%/3.55%; all59
windows regress. Sampled boundary states, final contexts and geometry counters
match with zero faults. Captures differ by up to38 indexed/138 RGB pixels on
the route and29 indexed/113 RGB pixels in Deep Dark. Thus it is both slower and
not pixel-identical. The production cache remains the saved-UV version. The
candidate was removed and the Docker build and standard local bundle were
restored to the accepted `SMG.bin`, code XIP and asset XIP hashes. Reports,
offline test bundle, captures, and cleanup record are in
`build/perf/2026-10-25-span16-recip-cache/`. No visible VM or hardware was used.

### 2026-10-25: signed16 model-UV range contract

The compiler's current face-wide UV normalization was checked against every
triangulated near and far stage OBJ, rather than only synthetic examples. The
Docker ASan/UBSan harness covers130,511 triangles: the old independent clamp
would saturate1,269 vertex-axis values;365 faces now shift by whole texture
periods. For every face and axis, the three quantized UV gradients remain exact,
all offsets are multiples of512, and every stored coordinate retains512 units
of signed16 headroom. A separate100,000-face randomized contract passes too.
`tools/test_material_repairs.py` runs both checks alongside its alpha/material
contracts.

This proves the compiler's fixed-point mapping preserves each face's local
texture repeat before serialization. It does not prove all stage textures look
correct in every runtime view; perspective approximation, mip selection,
authored UVs and level of detail remain separate visual factors. No runtime or
hardware performance claim follows from this asset-format check.

### 2026-10-25: process perspective raster spans as runs

Restructured the textured pixel loop so perspective spans are generated once,
then consumed by an inner bounded run loop. The old path tested span exhaustion
and decremented its remaining-pixel counter for every fragment. Endpoint math,
span size/error guard, UV increments, depth tests, coverage and material rules
are unchanged. Affine rendering retains its direct pixel loop. The raster-work
fixture was also corrected to derive expected counts from the current
16-pixel production cap rather than the retired8-pixel cap.

Docker ASan/UBSan tests pass:24,000 raster triangles at each resolution across
materials, winding, clipping, UV wrapping, mips and degenerates;64.4M/128.4M
rational UV samples with maximum error0.999495/0.999454 and exact coverage;
600 depth scenes per resolution; and2,000 independent material/depth/work
counter cases per resolution. The work counters match the independently
enumerated spans and divisions.

The Docker-built ARM candidate was installed by GWProv into a disposable
baseline-profile copy and replayed in headless GWemu. Against the validated
production span16 build, the accepted route matches all402 sampled states and
final context/image; Deep Dark matches all60 states and final context/image.
Every captured indexed image and palette is byte-identical. Geometry counters
and capacity limits match, with zero faults, arena drops, point/visibility
capacity hits, far-pass skips or deferred-effect drops.

Across the accepted route, guest world/raster instructions fall7.21%/8.39%;
all400 six-call windows improve, by at least4.29%/5.77%. In Deep Dark, they fall
4.78%/5.97%; all59 windows improve, by at least3.79%/4.96%. These are controlled
GWemu guest instruction counts, not M7 cycles or physical frame times.

The change adds approximately888B of ITCM code (47,504B total); the largest
available startup spans are RAM_EMU32,776B, ITCM18,032B, DTCM22,528B and
AHB21,036B. RAM_EMU code/data is78,396B and BSS602,716B. The six hot raster
functions and136B step table pass the layout checker. The standard local bundle
was regenerated with candidate SMG.bin SHA-256
`9b86e3202f55159d3ae81f6acbfd704c1f440f62ffe4c87e65185d93a7701154`, code XIP
`c669e8c20d96c05ba7ef34f6ec3b254c7431ff227ae549e8e48fa5db1dc113e2`, and
unchanged assets XIP `afd1871c93b3fb7eb04a11e153ad368c4438010460cecee3a49da40eed8f4b86`.
Reports, images, candidate ELF and exact bundle inputs are in
`build/perf/2026-10-25-raster-span-loop/`. All owned headless VMs and the
private GWProv daemon stopped; this run's project-local scratch was removed.
No visible VM, hardware or firmware was accessed. Sustained hardware60FPS and
physical M7 timing remain unverified.

### 2026-10-25: reject four-way perspective-pixel unrolling

Inspected the generated ARM loop after the span-run optimization. Framebuffer
and depth-row pointers are already formed outside the per-pixel loop and advance
by1/2 bytes per sample, so no repeated row-address multiply remained to remove.
A `#pragma GCC unroll 4` on the bounded span pixel loop preserved the raster and
depth fixture outputs and produced byte-identical accepted-route and Deep Dark
images, palettes, states, contexts and geometry counters. There were zero faults
or capacity losses.

Despite exact output, the candidate regressed guest raster/world work on every
measured window: route3.47%/2.94% and Deep Dark3.95%/3.12%. ITCM grew a further
680B to48,184B. The pragma was removed; the span-run candidate was rebuilt with
Docker and restored exactly (SMG.bin SHA-256
`9b86e3202f55159d3ae81f6acbfd704c1f440f62ffe4c87e65185d93a7701154`, code XIP
`c669e8c20d96c05ba7ef34f6ec3b254c7431ff227ae549e8e48fa5db1dc113e2`). Reports,
candidate ELF, images and the reject comparison are in
`build/perf/2026-10-25-raster-unroll4/`. Private headless GWemu instances stopped,
their daemon stopped and project-local scratch removed. No hardware or visible
VM was used.

### 2026-10-25: span32 after the span-run raster change

The earlier span32 experiment predated the bounded-run pixel loop, so cap32 was
remeasured against the current span16 build with the same 60Hz accepted route
and Deep Dark cohorts. The cap32 build preserves all402 route and60 Deep Dark
sampled states, final contexts, and geometry counters. It has zero faults or
capacity losses. Deep Dark's final indexed image is identical. On the route,
captures differ in only29–60 of76,800 indexed pixels (same palette); final image
differs sparsely as well. This is the small perspective approximation tradeoff
for fewer span evaluations.

Route guest world/raster instruction work falls1.82%/2.15%. Nineteen of400
six-call windows regress, concentrated in late low-work frames; worst regressions
are0.86% world and1.20% raster. The busier windows improve. Deep Dark work falls
0.49%/0.61%, and all59 windows improve (minimum0.037%/0.049%). The small gains
are not M7 cycle or hardware FPS measurements.

Docker ASan/UBSan raster, perspective-oracle, work-counter and depth fixtures
pass for both160×120 and320×240. The layout checker confirms the hot raster
functions remain in ITCM; the span-step table is264B. RAM_EMU code/data remains
78,396B, BSS602,716B; ITCM code is47,632B and the largest startup ITCM span is
17,904B. The standard local bundle was regenerated with SMG.bin SHA-256
`cd420cef04b07d4418d92834710ca09c6dc0ddb239c751916c60a2fcfab53d1a`, code XIP
`c669e8c20d96c05ba7ef34f6ec3b254c7431ff227ae549e8e48fa5db1dc113e2`, and assets
XIP `afd1871c93b3fb7eb04a11e153ad368c4438010460cecee3a49da40eed8f4b86`.
The local bundle SHA-256 is
`f195e40cbaf3bd47b15d84e36c6bb585707469c25993e461221f59c501f3db69`.
Headless GWProv/GWemu and detailed comparisons are retained in
`build/perf/2026-10-25-span32-after-runloop/`. Owned VMs and the private service
are stopped; no hardware or visible VM was used.

### 2026-10-25: reject span64 after the span-run raster change

Tested a64-pixel cap against the current32-pixel build on the same60Hz accepted
route and Deep Dark cohorts. Docker's independent rational UV oracle stays under
1 model-UV unit at both160×120 and320×240 with exact pixel coverage. The2,000
case work-counter fixture passes at both resolutions. All402 route and60 Deep
Dark sampled states, contexts and geometry counters match, with no faults or
capacity drops. Deep Dark's final image is identical; route captures differ by
at most57 indexed pixels (154 RGB pixels) and the route final image differs.

The added gain over cap32 is small: route world/raster instructions fall a
further0.57%/0.67%; Deep Dark falls0.12%/0.16%. Forty-six of400 six-call route
windows regress (worst0.64% world,0.85% raster), while all Deep Dark windows
improve only0.02%–0.31%. Rejected: cap64's slight average gain does not justify
more timing regressions and additional pixel changes. The source cap and
step-table layout were restored to32, then the homebrew was rebuilt with Docker
and the standard local bundle regenerated. Final `SMG.bin`, code XIP and asset
XIP match the cap32 artifacts; ITCM placement passes with the264B step table.
The regenerated local bundle SHA-256 is
`076af2e4c6ab825dcb8e67adb9ca38459f8f673ab22f36fbe8539817da92e97b`.
Headless reports and the rejected candidate are archived in
`build/perf/2026-10-25-span64/`. Private GWProv service and VM stopped, with its
project-local scratch removed. No visible instance or hardware was used.

### 2026-10-25: place the per-triangle raster dispatcher in ITCM

The compiled `engine_raster.part.0` wrapper selects a specialized shader for
each submitted triangle. It was still in RAM_EMU while all six shader bodies
were in ITCM. Added its linker input section to the measured ITCM placement;
the new wrapper occupies168B there, while linked RAM_EMU code/data falls by
160B. The unchanged SDK free-size query reports RAM_EMU32,776B, ITCM17,744B,
DTCM22,528B and AHB21,036B after startup. BSS remains602,716B.

Docker build and layout checks pass. GWProv installs the candidate normally and
headless GWemu replays the full accepted route and Deep Dark. Every sampled
state/context, geometry counter, indexed/RGB capture and final image is exactly
identical to cap32 with the dispatcher in RAM_EMU; zero CPU faults or capacity
losses. Guest instruction counts are also identical within counter aggregation
noise. This verifies correctness and placement, but does not measure an M7-cycle
or frame-time gain. The candidate is in the current local bundle as a narrowly
scoped ITCM placement optimization; hardware A/B remains required to quantify
its speed effect. Evidence: `build/perf/2026-10-25-itcm-raster-dispatch/`.
Current artifact hashes are `SMG.bin`
`ba43c398ed3db7ebe1acbf765a08ee01c0e15ebc3a2107aa20a6f1fa71586529`, code XIP
`ff4e3ba28acdae6a6a3b8fdb55be8fd916e5f8ed959d37a5e9ed566779e2067d`, unchanged
assets XIP `afd1871c93b3fb7eb04a11e153ad368c4438010460cecee3a49da40eed8f4b86`,
and local bundle
`f0f957e9c0721c4dedd3b8628ce220c667c7e7b40e9cfd6484f5f0fc31abcc19`.

### 2026-10-25: reject the short-span precheck threshold

Tested applying the division-avoiding perspective precheck only to spans at
least 16 pixels wide, with exact endpoint-error checks retained for shorter
spans. The rational oracle stayed within the existing error bound, with exact
coverage at 160×120 and 320×240. However, the controlled 60 Hz route regressed
world/raster guest instructions by 2.73%/3.21%; all 400 six-call windows
regressed. Deep Dark regressed 3.08%/3.91%, with all 59 windows regressing.
Correctness stayed effectively exact (one indexed pixel differed in one route
capture), but the extra short-span work erased any benefit. Rejected; production
source and bundle were restored to the cap32 ITCM-dispatch build. Evidence is
in `build/perf/2026-10-25-precheck-threshold/summary.json`; restored artifact
hashes match the preceding ITCM entry.

### 2026-10-25: spire view was an unintended interior, not an asset-load fault

The user confirmed reaching the stuck spire through the Garden dome doorway.
The captured pre-fix state is in `build/perf/2026-10-25-spire-invisible/`:
the doorway trigger object was still at `(192,19,6164)`, away from the visible
entrance. A repeatable60Hz headless walking probe stalled at that doorway in
the reference asset; moving the trigger object to `(548,0,6318)` causes the
same approach to open the galaxy menu, while roof and side probes do not
activate it. The asset edit changes eight bytes and leaves the code XIP and
homebrew binary unchanged. GWProv installation and headless replay complete
without CPU faults.

The captured interior-looking view is not evidence of missing streamed assets:
an independent projection of the Observatory source mesh at the captured
camera pose attributes all 19,200 world pixels to eight nearby faces from
`spire` and `dome_garden` (one `spire` face covers 9,245 pixels). Runtime had
41 draw entries. The camera is inside the dome and looking across those opaque
surfaces, so the rest of the stage is occluded. The practical root cause is
the misplaced doorway trigger allowing traversal into this unintended view;
there is no confirmed LOD or XIP loading failure in this capture. The candidate
bundle is rebuilt with the trigger correction, but the user has not yet
reviewed it. The user closed review VM295208; do not reopen it automatically.

### 2026-10-25: keep the depth-bin RAM mirror experimental

Built a compile-time `SMG_ZBIN_RAM=1` variant that copies the 2 KiB
`lut_zbin` table from mapped XIP to RAM_EMU after XIP mapping. It preserves the
same indexed lookup on the hot face-emission path and avoids changing data
format or runtime behavior. Compared it against `SMG_ZBIN_RAM=0` using the
accepted 60 Hz player route and a 360-frame Deep Dark cohort through isolated
GWProv installs and headless GWemu.

All 402 route and 60 Deep Dark boundary samples, final contexts, geometry
counters and final indexed images match; there are no CPU faults or capacity
drops. Guest instruction-stage averages are effectively identical: on the
route, world/raster are 2,371,142/2,004,418 instructions per API call for both
builds; on Deep Dark they are 4,480,992/3,535,285 versus 4,480,992/3,535,286.
Host replay durations move in opposite directions (115.80s vs 118.17s on the
route; 23.31s vs 22.98s on Deep Dark), so they do not establish a speed gain.
GWemu does not model STM32 XIP bus timing. Keep the feature available for a
hardware A/B, but default it off until device measurements show a benefit.

Evidence and both Docker-built ELF/artifact sets are in
`build/perf/2026-10-25-zbin-ram/`. The RAM copy is present only when explicitly
building with `SMG_ZBIN_RAM=1`; ordinary builds use the XIP table directly.
Both cohort VMs and the private GWProv service were stopped, its project-local
scratch was removed, and no visible VM or hardware was used.

After that comparison, the standard local bundle was regenerated with Docker
using the no-copy default and the already corrected Observatory source pack.
The layout checker passes. Current hashes: `SMG.bin`
`0d88bd511b4ebbbef6cbcba1b9abbf730a4e52f70f58d9498c30b738fbedf5db`, code XIP
`10c0865a1c2218071fa2796160dadc8ac5565513082a44d432e5008b7f427b1a`, assets
XIP `afd1871c93b3fb7eb04a11e153ad368c4438010460cecee3a49da40eed8f4b86`, and
bundle ZIP `524fc7ab408654382e8f7d1235d910d777c7183582ad9123dc3082d178de98cb`.

### 2026-10-25: reject the current-source outlined coverage fallback

Rebuilt the archived wide-coverage outlining idea against the current shader,
with the switch defaulting off. The Docker layout checker passed, but the
candidate places 47,840B in ITCM versus 47,792B for control. Its opaque-depth
shader is only 144B smaller; a 10,518B general fallback lands in code XIP and
adds 10,536B to code XIP overall. This fails to reproduce the old prototype's
claimed 5,160B ITCM reduction, so the extra branch/fallback is not worth a
long gameplay A/B on this build. No runtime speed or fallback-frequency claim
was measured. The candidate source and large intermediate artifacts were
removed, and a Docker rebuild restored the default executable/XIP outputs:
`SMG.bin` `0d88bd511b4ebbbef6cbcba1b9abbf730a4e52f70f58d9498c30b738fbedf5db`,
code XIP `10c0865a1c2218071fa2796160dadc8ac5565513082a44d432e5008b7f427b1a`.
These executable and XIP bytes match the pre-experiment control. The newly
written bundle ZIP SHA-256 is
`4d76f388d17ca742ebdc0d8a79b117fe9fd74d4fe313a22e1edd8417208c314f`.
Build details and discarded candidate hashes are in
`build/perf/2026-10-25-coverage-outline-current/status.json`.

The independent Docker render-contract audit passes both texture mip layouts,
indexed and non-indexed axis/facing rejection, and whole-chunk cone rejection.
It is separate from performance evidence and does not measure the mask
propagation's speed effect. Hardware remains relinquished; physical 60 FPS is
still unverified.

### 2026-10-25: reject a 48-pixel perspective-span cap

Compared a Docker-built 48-pixel span cap with the current32-pixel release. The
first diagnostic build exposed that the compile-time span table only covered
distances through32; that candidate was discarded before use. The corrected
candidate carries49 reciprocal entries (392B in ITCM, +128B) and passed the
independent Docker ASan/UBSan rational perspective oracle at160x120 and320x240:
exact coverage, bounded sampling, and maximum UV error below1 model unit. The
SDK layout checker confirmed the six raster shaders, dispatcher and span table
remain in ITCM.

GWProv installed the candidate into a disposable copy of the accepted baseline
profile, using the base ISO as the required `disc` input and the documented
10-trillion WASM fuel budget. Headless GWemu replayed the 360-call Deep Dark
opening and the full 2,407-call route at60Hz. All60/402 boundary states and
geometry counters match the32-pixel control; both runs have zero CPU faults or
capacity losses. Deep Dark world/raster guest instructions improve only
0.11%/0.14%. The route improves0.39%/0.47%, but52 of400 six-call windows regress,
up to0.73% world and0.96% raster. Captured route images differ by3–48 indexed
pixels out of76,800; palettes match. This is too little performance gain to
justify the extra image variation, so cap32 remains the production default and
the48-pixel source/table/layout-test experiment was removed. No M7 timing or
FPS conclusion follows from these GWemu instruction counts.

Control/candidate reports, captures, ELFs, bundles, summaries and cleanup record
are in `build/perf/2026-10-25-span48/`. GWProv stopped its private daemon and
candidate VM, and `tmp/span48-experiment-gwprov` was removed. The standard Docker
build and local bundle were regenerated with the32-pixel default; production
hashes match the pre-experiment control (`SMG.bin`
`0d88bd511b4ebbbef6cbcba1b9abbf730a4e52f70f58d9498c30b738fbedf5db`, code XIP
`10c0865a1c2218071fa2796160dadc8ac5565513082a44d432e5008b7f427b1a`, assets
XIP `afd1871c93b3fb7eb04a11e153ad368c4438010460cecee3a49da40eed8f4b86`, bundle
ZIP `3f6776f7e744fa3c7d682d14bf7c948a16bcd81bbe78760098b359bb8235ef2a`).

### 2026-10-25: High versus Full scene-detail cost and coverage

Forced scene-detail mode1 (High) and mode0 (Full) through the replay debugger
while running the same current release ELF, accepted 60Hz route and 360-call
Deep Dark opening. All402 route and60 Deep Dark boundary states match; final
player/camera state is unchanged. Neither mode reports CPU faults, arena drops,
visibility/list/point capacity hits, far-pass skips or deferred-effect drops.
Full's accepted-route high-water marks are arena804/3584, visible1075/8192 and
point pool2859/5120; thus the larger source meshes do not hit current pools on
these recordings.

Full increases average guest work over High by11.8% world and10.4% raster on the
route; visibility and main transforms rise15.4% and26.4%. Deep Dark rises9.5%
world and6.9% raster. Full rasterizes1,324,583 route triangles versus936,848
in High, and345,689 versus275,778 in Deep Dark. Yet the captured High/Full world
images differ by at most568/76,800 indexed pixels on the route and177 pixels
in Deep Dark, with identical palettes. These two workloads show Full is
capacity-safe and costs more, but do not show that it fixes the user's distant
galaxy-flight view or every stage. No detail default was changed. The screenshot
comparison and per-frame counters are in
`build/perf/2026-10-25-detail-full/`; GWProv stopped its private daemon/VM and
`tmp/detail-full-gwprov` was removed. No visible instance or hardware was used.

### 2026-10-25: cinematic near-range stress test

Replayed Deep Dark's recorded cinematic entry for960 API calls at60Hz, reaching
the launch-flight descent. The current cinematic High-plus-far view is the
control. A temporary Full view (near=1, no far pass) preserves game state but
overflows during descent:258,820 arena drops,3 point-pool hits, arena peak
3,584/3,584 and point peak5,120/5,120. Its image differs on35,520/76,800
pixels at the flight capture, but is not a valid visual candidate because
geometry was dropped.

Two bounded variants retained the far pass and had no faults or capacity loss:
near=0.875 peaked at arena991/3,584 and points2,172/5,120, with only172 changed
pixels at the flight capture; near=0.95 peaked at1,440 and2,446, with740 changed
pixels. The 0.95 variant rasterized877,306 triangles versus635,637 for High
(+38%). Both returned to the same final image/state. This recording does not
show a useful distant-galaxy detail gain for that extra work; keep High-plus-far
as the cinematic policy. Other stages and actual hardware timing remain outside
this probe.

Reports, screenshots, candidate ELFs and replay scripts are in
`build/perf/2026-10-25-cinematic-full/`. Private GWProv daemons and all
headless VM runs were stopped; no visible VM or hardware was used. The temporary
mode edits were removed, and the standard Docker bundle was rebuilt. Production
`SMG.bin`, code XIP and asset XIP hashes match the control.

### 2026-10-25: Deep Dark intermediate far-mesh prototypes

Tested two isolated spatial-cluster meshes derived from the authored Deep Dark
main OBJ. The runtime asset format and renderer were unchanged. The denser
candidate has1,517 points/3,047 faces; the balanced candidate has729 points/
1,465 faces. The current far mesh has896 points/790 faces. Both fit the existing
far-point limit and retain source material/UV references and stage part tags.
The balanced candidate changes only `stage_deepdark_far.bin` plus generated
texture pools and assembly data; its asset XIP is89,704B larger. Candidate
inputs, compiler outputs, Docker SDK builds, bundles, captures and scripts are
isolated in `build/perf/2026-10-25-deepdark-far-lod1/` and
`build/perf/2026-10-25-deepdark-far-balanced/`.

Headless GWProv/GWemu 960-call Deep Dark fly-in comparisons preserve all160
sampled game/camera states and final context. Both candidates have zero CPU
faults, geometry drops or pool-capacity hits. At call600 the balanced image
differs from the current far mesh on5,126/76,800 indexed pixels; its independent
projection retains more of the authored planet outline. It is visually
promising but grid clustering still changes some cliff/ground shapes, so this
is not accepted as a final LOD repair.

| Variant | Raster triangles | Arena attempts | Arena peak | Point peak | Change vs current |
| --- | ---: | ---: | ---: | ---: | --- |
| Current far mesh | 635,637 | 350,052 | 559 | 1,771 | baseline |
| Dense 3.9x mesh | 954,912 | 669,327 | 1,548 | 2,515 | +50.2% raster work |
| Balanced 1.9x mesh | 724,433 | 438,848 | 802 | 1,727 | +14.0% raster work |

The accepted2,407-call route remains in Observatory: candidate and current
builds have matching402 sampled states, final context/image and geometry
counters. Its emulator timing modes differ, so this is only a correctness
check and says nothing about speed or Deep Dark. The balanced 2,407-call
Deep Dark stage-forced diagnostic also has no capacity loss, but its timing is
not hardware evidence. Do not promote either grid-cluster candidate based on
GWemu counts. A seam/feature-aware simplifier and a representative hardware
measurement are still needed to decide whether denser far geometry is viable.
At the end of the far-mesh experiments, the production build/assets had been
restored byte-for-byte. Those checkpoint hashes were `SMG.bin`
`0d88bd511b4ebbbef6cbcba1b9abbf730a4e52f70f58d9498c30b738fbedf5db`, code XIP
`10c0865a1c2218071fa2796160dadc8ac5565513082a44d432e5008b7f427b1a`, and assets
XIP `afd1871c93b3fb7eb04a11e153ad368c4438010460cecee3a49da40eed8f4b86`.

### 2026-10-25: scene dispatch in ITCM

Moved `draw_scene`, its two bin dispatchers, and `scene_far_pass` into ITCM
alongside the raster/emission hot paths. This adds3,776B to ITCM (51,568B
total, 13,968B remaining under the 64KiB region) and reduces RAM_EMU code/data
by4,296B; BSS remains602,712B. Docker ARM build and raster-layout checks pass.

Headless GWProv/GWemu correctness replays match every sampled game state
(60 Deep Dark, 402 accepted-route), final context, geometry counters and final
indexed image. Both runs have zero CPU faults and no capacity drops. The live
relocated ITCM bytes on each VM match the candidate ELF exactly (51,568 bytes,
zero differences). The cohorts differ by `view_mode_override`, so these runs
are correctness and placement checks, not a performance A/B; host elapsed
times are not FPS evidence. Hardware timing remains unmeasured. Evidence and
the candidate bundle are in `build/perf/2026-10-25-itcm-scene-dispatch/`.
Current packaged hashes are `SMG.bin`
`8ee7fe7589521cb19b8cb28b17214772c42553403d81ee0a26bd6266bfeb2c66`, code
XIP `57084c5af70fb75da637a00eae88d72bd7769c0bf092547e8f10699092615fd2`,
unchanged asset XIP
`afd1871c93b3fb7eb04a11e153ad368c4438010460cecee3a49da40eed8f4b86`, and
local bundle ZIP
`4a158fb5124d52fd788a6304d0fc65cdeaa4853ee72a9f83d1fcf38e9abd26c6`.

### 2026-10-26: current-build Garden doorway regression

Repackaged the current ITCM-scene-dispatch ELF and GWHB binary with the
corrected asset XIP, then installed reference and candidate into disposable
copies of the accepted baseline profile through GWProv. The accepted60Hz
doorway approach still stalls against the shell with the reference trigger;
the current candidate activates at frame54, at x537, and transitions to
GALAXY_SELECT (scene6). The doorway probe triggers; roof, side and captured
stuck-position probes do not. Both VMs report zero CPU faults and zero arena,
visible-list, point-pool, far-pass or deferred-effect losses. The current
candidate's doorway capture shows the entrance geometry intact. No visible VM
or hardware was used. Both owned VMs and the private daemon stopped, and
`tmp/current-doorway-vm/` was removed. This verifies the current runtime's
doorway fix; it is not a user review of all Garden/dome rendering. Evidence,
captures, candidate links, install logs and the replay script are in
`build/perf/2026-10-26-current-doorway/`.

### 2026-10-26: disable perspective endpoint reuse by default

Replayed the accepted60Hz player route (2,407 calls, first2 warmups excluded)
and the360-call Deep Dark opening on the current span32 build, with the same
ELF and disposable accepted-baseline profiles. Endpoint reuse was the only
runtime A/B variable. Reuse-off reduces guest world/raster instructions by
5.95%/7.12% across2,405 active route calls and3.58%/4.58% across358 active
Deep Dark calls. All400 route and59 Deep Dark six-call windows improve with
reuse-off. The replay utility ends before the recording's final pause.

All402 route and60 Deep Dark sampled player/camera states, final contexts and
geometry counters match; both runs have zero faults or geometry-capacity loss.
The images are not byte-identical because float endpoint rounding differs
sparsely: the largest route capture changes35/76,800 indexed pixels (124 RGB),
and the largest Deep Dark capture changes30/76,800 indexed pixels (118 RGB).
Palettes match and visual inspection shows no geometry displacement. The
measured savings justify setting `g_perspective_reuse_enabled` false by default;
the runtime override remains available for A/B.

Docker ASan/UBSan perspective-span tests pass at160x120 and320x240 (maximum
error below1.0 model UV unit, exact coverage); the24,000-case raster regression
and independent original-render-contract audit also pass. Rebuilt the normal
local bundle with the SDK Docker image; raster layout remains valid. GWProv
installed the rebuilt native artifacts into a disposable accepted-baseline
profile, and headless GWemu confirmed the packaged default is off during a
360-frame Deep Dark replay, with zero CPU faults or capacity drops. No firmware
build, visible VM or hardware access. Full paired evidence is in
`build/perf/2026-10-26-perspective-reuse-current/`; default-package replay and
cleanup are in `build/perf/2026-10-26-perspective-reuse-default/`.

Updated production hashes: `SMG.bin`
`0bc3110b861acf060752a24984b1bbd742fb1e3e0867b56a11b2d5ccd86f798d`, core ELF
`a175edccdbffe34ca5311548301b7fdf1ac235ca01e9534b56c8bae65ece7243`, code XIP
`57084c5af70fb75da637a00eae88d72bd7769c0bf092547e8f10699092615fd2`, unchanged
assets XIP `afd1871c93b3fb7eb04a11e153ad368c4438010460cecee3a49da40eed8f4b86`,
and local bundle ZIP
`4c066a060774773d240c7d196bc4da22f5477179f1016b4c535b33137a5bdd81`.

### 2026-10-25: Deep Dark fly-in far-mesh detail audit

The cinematic High mode uses Deep Dark's authored far LOD. Its source OBJ has
808 faces versus17,970 in the main OBJ; the asset compiler emits790 far
triangles. This large source-model reduction explains why the destination
silhouette remains coarse even when the near-mesh fraction rises from0.75 to
0.95. The latter changes only740 pixels in the descent capture while adding
38% raster triangles over the full trace.

The same source OBJ count audit shows this is not unique to Deep Dark:

| Stage | Main faces | Far faces | Far/main |
| --- | ---: | ---: | ---: |
| Beach Bowl | 11,544 | 864 | 7.5% |
| Buoy Base | 12,225 | 790 | 6.5% |
| Deep Dark | 17,970 | 808 | 4.5% |
| Freezeflame | 20,183 | 547 | 2.7% |
| Good Egg | 25,990 | 788 | 3.0% |
| Gusty Garden | 18,359 | 883 | 4.8% |
| Honeyhive | 5,975 | 615 | 10.3% |
| Honeyhive Tower | 7,076 | 755 | 10.7% |
| Observatory | 4,139 | 281 | 6.8% |

These are authored OBJ faces before compilation/culling; they indicate model
complexity, not rendered-frame work.

The far texel path is active:31 of33 Deep Dark far materials retain detailed
mips. A same-ELF GWProv A/B toggling `--far-textures on/off` keeps all160
sampled game states identical, with no faults or geometry-capacity losses.
Turning detail off changes13,436 pixels in the descent capture and7,008–12,528
pixels in later checkpoints. Far textures therefore contribute visible detail;
the remaining coarse silhouette is in the authored far geometry. Disabling
High's small-face and distance-flattening rules with the same near cutoff also
leaves all six checkpoint images pixel-identical, so that shortcut is not the
cause in this view.

The Full mesh remains unsuitable for the whole fly-in because it drops arena
submissions during descent. The next quality experiment should supply an
intermediate far mesh for this stage and measure its silhouette, arena and
point-pool peaks before generalizing LOD generation to other stages. No source
or production asset was changed in these diagnostics. Same-binary far-texture
replay and the temporary renderer candidates are in
`build/perf/2026-10-25-cinematic-full/`; standard Docker release hashes were
restored afterward. All private headless GWProv/GWemu runs stopped and scratch
under `tmp/` was removed.

### 2026-10-26: experimental DTCM depth-bin lookup

Added opt-in `SMG_ZBIN_DTCM=1` to copy the 2 KiB per-face depth-bin LUT into
caller-owned DTCM after the existing texture and source-vertex caches. The
allocator requests it only when another 16 KiB remains, and the runtime keeps
the mapped XIP table as fallback. `smg_visibility_init()` recopies the table
after sleep/wake. The release default remains `SMG_ZBIN_DTCM=0`.

Docker allocation fixture passes2,097,168 capacity/failure combinations while
preserving texture priority and the 16 KiB reserve. A separate Docker C fixture
compiles the production copy/restore code and verifies exact bytes, wake restore,
undersized-buffer rejection and fallback. The opt-in SDK build passes linker,
asset-relocation and raster-layout checks. Its mapped assets are byte-identical
to control; the candidate adds216B code XIP and248B ITCM. The runtime heap
request is2,048B only when it succeeds.

GWProv installed candidate and control packages from independent copies of the
accepted baseline profile. The full 60 Hz accepted route (2,407 calls) and
360-call Deep Dark replay have matching sampled state, final context, geometry
counters and indexed images, with zero faults or capacity loss. GWemu did not
activate the DTCM allocation: the candidate pointer is null and the lookup
remained on XIP. Its allocator stats were not refreshed, so this run cannot
establish the available DTCM span. The candidate's fallback adds roughly0.8%
guest emission instructions; this is overhead in the inactive path, not a DTCM
performance result. Host durations moved in opposite directions and are not FPS
evidence. The replay tool now supports `--audit-zbin-dtcm` to verify either an
exact active copy or an intact XIP fallback.

At the time of the GWemu comparisons, no hardware access was made. Keep the
feature opt-in until a device run shows a repeatable timing gain; the default
bundle remains the unmodified control build. Evidence and both artifact sets
are in `build/perf/2026-10-26-zbin-dtcm/`. The private GWProv daemon was stopped
and its profiles/scratch under `tmp/` were removed.

### 2026-10-26: partial visibility-chunk cache, split main/far banks

The oversized-stage experiment gives main and far chunk tables separate direct-
mapped cache banks so far-table reads cannot evict main-table entries. Docker
ASan/UBSan fixtures cover synthetic ownership/collision cases and differential
leaf lists for all ten main/far stage tables. GWProv/GWemu replayed the same
360-call Freezeflame workload with the feature off/on: all60 sampled states,
final context, geometry counters and indexed framebuffer match; neither run has
faults or geometry-capacity loss.

On this stage the optional cache uses23,040B (512 main plus128 far entries).
The run observed81,748/157,592 main hits (51.9%) and13,745/13,788 far hits
(99.7%). Cache-enabled GWemu host time was34.82s versus31.28s disabled; this is
not a physical timing result, but provides no reason to promote the feature.
The normal build leaves `SMG_PARTIAL_CHUNK_CACHE` disabled. Artifacts are in
`build/perf/2026-10-26-partial-chunk-cache/`; its private daemon and `tmp/`
profiles are stopped/removed, with no hardware access.

#### Hardware follow-up

The user authorized a flash-only test on `DEVICE`. GWProv confirmed the
candidate was running in bank 2. The passive 60-second capture sampled the
current Freezeflame mission at the saved 30 FPS target (600 samples, 10 Hz):
1,080 rendered frames, about18.0 FPS, and53.20 ms of game work per rendered
frame. The world stage averaged43.80 ms, including22.28 ms visibility and
13.16 ms raster. Every sampled frame exceeded both the30 and60 FPS work
budgets. DTCM stats confirm the2,048-byte lookup is active; free DTCM is20,480
bytes after allocation. A follow-up GWProv read compared all2,048 bytes at
0x20015800 with the relocated XIP table; bytes and SHA-256
`9b611013336ab2231bda8164e4eca47c233a5fb71bcb56c7bf9f822d4b18ceee` match.

All six10-second windows stayed near18 FPS and53.2 ms/frame; there was no
progressive increase during the stationary capture. Geometry counters recorded
no arena drops, visibility or point-pool capacity hits, or far-capacity skips;
high-water marks were3,275 arena entries and4,561 visible entries. A second
60-second capture refreshed allocator free-span telemetry every5 seconds.
RAM_EMU, ITCM, DTCM and AHB free spans remained at32,768/13,720/20,480/18,836
bytes, and actor count stayed at62. This still does not reproduce the user's
movement-dependent growing lag; it shows a stable but overloaded stationary
Freezeflame view, not a demonstrated leak or progressive renderer slowdown.
The flash candidate remains installed. Full captures are
`build/perf/2026-10-26-zbin-dtcm/hardware/2026-10-25-stm32-60s.json` and
`build/perf/2026-10-26-zbin-dtcm/hardware/2026-10-25-lag-correlation-60s.json`.

Source inspection of the homebrew frame loop found no queued render-frame
backlog: `smg_game_frame()` runs synchronously once per loop, and
`smg_frame_dt()` derives elapsed time from the prior loop and caps one update at
100ms. Sustained over-budget work therefore slows wall-clock simulation rather
than accumulating an unbounded queue. This does not explain movement-triggered
lag or rule out workload growth while the camera/player move. A fresh GWProv
status query currently reports the hardware running Retro-Go SD rather than
SMG gameplay, so there is no active moving scene to sample; no input or reflash
was issued.

The accepted Observatory route provides a separate moving-workload check. Its
2,407-call replay sampled330 distinct player positions; actor count stayed39
across402 samples, and geometry counters reported no drops or pool exhaustion.
On the same route,60-call guest-instruction windows range from0.92M to3.23M
world instructions and0.72M to2.70M raster instructions, rising and falling
with the view rather than growing monotonically. This argues against general
actor accumulation on that route, but does not reproduce the user's
movement-dependent Freezeflame lag or establish physical timing.

The current360-call Deep Dark opening trace points at raster throughput as the
next software target: mean guest instructions in the first versus last60-call
windows rise from4.14M to4.69M for world work and3.13M to3.87M for raster,
while visibility falls from0.46M to0.20M. The opening camera exposes a denser
view over time; this is workload growth, not evidence of a leak or backlog.
Continue raster profiling against these changing views and the accepted route;
do not infer M7 frame time from guest instruction counts.

The current target disassembly also rejects an assumed easy optimization: the
perspective shader's span reciprocals compile to hardware `VDIV.F64` in
`engine_raster_inner_perspective_opaque_depth`, not a software double-divide
helper. A single-precision rewrite would therefore trade UV precision without
removing a software divide; it needs measured M7 cycle benefit and a separate
error proof before it is worth pursuing.

#### Partial-cache v3 guest instruction comparison

The optional v3 cache gives the main table 512 entries with two-way LRU and
keeps 128 far-table entries isolated, using 23,296 B total. An identical
360-call Freezeflame replay was measured with guest-internal instruction
counters (all 360 API calls rendered; startup excluded; no scanout/FPS claim).
All 60 boundary samples, final state, geometry counters, and indexed framebuffer
matched; neither run faulted or exhausted geometry pools. The cache reached
90,534/157,592 main hits (57.45%) and 13,745/13,788 far hits (99.69%).

This hit rate did not translate into a speedup: mean world-stage instructions
rose from 1.991 M to 2.008 M (+0.88%), visibility from 253.2 K to 269.8 K
(+6.56%), and emission from 120.4 K to 121.2 K (+0.73%); transform and raster
work were effectively unchanged. Host replay duration was 21.76 s off versus
22.45 s on, corroborating no benefit but not serving as physical timing. Keep
the cache opt-in and default off: its replacement policy improves hit ratio
over v2 but adds lookup/LRU work that costs more than the saved table reads in
this workload. Reports are in
`build/perf/2026-10-26-partial-chunk-cache/v3-instruction/`; private GWProv
instances and `tmp/` profiles were stopped and removed.

#### Render-contract fixture maintenance

`tools/audit_render_contracts.py` now isolates the production culling code from
the optional partial-cache preprocessor block. Its standalone Docker fixture
passes both texture mip layouts (U=2,V=7), indexed/unindexed axis and backface
triangle rejection, and normal-cone rejection. A new parent/child hierarchy
case confirms satisfied frustum-plane bits propagate: three leaves are retained
with five plane tests total, rather than rechecking the same five planes in
every child. `tools/test_visibility.py` had the same stale extraction boundary;
after updating it, its Docker ASan suite passes 4,000 randomized culling,
collection and transform jobs against the independent reference. The default
Docker bundle was restored afterward with the partial cache and DTCM Z-bin copy
disabled. No VM or hardware was touched by these fixture runs.

#### 54-slot vs. 64-slot texture-cache tradeoff

Compared the default 54-slot/32-KiB source-vertex-cache build against a
64-slot/16-KiB source-vertex-cache build. Both were Docker-built and installed
through a private GWProv daemon, then replayed headlessly in GWemu. The accepted
route used `build/perf/player-route-20261023-165706/route-02.tl` at 60 Hz with
startup dismissal/warmup and the final pause excluded; the separate 360-call
Deep Dark stage-7 run exercised the heavy scene. All 402 route samples and 60
Deep Dark samples matched, as did final contexts, indexed framebuffer hashes,
and every geometry counter. Both builds rendered every requested call without
faults, arena drops, or point/visibility-pool exhaustion.

The larger texture-cache build showed no meaningful runtime gain: GWemu host
replay time changed from 187.50 s to 183.96 s on the route and from 30.87 s to
30.83 s in Deep Dark. These are host durations, not FPS. Internal reported game
time moved in opposite directions (+1.4% route, -3.0% Deep Dark). The capture
does not expose per-slot texture-cache hits, so any texture-miss improvement is
unmeasured. The trade is concrete: source-cache capacity fell from 4,096 to
2,048 entries; route hits fell 5.4% (843,570 to 797,900), and Deep Dark hits
fell 13.2% (219,649 to 190,657), with corresponding misses increasing. The
runtime memory report also rose from 22,528 B to 28,672 B in its DTCM counter.
Keep 54 slots as the default; the 64-slot experiment does not establish a
speedup and sacrifices useful vertex-cache capacity. Detailed reports,
comparisons, and binary/ELF artifacts are under
`build/perf/2026-10-26-texture-cache-64/`. The normal Docker bundle was rebuilt
afterward with 54 slots, partial chunk caching off, and the DTCM Z-bin copy off.
GWProv stopped its private daemon and all owned VMs and removed its `tmp/`
profiles; no hardware was accessed.

### 2026-10-26: exact 32-bit whole-chunk cone products

The hot cone test only evaluates deltas strictly inside +/-2^17. With signed8
normal and unsigned8 extent fields, the three-term dot and radius sums are
bounded by 50,331,264 and100,269,315, so their products/sums fit signed32.
`chunk_cone_rejected()` now uses 32-bit multiply/accumulate for those terms,
retains the conservative out-of-range fallback, and keeps the final
`cone_plane_limit` comparison 64-bit. Target disassembly places the culler in
ITCM and shows 32-bit `MUL/MLA` for cone products; the previous path used
`UMULL`/`SMULL`/`SMLAL` wide product accumulation there. The compiled culler
shrunk60B (0x958 to0x91c); the build's total ITCM payload decreased56B.
RAM_EMU code/data and BSS are unchanged.

Docker ASan differential visibility tests pass 4,000 randomized jobs plus72
cone boundary combinations: deltas immediately inside/outside both +/-2^17,
normal extremes, maximum extents and cone limits spanning int32. The independent
64-bit culling oracle matches exactly; the separate mip/triangle/cone/frustum
render-contract fixtures pass. The Docker SDK build, relocation generation and
ITCM layout check pass.

GWProv installed the candidate into a disposable copy of the accepted baseline.
The full60Hz route (2,407 calls/402 sampled states) and360-call Deep Dark replay
match the previous build's every sampled state, final context, indexed image
and geometry counter, with zero faults or capacity losses. One headless run
changed GWemu host duration from187.50s to181.75s on the route and30.87s to
30.84s in Deep Dark; internal game time changed10484 to10422ms on the route and
2603 to2614ms in Deep Dark. These single-run mixed results do not establish a
repeatable speedup or FPS gain. The exact arithmetic improvement remains in the
default build; confirm its timing on hardware when access is granted. Evidence
and the candidate bundle/report are in
`build/perf/2026-10-26-cone-narrow/`. GWProv stopped its private daemon and all
owned VMs and removed `tmp/cone-narrow-gwprov/`; hardware and visible VMs were
untouched.

### 2026-10-26: bound cone-camera deltas once per cull job

`StageChunk.cone_center[]` is signed16 and cone evaluation accepts only deltas
strictly inside +/-2^17. A camera component outside `[-163839,163838]` cannot
form an accepted delta with any chunk center, so `geometry_cull_chunks()` now
checks this once and skips cone arithmetic for the entire job. For reachable
cameras, camera-minus-center is proven safe in signed32; cone dot/radius sums
remain signed32 and the plane-limit expression remains signed64. The target
culler is 12 bytes smaller than the preceding exact32-bit cone build.

Docker ASan differential tests pass 4,000 randomized jobs and84 cone cases,
including the four camera reachability endpoints and adjacent values on each
axis. The independent render-contract fixtures pass. The Docker SDK bundle,
asset relocation check and ITCM layout check pass. GWProv installed it into a
disposable copy of the accepted profile and headless GWemu completed both the
full player route (2,407 calls/402 samples) and Deep Dark (360 calls/60
samples). Every sampled state, final context, indexed image and geometry count
matches the preceding build; neither replay has faults or capacity losses.

Single-run timing is mixed: route GWemu wall time was181.75s before and182.03s
after, with measured game time10,422ms and10,520ms; Deep Dark was30.84s and
30.56s, with game time2,614ms and2,569ms. These runs do not establish a
repeatable speedup. The sampled route and Deep Dark cameras were always within
the reachable range, so this workload does not exercise the job-wide skip.
Evidence and both bundles/reports are under
`build/perf/2026-10-26-cone-camera-reachability/`. GWProv stopped its private
daemon, all owned VMs and `tmp/cone-camera-reachability-gwprov/`; no visible VM
or hardware was accessed.

### 2026-10-26: remove redundant cull-job stack copy

`geometry_cull_chunks()` writes only `leaf_count`, and only after finishing the
traversal. Its job inputs are therefore stable throughout the walk; reading
the descriptor in place removes the prior 176-byte `memcpy` on every cull call.
The Docker-built ITCM function shrank from 0x910 to 0x86c bytes, and its stack
allocation fell from 684 to 308 bytes. The memcpy veneer is gone.

Docker ASan differential visibility tests pass 4,000 randomized cases and 84
cone boundary cases; independent raster, axis-rejection, cone and inherited
frustum-mask contracts also pass. GWProv installed the candidate into a
disposable baseline profile. Headless GWemu ran the full accepted route and
Deep Dark. All 402 route and 60 Deep Dark sampled states, final contexts,
indexed images and geometry counters match; both have zero faults and zero
geometry-capacity losses. Rendered-world game time was 10,520 to 10,400 ms on
the route and 2,569 to 2,469 ms in Deep Dark (single runs, not M7 cycle
measurements). GWemu host time was 182.03 to 182.96 s on the route and 30.56
to 30.34 s in Deep Dark, so it does not establish a repeatable speedup. The
substantial stack reduction and removal of a hot-path copy are retained;
hardware timing remains outstanding.
ELFs, bundles, reports and comparisons are under
`build/perf/2026-10-26-cull-job-copy/`. GWProv stopped its private daemon and
all owned VMs and removed `tmp/cull-job-copy-gwprov/`; no visible VM or hardware
was accessed.

### 2026-10-26: transform-job copy experiment rejected

Removing `engine_transform_points()`'s 76-byte input snapshot removed its
`memcpy` veneer, shrank the function by 124 bytes and reduced its local stack
allocation from 196 to 52 bytes. Docker randomized transform, DSP and vertex
cache suites passed. The accepted route and Deep Dark replayed exactly, with
matching states, images and geometry and no capacity losses or faults.

The runtime result did not justify retaining it: internal render time rose
from 10,400 to 10,527 ms on the route and 2,469 to 2,493 ms in Deep Dark; host
times also rose slightly. These are single noisy runs, but neither cohort
showed a gain. The snapshot likely helps keep the matrix/pointer inputs stable
through the per-vertex loop. Restored it and Docker-rebuilt the default bundle;
the active bundle is back to the verified culler-only build. Rejected binary
and reports remain under `build/perf/2026-10-26-transform-job-copy/`. GWProv
stopped its private daemon and all owned VMs and removed
`tmp/transform-job-copy-gwprov/`; no visible VM or hardware was accessed.

### 2026-10-26: five-entry frustum-mask stack experiment rejected

The culler’s inherited-plane mask can only lose bits, so the maximum live
nesting is five. A crafted five-level test confirms the traversal restores all
inherited masks correctly. Reducing the saved stack from 32 entries to five
saved 224 bytes of culler stack, but increased the function from 0x86c to
0x8a0 bytes. Headless GWemu replay matched all 402 route and 60 Deep Dark
boundary samples, images, contexts and geometry, with no faults. Game time
rose from 10,400 to 10,599 ms on the route and 2,469 to 2,567 ms in Deep Dark;
host replay time also rose slightly. Since this small stack saving regressed
both workloads, the 32-entry stack is retained. The five-level inherited-mask
regression test remains. Evidence is under
`build/perf/2026-10-26-cull-mask-stack/`. GWProv stopped its private daemon and
removed `tmp/cull-mask-stack-gwprov/`; no visible VM or hardware was accessed.

### 2026-10-25: projection divider on accepted replay workloads

Added `--projection-divide on|off` to `tools/replay_game_frames.py` and
compared the existing exact UDIV path with the mapped-XIP table path in the
same ELF. The accepted Observatory recording and the 360-call Deep Dark opening
match at every sampled boundary (402/60), final context, indexed image and
geometry counter; neither run faults or changes geometry. This closes the
previous short stationary-view correctness/performance evidence gap.

The single GWemu run is inconclusive for speed: host replay time changed from
184.37 to 183.12 seconds on the route and 30.45 to 30.14 seconds in Deep Dark.
Rendered-world timer/frame changed from 4.99 to 4.57 ms on the route but 7.40
to 7.53 ms in Deep Dark; SDK skipped-blit counts also differed (114/146 route,
15/19 Deep Dark). These mixed emulator results do not establish an M7-cycle or
hardware benefit. Keep the exact divider enabled as currently configured; the
external-flash latency trade still needs a physical A/B when hardware access is
granted. Reports, captures and private-service cleanup record are in
`build/perf/2026-10-26-projection-divider/`. GWProv stopped its private daemon
and all owned VMs and removed `tmp/projection-divider-gwprov/`; no visible VM or
hardware was accessed.

### 2026-10-26: match the GBA's mip-selection rule

The original `src/graphics/raster.s` calculates mip level from the horizontal
U and V gradients only, using `RECIP`, signed arithmetic shift and the 1536
threshold. The C path had included vertical gradients and used a direct
mathematical threshold, which differs at reciprocal-rounding boundaries. The
GNW path now mirrors the GBA reciprocal normalization/table and signed-shift
behavior for supported positive 32-bit screen areas, and tests horizontal U/V
only. The wider-area fallback retains the previous conservative C rule; with
the current 160×120 raster bounds those areas are unreachable.

The independent Docker audit passes 8,918 reciprocal/threshold oracle cases,
seven pixel-level mip/addressing cases, and the existing axis rejection,
backface cone, and inherited-frustum-mask fixtures. The runtime replay uses
the accepted route and Deep Dark opening. All 402 route and 60 Deep Dark
sampled player/camera states and final contexts match the previous build; both
complete all game render calls without faults, buffer losses, or culling
changes. Mip counts change as expected: route 274,916→144,337 and Deep Dark
59,455→33,923 triangles select mip 1. Across the six saved frames, indexed
pixel differences range from 0.02% to 0.59%; these are texture-level changes,
not geometry or simulation divergence.

The helper and 512-byte reciprocal table are placed in ITCM. The current
`.core_itcm` code image is 54,304 of 65,536 bytes, leaving 11,232 bytes. One
GWemu run measured game time 10,323→9,986 ms on the route and 2,566→2,392 ms in
Deep Dark; wall duration moved 183.12→181.81 s and 30.14→30.62 s respectively.
This mixed single-run result is inconclusive, and GWemu time is not a hardware
FPS measurement. No hardware was accessed. Replays, comparisons, and the
disposable runner are in `build/perf/2026-10-26-mip-policy/`; its private
GWProv daemon and owned VMs were stopped and `tmp/mip-policy-gwprov/` removed.

### 2026-10-26: current-build stage-instruction profile

Captured instruction-stage reports from the current mip-policy build on the
accepted 2,407-call Observatory recording and the 360-call stage-7 Deep Dark
opening. The active portions include 2,405 and 358 calls after warmup. Mean
Deep Dark guest work is 4.40M instructions/call for world rendering, including
3.46M raster, 354K visibility, 87K transform, and 245K emission. The heaviest
six-call raster windows reach 4.0M instructions/call; the final four calls
reach 4.15M. On the moving route, active means are 2.27M world and 1.91M
raster instructions/call, with the heaviest raster windows around 2.84M.

Both traces complete without CPU faults. They use GWemu baseline instruction
counting, with the firmware audio-DMA wait bypassed after each game API return;
they are workload-location evidence, not M7 cycles or FPS. Raster remains the
largest optimization target, and the changing Deep Dark view must remain in
the measurement set. Full reports and six-call window summaries are in
`build/perf/2026-10-26-current-instruction-profile/`. The private GWProv daemon
and owned headless VMs were stopped, and
`tmp/current-instruction-profile-gwprov/` was removed. No hardware or visible
VM was accessed.

### 2026-10-26: specialize the default perspective span path

The common perspective-texture span mode has neither endpoint reuse nor the
curvature precheck. Added an inlined entry that passes literal null optional
state to the existing span implementation, allowing the compiler to remove
those checks from the pixel loop. The other span modes retain the generic
implementation. This matches the current tested default, where endpoint reuse
is disabled; no runtime setting or image policy changed.

Docker-built instruction-stage comparisons on the accepted route and Deep
Dark opening show raster guest instructions per call falling from 1.907M to
1.799M (-5.68%) on the route and 3.460M to 3.259M (-5.81%) in Deep Dark.
Every six-call window improved. The accepted route's 402 and Deep Dark's 60
sampled states, final indexed images, geometry counters and render-call
completion match; there are no faults or recorded workload changes. The span
and raster correctness suites pass at both supported render sizes.

The code image grew by 944 bytes to 55,248 bytes of the 65,536-byte ITCM
budget, leaving 10,288 bytes. This is an instruction-count reduction, not a
measured hardware FPS improvement. Single GWemu internal render timings rose
slightly (route 10,400→10,527 ms; Deep Dark 2,469→2,493 ms), while host duration
fell slightly, so timing evidence is inconclusive and does not support an FPS
claim. The current local bundle contains this candidate: `SMG.bin` SHA-256
`72b3e8189f460ae9aa6503a07cee89631fb419a3ae06e522344d5e1dc239489f`; bundle
ZIP SHA-256
`02e962c050f984616e1b400ccf7b6503cf6a54548af07a17b33d26347ec12388`. Full
reports are under `build/perf/2026-10-26-raster-default-span-fastpath/`.
GWProv stopped its private daemon and owned VMs and removed
`tmp/raster-default-span-fastpath-gwprov/`; no hardware or visible VM was
accessed.

### 2026-10-26: global perspective-correction A/B

Compared the current default affine texture mapping with perspective
correction enabled through the existing runtime control. Both variants used the
same ELF, current bundle, accepted 60 Hz route and stage-7 Deep Dark replay in
fresh copies of the preserved baseline profile. All 402 route and 60 Deep Dark
player/camera boundary states match, as do final contexts and geometry
counters; both render every call without CPU faults. The indexed images differ
as expected because the mapping policy changes.

Perspective correction makes substantial visible changes, concentrated in the
ground-heavy lower half of the route captures: 25.7%, 36.6% and 26.0% of those
pixels change at calls 120, 600 and 1200. This supports the diagnosis that
affine mapping causes much of the observed ground-texture distortion. It also
raises guest raster instructions by 35.8% on the route and 39.0% in Deep Dark;
all 401 and 60 complete six-call windows respectively cost more. Guest world
instructions rise 28.1% and 27.8%. This is an emulator instruction comparison,
not hardware frame time or proof that perspective correction alone accounts
for every texture defect.

Following the user's decision, global perspective correction is now the
default for new or unconfigured runtimes. The existing pause-menu
**Texture mapping: Fast / Accurate** choice toggles the same renderer-wide
setting and persists through the per-app settings store; saved Fast choices
remain respected. No material-specific flag is added. This knowingly accepts
the measured work increase for now; sustained hardware timing remains
unverified. Per-capture images, comparisons, reports and the headless runner are in
`build/perf/2026-10-26-perspective-correction-ab/`. GWProv stopped its private
daemon and all owned VMs, and removed
`tmp/perspective-correction-ab-gwprov/`. No hardware or visible VM was
accessed.

### 2026-10-26: default to global Accurate mapping, retain pause-menu toggle

Set the engine's unconfigured `g_perspective_enabled` initial value to true.
The homebrew adapter already defaults missing per-app `tex_mapping` settings
to Accurate and already exposes the persisted Fast/Accurate pause-menu toggle;
saved choices still override the initial value. Accurate applies to all
textured surfaces through the existing global raster path. No Retro-Go firmware
or material/asset format was changed.

The Docker-built local bundle passes the perspective-span, opaque-raster and
raster-specialization suites at 160x120 and 320x240, plus the independent GBA
mip/addressing, collector-axis, backface-cone and frustum-mask audit. GWProv
installed that bundle into a disposable copy of the accepted baseline profile.
A 120-call accepted-route headless smoke replay, without a perspective
override, reports Accurate enabled at startup, zero CPU faults and zero arena,
visibility, point-pool or far-capacity losses. The full route/Deep Dark A/B in
the preceding entry exercises both Fast and Accurate with matching sampled
states and geometry; the existing adapter callback persists user toggles.

Build sizes: SMG.bin 132,417 bytes; RAM code/data 74,960 bytes; BSS 602,712
bytes; ITCM 55,248 of 65,536 bytes. SMG.bin SHA-256
`02c5e7b2d4d384c22de7a7648469ca43483afe9661ad19b58f31a8b1ea6a1c0e`, code XIP
`b5c998aa2ad5b4818aa4c5bd6ce0747d1194397acb477fdcd984264713ae34e4`, bundle
ZIP `3a5ddd871839146305ecd7b6adb58c80580b52967e2ee9440a6feb61313bdf03`.
GWProv stopped its private daemon and owned VM and removed
`tmp/accurate-default-smoke-gwprov/`. No hardware or visible VM was accessed.

### 2026-10-28: recheck the user-reported sparse scene

The saved runtime state at `build/perf/2026-10-26-review-invisible-level/`
has `stage_index=3`, which the generated stage table maps to
`STAGE_HONEYHIVE_TOWER`, not Freezeflame. The previous conversational
identification as Freezeflame was incorrect. The saved view mode is High; the
renderer has no recorded arena/list/point/far-capacity losses and no captured
CFSR/HFSR faults. A fresh Docker asset reconstruction produces the same
32,750,308-byte asset XIP hash already in the current runtime, so stale
generated stage assets do not explain the report.

Offline source-mesh projections at the captured camera show that the Tower's
far mesh contributes broad ground/column surfaces that are absent from the
full-mesh-only projection. This is consistent with High's far-LOD path, but it
does not establish that the user's missing surfaces are an intended LOD result:
the capture script can stop between frame boundaries, and the geometry buffer
is not guaranteed to describe the displayed screenshot. GWProv now reports the
old visible GWemu with its QMP endpoint unavailable, so no same-position
High/Full runtime comparison was possible. The independent projection images
and passive capture remain under
`build/perf/2026-10-26-review-invisible-level/`. No live VM or hardware state was
changed; the LOD explanation remains a hypothesis pending a coherent runtime
comparison.

The independent Docker-built `tools/audit_render_contracts.py` gate was rerun
on 2026-10-28 and passes: 8,918 original-GBA mip-threshold cases, both texture
mip address patterns, indexed/non-indexed axis and facing rejection, whole-chunk
cone rejection, and inherited frustum masks including the five-level stack.

### 2026-10-28: Tower detail comparison at arrival

Using the current native bundle, GWProv and headless GWemu, replayed 180 calls
at 60 Hz from disposable copies of the accepted baseline, overriding the stage
to Honeyhive Tower. High and Full matched all 30 sampled player/camera states
and final context. The intro was still active, so its automatic High-detail
override was disabled for this comparison. With the override enabled, both
requested settings produced identical images and geometry, confirming that
the earlier pair did not actually compare the modes.

At the arrival camera Full restores substantial terrain missing in High; the
frame-180 images differ in 33,572 of 76,800 RGB output pixels. Full submitted
309,431 arena entries over the run versus 54,399 for High, with peaks of 2,083
and 366 respectively. Neither run recorded faults or geometry capacity losses.
This is a synthetic stage override during arrival, not the user's captured
position or a hardware FPS measurement; it shows an important quality/cost
tradeoff without proving the cause of that report. Images, reports and the
comparison are under `build/perf/2026-10-28-tower-detail/no-cinematic/`.
The private daemon and owned VMs stopped and the disposable profile was removed.

### 2026-10-28: Tower source-mesh coverage diagnosis

Projected the reconstructed main and far Tower OBJs independently at the three
captured arrival cameras, with the same near-plane clipping, backface policy,
depth and binary alpha rules. At calls 60, 120 and 180 the main mesh covers
2,517, 1,610 and 1,338 of 19,200 world pixels respectively where the far mesh
has no coverage. These are static source projections: the runtime distance
split, actors, lighting and animated materials are excluded. They establish
that the authored far mesh itself has coverage gaps in these views; they do not
attribute every runtime pixel difference to that mesh.

At call 180 the uncovered main-mesh pixels are chiefly soil (663), stripes
(180) and water (134). None of the contributing main triangles has an exact
vertex-for-vertex far triangle, although a different triangulation can represent
the same surface. Across the three views, selecting every main triangle that
contributes uncovered pixels would add 483 faces and 599 source positions not
exactly present in the 881-point far OBJ. The 1,535-point compiler limit leaves
little apparent room; these are pre-quantization counts, not a compiled size.
Blindly appending those faces could overlap existing simplified surfaces and
introduce depth or seam artifacts, and would only target three cameras. No mesh
change has been promoted. The coverage summary and contributing faces are in
`build/perf/2026-10-28-tower-detail/no-cinematic/`; temporary projections were
removed. A feature-aware repair should preserve material/UV and topology, then
be checked against wider viewpoints and measured for geometry and raster cost.

### 2026-10-28: Tower far-mesh overlay prototype

Built an offline, temporary OBJ by adding the 483 main-mesh faces selected by
the three arrival projections to the existing far mesh. The resulting OBJ has
1,480 positions before compiler quantization, versus the 1,535-point compiled
limit. It fills the selected views' measured source-projection coverage gaps,
but changes 2,348–3,738 already-covered pixels per view as added triangles
overlap the simplified far mesh. Exact RGB matches to the main projection rise,
but this approximate static renderer cannot establish correct depth ordering,
texture animation, or seam behavior.

The independently captured user camera was not used to select faces: its 85
main-only source pixels remain unchanged, and the candidate changes no pixels
there. Thus this view-specific overlay does not address the reported scene and
is not promoted. The prototype script, numerical report and two static images
are in `build/perf/2026-10-28-tower-detail/no-cinematic/`; its temporary OBJ and
images were removed. No game asset, firmware, hardware or live VM was changed.

### 2026-10-28: captured Tower view uses the alternate stage mesh

Restored the saved camera and player position in disposable headless GWemu,
then invoked `draw_scene` directly with figures suppressed. This is a coherent
static-geometry comparison, not a restoration of the original gameplay state.
At the captured pose, both High and Full Tower views draw only a small distant
structure (27 and 253 submitted entries respectively). Loading regular
Honeyhive at the same pose draws a broad foreground surface and columns in both
detail modes (194 and 424 entries). No geometry capacity losses occurred in
these four draws. Images and matching camera/counter records are in
`build/perf/2026-10-28-tower-user-pose/`.

The saved Tower state contained 53 actors and 17 submitted figures, including
Lotus Leaf platforms around Mario. The reconstructed regular Honeyhive OBJ
contains `fr_top`, `fr_belly`, and `fr_columns` (532 faces total) around this
area; those groups are absent from the Tower OBJ. Tower nevertheless has the
shared stage objects there. This provides a concrete explanation for the
reported missing surroundings: changing High to Full cannot restore geometry
absent from the selected stage mesh. Tower also has mission-specific groups
absent from regular Honeyhive, so replacing the whole stage with the regular
mesh would discard other content. A proper repair must combine the required
geometry and collision with scenario semantics, validate the converter paths
and check memory/geometry limits and representative traversal. The saved
capture reports Tower scenario 1; the normal Honeyhive mission selector chooses
the Tower variant for missions 2 and 3, so how this state was reached is not
established. No production asset was changed by this diagnostic.

### 2026-10-28: isolated Tower mesh restoration candidate

Built an isolated candidate by copying the regular Honeyhive `fr_top`,
`fr_belly` and `fr_columns` groups into the Tower main OBJ, and the corresponding
`fr_` materials' faces into its far OBJ. Its generator also supplies the eight
missing main materials and their textures. The existing Tower groups are
retained. The corrected Docker compiler output adds 532 main faces and 57 far
faces: main 4,536 points/7,642 faces/267 materials; far 954 points/814 faces,
under the 1,535-point limit. Collision includes 7,638 triangles versus 7,106
before. A Docker production-ray audit reports 0 misses or capped queries in
74,424 floor probes (original: 0 in 68,730). This does not cover dynamic
collision or all traversal. The first candidate generator accidentally repeated
OBJ object declarations; the importer retains only the last group for repeated
names. That discarded build was corrected before the results below.

The Docker SDK homebrew has 75,120 bytes RAM code/data, 602,712 BSS and 55,248
ITCM; asset XIP grows by 39,592 bytes. GWProv installed the candidate in a
disposable headless profile. At the saved user camera, completed static renders
in High and Full show the missing foreground and columns, with no recorded
geometry losses. Figures were suppressed, so this is not a full gameplay
reconstruction. The accepted 2,407-call route matches the current build at all
402 boundary states, final context and three captured indexed/palette/world
images; no faults or geometry losses. A separate 180-call Tower arrival cohort
matches all 30 player/camera samples in both detail modes; candidate High/Full
arena peaks are 404/2,234 with no losses. This is not a physical FPS or mission
traversal result.

The candidate remains isolated: its reconstructed OBJ edits are not yet encoded
in the shared native/WASM stage recipes, and neither converter has produced a
release containing them. Scenario behavior, full traversal, broader visibility
and physical performance remain to be checked before promotion. The reproducible
generator, compiler/build logs, images and replay reports are under
`build/perf/2026-10-28-tower-mesh-candidate/`; owned private VMs and daemons
were stopped and temporary build/profile files removed. No hardware or Retro-Go
firmware was accessed or changed.

### 2026-10-28: shared Tower recipes and normal installation

Encoded the tested Tower restoration in the existing compressed stage recipe
and added two output aliases to the shared texture recipe. The original Tower
OBJ prefixes and stage/galaxy descriptors are preserved; the new main/far
faces match source positions, UVs and material mappings. The deterministic
generator `tools/asset_construction/restore_honeyhive_tower.py` checks its
pre-repair recipe and reconstructed-source hashes; the source packs are archived
under `build/perf/2026-10-28-tower-mesh-candidate/pre-repair-*`. The independent
`tools/test_honeyhive_tower_recipe.py` checks the added geometry and aliases,
and can compare original prefixes when given the pre-repair constructed stages.

The native and Rust stage decoders produced identical OBJ/MTL/descriptor bytes.
The Rust texture aliases decode to identical source pixels. Docker compiled the
decoded assets and the previously tested OBJ candidate independently: all 842
output files were byte-identical. A clean native asset and SDK Docker rebuild
produced the same Tower binary hashes and the same 32,789,900-byte asset XIP as
the candidate. GWProv installed the standard WASM bundle from the USA ISO at a
10-trillion fuel budget; its output matches native SHA-256
`a12eec2dd89bf7f29c75d1adf914893d325bc2959b2bf87dd6f27eafe1d5fad1`.
The installed bundle completed a 120-call accepted-route headless smoke with
Accurate mapping, zero CPU faults and no recorded geometry losses. Full ISO
conversion in headless Chromium 153 and Firefox 155 at 128 MiB also matched that
hash, in 105.82 s and 108.38 s respectively. The 64 and 256 MiB converter
variants were rebuilt but were not separately run through the full browser test.

The current bundle ZIP SHA-256 is
`0a34357164ec70778f86e0492ee91b38f583e461367ef4d59e933b3b971aa152`.
Reports and browser logs are in `build/perf/2026-10-28-tower-wasm-install/`.
This fixes the tested static missing terrain; complete mission traversal,
dynamic actors, other views and physical performance still require validation.
No live review VM or hardware was touched.

### 2026-10-28: green Beach Bowl bridge in the review VM

The user's screenshot shows a solid green bridge in Beach Bowl. The running
GWemu's relocated ITCM image matches the current ELF byte for byte. Its scene
uses Full detail, depth buffering and perspective textures. A briefly halted
renderer snapshot identified a bridge triangle whose mip 0 points to
`0x91e036f4`; all 1,024 texels there are `0xff` (palette index 255 is green).
The matching bytes in the local asset XIP contain varied wood-coloured texels,
and the installed mip 1 also contains nonuniform image data.

Read-only comparison of the review profile's virtual NOR with the matching
local asset XIP found a damaged 256 KiB range at `0x91e00000..0x91e40000`:
the first 64 KiB is erased and the following 192 KiB differs extensively. The
next 64 KiB matches again. Relocated pointer tables elsewhere legitimately
differ from the unrelocated XIP, so the whole-file difference count must not be
interpreted as corruption. This explains the green surface and may affect
other textures. It does not establish when or how the range was overwritten.
The game was resumed after capture; neither the profile nor the physical
device was modified.

The external-flash scan identifies the sidecar as part of FrogFS beginning at
`0x90400000`, not the SD file cache. The damaged range is exactly one 256 KiB
web-builder flasher transfer block, aligned relative to the FrogFS start. Its
192 KiB non-erased tail exactly matches a contiguous range of the older
FrogFS file `/homebrews/Jazz.jj` in another local profile, consistent with an
interrupted or incomplete replacement;
this does not prove which writer was responsible or whether an error was
reported. The user recalls installation through GWemu or web-builder.

A fresh GWProv installation, followed by a 120-frame headless Beach Bowl replay,
left the 6 MiB region containing the damaged address byte-identical to the
native sidecar. This does not reproduce the web-builder update path. The
read-only `tools/audit_flash_texture_pool.py` compares the entire immutable
texture pool without confusing relocated pointer tables for corruption; the
review image had exactly four differing 64 KiB blocks. The user's GWemu had
closed, so its local profile's 256 KiB block was restored from the verified
sidecar with the old bytes preserved for rollback in
`build/perf/2026-10-28-flash-integrity/review-frogfs-block-before.bin`.
The full 8,019,732-byte immutable texture pool now matches. An independent
comparison of the complete 32,789,900-byte installed sidecar, applying 12,085
ELF-recorded relocation sites to the expected image, also matches every byte.
Reports are in
`build/perf/2026-10-28-flash-integrity/`. The current web-builder flasher
protocol suite passed 133 tests when compiled and run in an isolated Docker
copy. The event that interrupted or skipped the update remains unidentified;
reproduce it and repair the install path before considering this resolved.
