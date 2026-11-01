#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

#include "common.h"
#include "odroid_overlay.h"
#include "gw_firmware_abi.h"

uint8_t *odroid_overlay_cache_file_in_flash_relocate(
    const char *file_path, uint32_t *file_size_p, bool byte_swap,
    gw_flash_relocate_cb_t relocate_cb);

#ifndef HOST_BUILD
#include "gw_core_bridge.h"
#endif

#define SMG_XIP_PATH "/homebrews/smg_code.xip"
#define SMG_ASSET_PATH "/homebrews/smg_assets.xip"
#define SMG_XIP_LINK_BASE 0xDEC00000u
#define SMG_ASSET_LINK_BASE 0xBEEF0000u
#define SMG_XIP_MAX_SIZE (16u * 1024u * 1024u)
#define SMG_ASSET_MAX_SIZE (64u * 1024u * 1024u)

extern uint8_t __RAM_EMU_START__[];
extern uint8_t __ITCM_CORE_START__[], __CORE_ITCM_CODE_END__[];
extern uint32_t __smg_data_relocs_start__[], __smg_data_relocs_end__[];
extern uint32_t __smg_code_relocs_start__[], __smg_code_relocs_end__[];
extern uint32_t __smg_ram_relocs_start__[], __smg_ram_relocs_end__[];
extern uint32_t __smg_code_self_relocs_start__[], __smg_code_self_relocs_end__[];
extern uint32_t __smg_ram_code_relocs_start__[], __smg_ram_code_relocs_end__[];

/* GWProv uses these conventional names to rebase symbols in the two mapped
 * ELF sections while debugging the runtime-cached sidecars. */
const uint8_t *smg_assets_xip_runtime_base;
const uint8_t *smg_code_xip_runtime_base;
static uint32_t smg_code_size;
static uint32_t smg_asset_data_size;

/* Exact relocation sites may reside in either loaded code segment. ITCM's
 * SDK origin is legitimately zero; test integer bounds, never !base. */
static bool smg_loaded_reloc_site(uint32_t site)
{
    if (site & 3u) return false;
    uintptr_t smg_ram_begin = (uintptr_t)__RAM_EMU_START__;
    uintptr_t smg_ram_limit = (uintptr_t)&__CORE_CODE_END__;
    uintptr_t smg_itcm_begin = (uintptr_t)__ITCM_CORE_START__;
    uintptr_t smg_itcm_limit = (uintptr_t)__CORE_ITCM_CODE_END__;
    return (smg_ram_limit >= smg_ram_begin + 4 && site >= smg_ram_begin && site <= smg_ram_limit - 4) ||
           (smg_itcm_limit >= smg_itcm_begin + 4 && site >= smg_itcm_begin && site <= smg_itcm_limit - 4);
}

static bool smg_rebase_word(uint32_t *word, uint32_t base, uint32_t size,
                            int32_t delta)
{
    uint32_t value = *word;
    uint32_t bare = value & ~1u;
    if (bare < base || bare >= base + size)
        return false;
    *word = (uint32_t)(value + (uint32_t)delta);
    return true;
}

static void smg_patch_asset_reloc_sites(uint8_t *buffer, uint32_t length,
                                        uint32_t offset_in_file, uint32_t site_base,
                                        uint32_t file_size, uint32_t target_base,
                                        uint32_t target_size, uint8_t *target_address,
                                        const uint32_t *sites_start,
                                        const uint32_t *sites_end)
{
    if (file_size < sizeof(uint32_t))
        return;
    uint32_t chunk_end = offset_in_file + length;
    int32_t delta = (int32_t)((uint32_t)(uintptr_t)target_address - target_base);
    for (const uint32_t *site = sites_start; site < sites_end; ++site) {
        if (*site < site_base || *site - site_base > file_size - sizeof(uint32_t))
            continue;
        uint32_t offset = *site - site_base;
        if (offset < offset_in_file || offset + sizeof(uint32_t) > chunk_end)
            continue;
        uint32_t *word = (uint32_t *)(buffer + offset - offset_in_file);
        smg_rebase_word(word, target_base, target_size, delta);
    }
}

/* The raw asset sidecar has no relocBase because it contains opaque bytes.
 * Its internal ABS32 pointers are patched only at linker-recorded locations. */
static void smg_relocate_asset_chunk(uint8_t *buffer, uint32_t length,
                                     uint32_t offset_in_file,
                                     uint8_t *file_address, uint32_t file_size)
{
    smg_patch_asset_reloc_sites(buffer, length, offset_in_file,
                                SMG_ASSET_LINK_BASE, file_size,
                                SMG_ASSET_LINK_BASE, file_size, file_address,
                                __smg_data_relocs_start__, __smg_data_relocs_end__);
    if ((offset_in_file & 0xfffu) == 0)
        wdog_refresh();
}

/* Code and rodata share this sidecar. Scanning its words would also relocate
 * coincidental values in the sine, pose, and image tables. Patch only ABS32
 * references, including references from code into the asset sidecar. */
static void smg_relocate_code_chunk(uint8_t *buffer, uint32_t length,
                                    uint32_t offset_in_file,
                                    uint8_t *file_address, uint32_t file_size)
{
    smg_patch_asset_reloc_sites(buffer, length, offset_in_file,
                                SMG_XIP_LINK_BASE, file_size,
                                SMG_XIP_LINK_BASE, file_size, file_address,
                                __smg_code_self_relocs_start__,
                                __smg_code_self_relocs_end__);
    smg_patch_asset_reloc_sites(buffer, length, offset_in_file,
                                SMG_XIP_LINK_BASE, file_size,
                                SMG_ASSET_LINK_BASE, smg_asset_data_size,
                                (uint8_t *)smg_assets_xip_runtime_base,
                                __smg_code_relocs_start__, __smg_code_relocs_end__);
    if ((offset_in_file & 0xfffu) == 0)
        wdog_refresh();
}

