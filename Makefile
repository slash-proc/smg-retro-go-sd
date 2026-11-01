DEVKITPRO ?= /opt/devkitpro
DEVKITARM ?= $(DEVKITPRO)/devkitARM
PREFIX    := $(DEVKITARM)/bin/arm-none-eabi-
CC        := $(PREFIX)gcc
OBJCOPY   := $(PREFIX)objcopy
GBAFIX    ?= $(DEVKITPRO)/tools/bin/gbafix
HOSTCC    ?= cc
HOSTFLAGS := -std=c11 -D_DEFAULT_SOURCE -O2 -ffp-contract=off -Wall -Wextra

BUILD     := build
GEN       := $(BUILD)/assets
ISO       ?= baserom_us.iso
ORIG      := $(BUILD)/orig
CONSTRUCTED := $(BUILD)/constructed
DATA      := asset_construction
CTOOLS    := tools/asset_construction
STEPS     := $(filter $(notdir $(wildcard $(DATA)/*)),procedural audio mario_figure figures stages textures ui_headers actor_headers)
GEN_DIRS  := actors game graphics player ui world

ARCH      := -mcpu=arm7tdmi -mtune=arm7tdmi -mthumb -mthumb-interwork
CFLAGS    := $(ARCH) -std=gnu11 -O2 -fwrapv -ffp-contract=off -fno-strict-aliasing \
             -ffunction-sections -fdata-sections -fno-common \
             -Wall -Wextra -Wno-unused-parameter -Isrc -I$(GEN) \
             -I$(CONSTRUCTED)/src $(addprefix -I$(CONSTRUCTED)/src/,$(GEN_DIRS)) \
             $(addprefix -Isrc/,$(GEN_DIRS))
LDFLAGS   := $(ARCH) -nostartfiles -T gba.ld -Wl,--gc-sections -Wl,--no-warn-rwx-segments

FRAGMENTS := $(shell find src -name '*.c' -o -name '*.h' -o -name '*.inc')
ASM       := src/gba/startup.s src/graphics/raster.s src/graphics/visibility_bvh.s \
             src/graphics/visibility_indexed.s src/math/fastmath.s

# Disposable compiler caches; keep generated assets, bundles and test evidence.
CACHE_DIRS := tools/wasm_converter/target tools/wasm_range_probe/target \
              $(BUILD)/wasm_converter/stage-galaxy-target .ruff_cache

all: $(BUILD)/galaxy.gba

$(ORIG)/.verified: $(wildcard $(DATA)/*/sources.txt) $(CTOOLS)/dump.py $(CTOOLS)/common.py
	python3 -B $(CTOOLS)/dump.py --iso $(ISO) --out $(ORIG) --tools .tools \
	    --manifest $(wildcard $(DATA)/*/sources.txt)

$(CONSTRUCTED)/.done: $(ORIG)/.verified $(shell find $(DATA) $(CTOOLS) assets -type f ! -name .DS_Store)
	rm -rf $(CONSTRUCTED)
	@mkdir -p $(CONSTRUCTED)/assets
	cp -R assets/. $(CONSTRUCTED)/assets/
	@for step in $(STEPS); do \
	    echo "Constructing $$step"; \
	    python3 -B $(CTOOLS)/$$step.py --orig $(ORIG)/files --data $(DATA) --out $(CONSTRUCTED) || exit 1; \
	done
	@touch $@

$(BUILD)/tools/asset_compiler: $(wildcard tools/asset_compiler/*) src/assets/asset_format.h \
                          $(CONSTRUCTED)/.done
	@mkdir -p $(@D)
	$(HOSTCC) $(HOSTFLAGS) -I$(CONSTRUCTED)/tools/asset_compiler -o $@ $(filter-out tools/asset_compiler/wasm_%.c,$(filter %.c,$^)) -lm

$(BUILD)/tools/pose_anchor_costs: tools/assets/pose_anchor_costs.c
	@mkdir -p $(@D)
	$(HOSTCC) $(HOSTFLAGS) -o $@ $<

$(GEN)/assets_gen.h: $(BUILD)/tools/asset_compiler $(BUILD)/tools/pose_anchor_costs \
                     $(wildcard tools/assets/*.py) $(CONSTRUCTED)/.done
	@mkdir -p $(GEN)
	$(BUILD)/tools/asset_compiler $(CONSTRUCTED)/assets $(GEN)
	python3 -B tools/assets/pack.py $(GEN) $(BUILD)/tools/pose_anchor_costs

$(GEN)/assets.s: $(GEN)/assets_gen.h

GAME_OBJS := $(BUILD)/game/main.o $(patsubst src/%.s,$(BUILD)/game/%.o,$(ASM)) $(BUILD)/game/assets.o

$(BUILD)/game.elf: $(GAME_OBJS) gba.ld
	$(CC) $(LDFLAGS) -o $@ $(filter %.o,$^) -Wl,-Map,$(BUILD)/game.map -lm

$(BUILD)/game/%.o: src/%.c $(GEN)/assets_gen.h $(FRAGMENTS) $(CONSTRUCTED)/.done
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -c -o $@ $<

$(BUILD)/game/%.o: src/%.s
	@mkdir -p $(@D)
	$(CC) $(ARCH) -c -o $@ $<

$(BUILD)/game/assets.o: $(GEN)/assets.s $(GEN)/assets_gen.h
	@mkdir -p $(@D)
	$(CC) $(ARCH) -c -o $@ $< -Wa,-I$(GEN)

$(BUILD)/galaxy.gba: $(BUILD)/game.elf
	$(OBJCOPY) -O binary $< $@
	$(GBAFIX) $@ -tGALAXYADV -cGLXY >/dev/null
	@echo "Built $@ ($$(wc -c < $@ | tr -d ' ') bytes)"

clean:
	rm -rf $(BUILD)

clean-cache:
	rm -rf $(CACHE_DIRS)
	find tools retro-go-sd/scripts -type d -name __pycache__ -prune -exec rm -rf {} +

clean-history:
	rm -rf $(BUILD)/perf retro-go-sd/build/perf tmp

disk-usage:
	@for path in $(BUILD) tmp .tools retro-go-sd/build tools/wasm_converter/target \
	    tools/wasm_range_probe/target baserom_us.iso; do \
	    if [ -e "$$path" ]; then du -sh "$$path"; fi; \
	done

distclean: clean
	rm -rf .tools

.PHONY: all clean clean-cache clean-history disk-usage distclean
