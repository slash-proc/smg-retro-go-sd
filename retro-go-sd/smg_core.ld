/*
 * SMG GWHB layout: game code and opaque generated assets are separate XIP
 * sidecars. Code uses the dist-spec relocBase; asset bytes have no relocBase
 * and are patched only at linker-recorded ABS32 reference locations.
 */
INCLUDE sdk/ld/gnw_ram_emu.ld
INCLUDE sdk/ld/gnw_itcm_core.ld

MEMORY
{
  RAM_EMU   (xrw) : ORIGIN = __RAM_EMU_START__, LENGTH = __RAM_EMU_LENGTH__
  ITCM      (xrw) : ORIGIN = __ITCM_CORE_START__, LENGTH = __ITCM_CORE_LENGTH__
  SMG_CODE  (rx)  : ORIGIN = 0xDEC00000, LENGTH = 16M
  SMG_ASSET (r)   : ORIGIN = 0xBEEF0000, LENGTH = 64M
}

PHDRS
{
  text    PT_LOAD FLAGS(5);
  data    PT_LOAD FLAGS(6);
  code    PT_LOAD FLAGS(5);
  assets  PT_LOAD FLAGS(4);
  itcm    PT_LOAD FLAGS(5);
}

SECTIONS
{
  /* Select before the broad AXI .text.engine_* rule consumes this input.
   * An empty placement file leaves the rasterizer in AXI for A/B builds. */
  .core_itcm ORIGIN(ITCM) :
  {
    INCLUDE build/smg/smg_itcm_placement.ld
    . = ALIGN(4);
  } >ITCM :itcm
  __CORE_ITCM_CODE_END__ = ADDR(.core_itcm) + SIZEOF(.core_itcm);
  __CORE_ITCM_BSS_END__ = __CORE_ITCM_CODE_END__;
  ASSERT(SIZEOF(.core_itcm) <= LENGTH(ITCM), "SMG hot code overflows SDK ITCM region")

  .core_entry __RAM_EMU_START__ :
  {
    KEEP(*(.core_entry))
    KEEP(*(.gw_core_bridge_probe))
    . = ALIGN(4);

    /* The sidecar loader and its callbacks must execute from RAM before XIP
     * has been mapped and its references relocated. */
    __SMG_XIP_PATCHER_START__ = .;
    build/smg/smg_xip.o (.text .text.* .rodata .rodata.*)
    __SMG_XIP_PATCHER_END__ = .;

    /* Measured render/visibility hotspots execute from cached AXI RAM. Keep
     * cold game logic and large constant tables in the mapped sidecar. */
    build/smg/main.o (.text.engine_* .text.geometry_* .text.project_to_screen*
                     .text.emit_stage* .text.screen_area* .text.figure_clipped*
                     .text.depth_bin_for_z* .text.draw_scene* .text.draw_figure*
                     .text.clip_* .text.emit_* .text.rank_bits* .text.collect_visible*
                     .text.smg_game_render_ui*)

    EXCLUDE_FILE (build/smg/main.o) *(.text .text*)
    EXCLUDE_FILE (build/smg/main.o build/smg/assets.o) *(.rodata .rodata.*)
    *(.glue_7)
    *(.glue_7t)

    . = ALIGN(4);
    __CORE_INIT_ARRAY_START__ = .;
    KEEP(*(SORT(.init_array.*)))
    KEEP(*(.init_array*))
    __CORE_INIT_ARRAY_END__ = .;

  } >RAM_EMU :text

  .data :
  {
    *(.data .data*)

    /* Keep generated tables after initialized data. Their presence in the
     * final link must not move any sites recorded by the preliminary link. */
    . = ALIGN(4);
    __smg_data_relocs_start__ = .;
    KEEP(*(.smg_asset_relocs.data))
    __smg_data_relocs_end__ = .;
    __smg_code_relocs_start__ = .;
    KEEP(*(.smg_asset_relocs.code))
    __smg_code_relocs_end__ = .;
    __smg_ram_relocs_start__ = .;
    KEEP(*(.smg_asset_relocs.ram))
    __smg_ram_relocs_end__ = .;
    __smg_code_self_relocs_start__ = .;
    KEEP(*(.smg_asset_relocs.code_self))
    __smg_code_self_relocs_end__ = .;
    __smg_ram_code_relocs_start__ = .;
    KEEP(*(.smg_asset_relocs.ram_code))
    __smg_ram_code_relocs_end__ = .;
  } >RAM_EMU :data

  __CORE_CODE_END__ = .;

  .bss (NOLOAD) :
  {
    . = ALIGN(8);
    *(.bss*)
    *(COMMON)
    . = ALIGN(8);
  } >RAM_EMU :data

  __CORE_BSS_END__ = .;
  ASSERT(__CORE_BSS_END__ <= __RAM_EMU_END__,
         "Error: SMG code and BSS overflow RAM_EMU")

  .xip_smg_code ORIGIN(SMG_CODE) :
  {
    . = ALIGN(4);
    __smg_xip_start__ = .;
    build/smg/main.o (.text .text.* .rodata .rodata.*)
    . = ALIGN(4);
    __smg_xip_end__ = .;
  } >SMG_CODE :code
  ASSERT(__smg_xip_end__ - __smg_xip_start__ <= LENGTH(SMG_CODE),
         "Error: SMG code sidecar exceeds its 16 MiB XIP window")

  .xip_smg_assets ORIGIN(SMG_ASSET) :
  {
    . = ALIGN(4);
    __smg_assets_start__ = .;
    build/smg/assets.o (.rodata.assets .rodata.assets.*)
    . = ALIGN(4);
    __smg_assets_end__ = .;
  } >SMG_ASSET :assets
  ASSERT(__smg_assets_end__ - __smg_assets_start__ <= LENGTH(SMG_ASSET),
         "Error: SMG asset sidecar exceeds the 64 MiB mapping window")

  __exidx_start = .;
  __exidx_end = .;

  /DISCARD/ :
  {
    *(.fini_array*)
    *(.preinit_array*)
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.comment)
    *(.ARM.attributes)
  }
}