bool smg_xip_load(void)
{
    uint32_t size = 0;
    uint8_t *mapped = odroid_overlay_cache_file_in_flash_relocate(
        SMG_ASSET_PATH, &size, false, smg_relocate_asset_chunk);
    if (!mapped || size == 0 || size > SMG_ASSET_MAX_SIZE) {
        printf("smg: missing or invalid asset sidecar %s (%lu bytes)\n",
               SMG_ASSET_PATH, (unsigned long)size);
        return false;
    }
    smg_assets_xip_runtime_base = mapped;
    smg_asset_data_size = size;
    for (const uint32_t *site = __smg_data_relocs_start__;
         site < __smg_data_relocs_end__; ++site) {
        if (*site < SMG_ASSET_LINK_BASE ||
            *site - SMG_ASSET_LINK_BASE > size - sizeof(uint32_t))
            continue;
        uint32_t value = *(uint32_t *)(mapped + (*site - SMG_ASSET_LINK_BASE));
        uint32_t bare = value & ~1u;
        if (bare >= SMG_ASSET_LINK_BASE && bare < SMG_ASSET_LINK_BASE + size) {
            printf("smg: unrelocated asset pointer at +%lu\n",
                   (unsigned long)(*site - SMG_ASSET_LINK_BASE));
            return false;
        }
    }

    size = 0;
    mapped = odroid_overlay_cache_file_in_flash_relocate(
        SMG_XIP_PATH, &size, false, smg_relocate_code_chunk);
    if (!mapped || size == 0 || size > SMG_XIP_MAX_SIZE) {
        printf("smg: missing or invalid code sidecar %s (%lu bytes)\n",
               SMG_XIP_PATH, (unsigned long)size);
        return false;
    }
    smg_code_xip_runtime_base = mapped;

    /* Validate pointer sites only; rodata may contain legitimate values in
     * either sentinel range. */
    for (const uint32_t *site = __smg_code_self_relocs_start__;
         site < __smg_code_self_relocs_end__; ++site) {
        if (*site < SMG_XIP_LINK_BASE || *site - SMG_XIP_LINK_BASE > size - 4u)
            continue;
        uint32_t bare = *(uint32_t *)(mapped + (*site - SMG_XIP_LINK_BASE)) & ~1u;
        bool code_sentinel = bare >= SMG_XIP_LINK_BASE &&
                             bare < SMG_XIP_LINK_BASE + size;
        if (code_sentinel) {
            printf("smg: unrelocated code word at +%lu\n",
                   (unsigned long)(*site - SMG_XIP_LINK_BASE));
            return false;
        }
    }
    for (const uint32_t *site = __smg_code_relocs_start__;
         site < __smg_code_relocs_end__; ++site) {
        if (*site < SMG_XIP_LINK_BASE || *site - SMG_XIP_LINK_BASE > size - 4u)
            continue;
        uint32_t value = *(uint32_t *)(mapped + (*site - SMG_XIP_LINK_BASE));
        uint32_t bare = value & ~1u;
        if (bare >= SMG_ASSET_LINK_BASE &&
            bare < SMG_ASSET_LINK_BASE + smg_asset_data_size) {
            printf("smg: unrelocated asset reference at code +%lu\n",
                   (unsigned long)(*site - SMG_XIP_LINK_BASE));
            return false;
        }
    }

    smg_code_size = size;
    int32_t delta = (int32_t)((uint32_t)(uintptr_t)mapped - SMG_XIP_LINK_BASE);
    uint32_t patched = 0;

    for (const uint32_t *site = __smg_ram_code_relocs_start__;
         site < __smg_ram_code_relocs_end__; ++site) {
        if (!smg_loaded_reloc_site(*site)) {
            printf("smg: invalid loaded code relocation site %08lx\n", (unsigned long)*site);
            return false;
        }
        patched += smg_rebase_word((uint32_t *)(uintptr_t)*site,
                                   SMG_XIP_LINK_BASE, size, delta) ? 1u : 0u;
    }

    int32_t asset_delta = (int32_t)((uint32_t)(uintptr_t)smg_assets_xip_runtime_base -
                                    SMG_ASSET_LINK_BASE);
    for (const uint32_t *site = __smg_ram_relocs_start__;
         site < __smg_ram_relocs_end__; ++site) {
        if (!smg_loaded_reloc_site(*site)) {
            printf("smg: invalid loaded asset relocation site %08lx\n", (unsigned long)*site);
            return false;
        }
        patched += smg_rebase_word((uint32_t *)(uintptr_t)*site,
                                   SMG_ASSET_LINK_BASE,
                                   smg_asset_data_size, asset_delta) ? 1u : 0u;
    }

#ifndef HOST_BUILD
    __asm__ volatile("dsb 0xf\nisb 0xf" ::: "memory");
#endif
    printf("smg: code XIP %lu bytes @ %p; rebased %lu loaded references\n",
           (unsigned long)smg_code_size, (void *)mapped,
           (unsigned long)patched);
    if (patched == 0)
        return false;

    printf("smg: raw assets %lu bytes @ %p\n",
           (unsigned long)smg_asset_data_size,
           (const void *)smg_assets_xip_runtime_base);
    return true;
}
