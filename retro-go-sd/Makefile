# STM32H7B0 Retro-Go SD homebrew build for Super Mario Galaxy.

PROJECT_KIND := homebrew
CORE_NAME := smg
CORE_ENTRY := app_main
CORE_C_SOURCES := \
../src/main.c \
src/smg_homebrew.c \
src/smg_xip.c \
src/arm_compat.c
CORE_C_INCLUDES := \
-I../src \
-I../src/actors \
-I../build/assets \
-I../build/constructed/src \
-I../build/constructed/src/actors \
-I../build/constructed/src/game \
-I../build/constructed/src/graphics \
-I../build/constructed/src/player \
-I../build/constructed/src/ui \
-I../build/constructed/src/world \
-Isrc
SMG_ZBIN_RAM ?= 0
SMG_ZBIN_DTCM ?= 0
SMG_PARTIAL_CHUNK_CACHE ?= 0
SMG_TEXTURE_CACHE_SLOTS ?= 54
CORE_C_DEFS := -DPROJECT_KIND_HOMEBREW=1 -D__GNW__ -DSMG_GAME_RUNTIME=1 -DSMG_ZBIN_RAM=$(SMG_ZBIN_RAM) -DSMG_ZBIN_DTCM=$(SMG_ZBIN_DTCM) -DSMG_PARTIAL_CHUNK_CACHE=$(SMG_PARTIAL_CHUNK_CACHE) -DSMG_TEXTURE_CACHE_SLOTS=$(SMG_TEXTURE_CACHE_SLOTS)
CORE_LDSCRIPT := smg_core.ld
BUILD_DIR := build/smg
SMG_RELOCS_PRE_ELF := build/smg/smg_core_relocs.elf
SMG_RELOCS_ASM := build/smg/smg_asset_relocs.s
SMG_RELOCS_OBJ := build/smg/smg_asset_relocs.o
SMG_ITCM ?= 1
SMG_ITCM_VIS ?= 1
SMG_ITCM_EMIT ?= 1
SMG_ITCM_PLACEMENT := build/smg/smg_itcm_placement.ld
CORE_EXTRA_OBJECTS := build/smg/assets.o $(SMG_RELOCS_OBJ)
CORE_ELF_EXTRA_DEPS := build/smg/assets.o $(SMG_RELOCS_OBJ) $(SMG_ITCM_PLACEMENT)

# Set this before including the SDK: its default is -Os, and a later ?= would
# never override that value. This port's validated game build uses -O3.
OPT ?= -O3 -fno-strict-aliasing

GNW_CORE_SDK ?= sdk
BUILD_DIR ?= build

include $(GNW_CORE_SDK)/Makefile

LDFLAGS += -Wl,--emit-relocs

# Keep absolute references to mapped assets as linker relocations. The build
# emits their exact locations so runtime code never scans opaque asset bytes.
CFLAGS += -mword-relocations
ASFLAGS += -mword-relocations

PACKED_BIN := SMG.bin
HB_NAME := Super Mario Galaxy
COVER_JPG := $(BUILD_DIR)/cover.jpg
COVER_WIDTH ?= 128
COVER_HEIGHT ?= 96
GEN_COVER := scripts/gen_homebrew_cover.py
PACK_HOMEBREW := $(GNW_CORE_SDK)/tools/pack_homebrew.py
SMG_CODE_XIP := build/smg/smg_code.xip
SMG_ASSET_XIP := build/smg/smg_assets.xip
XIP_CODE_SIDECAR := smg_code.xip
XIP_ASSET_SIDECAR := smg_assets.xip
CORE_VERSION ?= $(shell git describe --tags --abbrev=0 2>/dev/null || echo 0.0.0)

.PHONY: all pack cover docker local-bundle
all: pack

cover: $(COVER_JPG)

$(COVER_JPG): $(GEN_COVER)
	python3 $(GEN_COVER) --out $@ --title "$(HB_NAME)" --width $(COVER_WIDTH) --height $(COVER_HEIGHT)

pack: $(TARGET_BIN) $(XIP_CODE_SIDECAR) $(XIP_ASSET_SIDECAR) $(COVER_JPG)
	python3 scripts/gen_asset_relocs.py --elf $(TARGET_ELF) --out $(SMG_RELOCS_ASM) --check
	python3 scripts/check_raster_layout.py --elf $(TARGET_ELF) --itcm "$(SMG_ITCM)"
	python3 $(PACK_HOMEBREW) --elf $(TARGET_ELF) --bin $(TARGET_BIN) --name "$(HB_NAME)" --version "$(CORE_VERSION)" --cover $(COVER_JPG) --out $(PACKED_BIN)

$(SMG_RELOCS_PRE_ELF): $(C_OBJECTS) $(CXX_OBJECTS) $(ASM_OBJECTS) build/smg/assets.o $(CORE_LDSCRIPT) $(SMG_ITCM_PLACEMENT)
	$(CC) $(C_OBJECTS) $(CXX_OBJECTS) $(ASM_OBJECTS) build/smg/assets.o $(LDFLAGS) -Wl,--emit-relocs -o $@

$(SMG_RELOCS_ASM): $(SMG_RELOCS_PRE_ELF) scripts/gen_asset_relocs.py
	python3 scripts/gen_asset_relocs.py --elf $< --out $@

$(SMG_RELOCS_OBJ): $(SMG_RELOCS_ASM)
	$(CC) $(MCU) -c -o $@ $<

build/smg/assets.o: ../build/assets/assets.s ../build/assets/assets_gen.h
	@mkdir -p $(@D)
	$(CC) $(MCU) $(OPT) -mword-relocations -c -o $@ $< -Wa,-I../build/assets

$(SMG_CODE_XIP): $(TARGET_ELF)
	$(CP) -O binary --only-section=.xip_smg_code $< $@

$(SMG_ASSET_XIP): $(TARGET_ELF)
	$(CP) -O binary --only-section=.xip_smg_assets $< $@

$(XIP_CODE_SIDECAR): $(SMG_CODE_XIP)
	cp $< $@

$(XIP_ASSET_SIDECAR): $(SMG_ASSET_XIP)
	cp $< $@

.PHONY: print-SIDECARS
print-SIDECARS:
	@echo $(XIP_CODE_SIDECAR) $(XIP_ASSET_SIDECAR)

DOCKER_IMAGE ?= sylverb/retro-go-sd-builder:v1.5
.PHONY: smg-layout-force
smg-layout-force:

$(SMG_ITCM_PLACEMENT): smg-layout-force scripts/gen_itcm_placement.py
	python3 scripts/gen_itcm_placement.py --enabled "$(SMG_ITCM)" --visibility "$(SMG_ITCM_VIS)" --emission "$(SMG_ITCM_EMIT)" --out $@

docker:
	mkdir -p "$(CURDIR)/../tmp"
	docker run --rm --user "$$(id -u):$$(id -g)" -e TMPDIR=/opt/project/tmp -v "$(CURDIR)/..:/opt/project" -w /opt/project/retro-go-sd $(DOCKER_IMAGE) make all OPT="$(OPT)" CORE_VERSION="$(CORE_VERSION)" SMG_ITCM="$(SMG_ITCM)" SMG_ITCM_VIS="$(SMG_ITCM_VIS)" SMG_ITCM_EMIT="$(SMG_ITCM_EMIT)" SMG_ZBIN_RAM="$(SMG_ZBIN_RAM)" SMG_ZBIN_DTCM="$(SMG_ZBIN_DTCM)" SMG_PARTIAL_CHUNK_CACHE="$(SMG_PARTIAL_CHUNK_CACHE)" SMG_TEXTURE_CACHE_SLOTS="$(SMG_TEXTURE_CACHE_SLOTS)"

# Build the GWRG bundle from the executable and both mapped XIP sidecars just
# produced by the Docker SDK build. Keep the manifest and hashes in build/.
local-bundle: docker
	python3 scripts/package_local_bundle.py

# Read-only values consumed by the shared GWRG manifest builder.
.PHONY: print-PROJECT_KIND print-CORE_NAME print-PACKED_BIN print-TARGET_ELF
print-PROJECT_KIND:
	@echo $(PROJECT_KIND)
print-CORE_NAME:
	@echo $(CORE_NAME)
print-PACKED_BIN:
	@echo $(PACKED_BIN)
print-TARGET_ELF:
	@echo $(TARGET_ELF)
