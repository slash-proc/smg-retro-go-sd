/*
 * Retro-Go SD template — minimal skeleton for a CORE or a GWHB homebrew.
 *
 * Select the kind at build time:
 *   make PROJECT_KIND=core      (default)  → ROM loader + cheat hooks + footer logos
 *   make PROJECT_KIND=homebrew             → no ROM load; ACTIVE_FILE is this .bin
 *
 * Shows path/size (cores) or the GWHB name (homebrews) and which buttons are held.
 * Holding a gameplay button plays a square-wave beep (audio path demo).
 * Save/load/screenshot hooks are stubs — fill them when you plug in real logic.
 *
 * Also demonstrates:
 *   - Pause-menu game options via odroid_dialog_choice_t
 *   - Per-core string tables with gw_i18n() (firmware language)
 *   - Cheat Codes entry (cores only: pack --cheat-ext + cheat_update_cb)
 *   - Optional shutdown / sleep-wake / SRAM-save hooks
 *
 * Entry (run_dynamic_core / run_gwhb_homebrew):
 *   void app_main(uint8_t load_state, uint8_t start_paused, int8_t save_slot)
 */

#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

#include "common.h"
#include "gw_lcd.h"
#include "gw_audio.h"
#include "rom_manager.h"
#include "odroid_system.h"
#include "appid.h"
#include "odroid_overlay.h"
#include "odroid_settings.h"
#include "gw_malloc.h"
#if defined(SMG_GAME_RUNTIME)
#include "smg_game.h"
#include "graphics/frame_dimensions.h"
#include "graphics/perf.h"
#endif

#ifndef HOST_BUILD
#include "gw_core_bridge.h"
#include "gw_core_i18n.h"
#else
#include "host_compat.h"
#include "gw_core_i18n.h"
#endif

#if defined(PROJECT_KIND_HOMEBREW)
#define APP_ID  APPID_HOMEBREW
#elif defined(PROJECT_KIND_CORE)
#define APP_ID  APPID_CORE
#else
#error "Build with PROJECT_KIND=core or PROJECT_KIND=homebrew"
#endif

#define FPS          60
#define SAMPLE_RATE  16000
#define AUDIO_LENGTH (SAMPLE_RATE / FPS)

#ifndef MAX_CHEAT_CODES
#define MAX_CHEAT_CODES 13
#endif

#if defined(PROJECT_KIND_CORE)
static const uint8_t *rom_data;
static uint32_t rom_size;
static bool rom_in_ram;
static int cheats_on; /* count of enabled cheat slots */
#endif

static uint32_t frame;
static odroid_gamepad_state_t pad; /* last read — used by blit() / audio */
static uint32_t audio_phase;       /* 16.16 phase for square-wave demo */
#if defined(SMG_GAME_RUNTIME)
static uint8_t smg_framebuffer[320 * 240];
static void* smg_home_allocation;
static void smg_allocate_home_cache(void);
static uint32_t smg_palette[256];
static uint16_t smg_palette_565[256];
static bool smg_full_res;
static uint16_t smg_dt_remainder;
static uint32_t smg_last_step_ms;
static unsigned smg_target_fps = 30;
static unsigned smg_applied_fps;
static uint32_t smg_clock_hz = 280000000;
static char smg_fps_value[8];
static char smg_detail_value[8];
static char smg_depth_value[4];
static char smg_mapping_value[9];
static const char *const smg_detail_names[] = {"Full", "High", "Default", "Near", "Short"};
uint32_t smg_perf_frame_cycles[SMG_PERF_STAGE_COUNT];
SmgGeometryStats smg_geometry_stats = {.version = 2};

/* Passive debug ABI. Cycles include interrupts; nested world sub-stages are
 * deliberately overlapping. seq protects the short aggregate commit. */
volatile struct {
    uint32_t version, seq, frames, target_fps;
    uint64_t cycles[SMG_PERF_STAGE_COUNT];
    uint64_t work_cycles;
    uint32_t max_work_cycles, over_30, over_60;
    uint32_t histogram[16]; /* work time in 2 ms bins; final bin >=30 ms */
    uint32_t ring_index;
    uint32_t recent_work_cycles[256];
} smg_perf = {.version = 1};

static void smg_perf_commit(uint32_t work)
{
    smg_perf.seq++;
    for (unsigned i = 0; i < SMG_PERF_STAGE_COUNT; i++)
        smg_perf.cycles[i] += smg_perf_frame_cycles[i];
    smg_perf.work_cycles += work;
    if (work > smg_perf.max_work_cycles) smg_perf.max_work_cycles = work;
    if (work > smg_clock_hz / 30) smg_perf.over_30++;
    if (work > smg_clock_hz / 60) smg_perf.over_60++;
    unsigned bin = work / (smg_clock_hz / 500);
    smg_perf.histogram[bin < 16 ? bin : 15]++;
    smg_perf.recent_work_cycles[smg_perf.ring_index++ & 255] = work;
    smg_perf.target_fps = smg_target_fps;
    smg_perf.frames++;
    smg_perf.seq++;
}

/* Cumulative counters for passive hardware timing through the debug console.
 * game_ms includes simulation and world rendering; blit_ms includes the HUD. */
volatile struct {
    uint32_t frames, game_ms, blit_ms;
    uint32_t raster_width, raster_height;
} smg_render_stats = {0, 0, 0, FRAME_WIDTH, FRAME_HEIGHT};

/* Read-only allocator queries, published once at startup and on debugger
 * request. Values are largest allocatable spans after firmware reservations,
 * not pool totals. Never reset an allocator to obtain these measurements. */
volatile uint32_t smg_memory_request = 1;
volatile struct {
    uint32_t version, seq, request, frames;
    uint32_t ram_emu, itcm, dtcm, ahb;
} smg_memory_stats = {.version = 1};

static void smg_memory_sample(void)
{
    uint32_t request = smg_memory_request;
    if (request == smg_memory_stats.request) return;
    smg_memory_stats.seq++;
    smg_memory_stats.frames = smg_render_stats.frames;
    smg_memory_stats.ram_emu = ram_get_free_size();
    smg_memory_stats.itcm = itc_get_free_size();
    smg_memory_stats.dtcm = dtc_get_free_size();
    smg_memory_stats.ahb = ahb_get_free_size();
    smg_memory_stats.request = request;
    smg_memory_stats.seq++;
}

static void smg_palette_sync(void)
{
    smg_game_get_palette(smg_palette);
    for (unsigned i = 0; i < 256; i++) {
        unsigned r = (smg_palette[i] >> 16) & 255;
        unsigned g = (smg_palette[i] >> 8) & 255;
        unsigned b = smg_palette[i] & 255;
        smg_palette_565[i] = (uint16_t)(((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3));
    }
}

static uint16_t smg_buttons(void)
{
    uint16_t buttons = 0;
    if (pad.values[ODROID_INPUT_A]) buttons |= SMG_KEY_A;
    if (pad.values[ODROID_INPUT_B]) buttons |= SMG_KEY_B;
    /* The G&W port maps its physical Start/Select buttons to X/Y. GAME/TIME
     * are firmware controls and must not be treated as in-game Start/Select. */
    if (pad.values[ODROID_INPUT_Y]) buttons |= SMG_KEY_SELECT;
    if (pad.values[ODROID_INPUT_X]) buttons |= SMG_KEY_START;
    if (pad.values[ODROID_INPUT_RIGHT]) buttons |= SMG_KEY_RIGHT;
    if (pad.values[ODROID_INPUT_LEFT]) buttons |= SMG_KEY_LEFT;
    if (pad.values[ODROID_INPUT_UP]) buttons |= SMG_KEY_UP;
    if (pad.values[ODROID_INPUT_DOWN]) buttons |= SMG_KEY_DOWN;
    return buttons;
}

static smg_fx smg_frame_dt(uint32_t menu_ms)
{
    uint32_t now = get_elapsed_time();
    uint32_t elapsed = smg_last_step_ms ? now - smg_last_step_ms - menu_ms : 1000 / smg_target_fps;
    smg_last_step_ms = now;
    if (elapsed > 100) elapsed = 100; /* bounded catch-up after an external stall */
    unsigned total = smg_dt_remainder + elapsed * 256;
    smg_dt_remainder = total % 1000;
    return (smg_fx)(total / 1000);
}

static bool smg_fps_update_cb(odroid_dialog_choice_t *option,
                              odroid_dialog_event_t event, uint32_t repeat)
{
    (void)repeat;
    if (event == ODROID_DIALOG_PREV || event == ODROID_DIALOG_NEXT) {
        smg_target_fps = smg_target_fps == 30 ? 60 : 30;
        odroid_settings_app_int32_set("render_fps", smg_target_fps);
    }
    snprintf(option->value, sizeof smg_fps_value, "%u FPS", smg_target_fps);
    return event == ODROID_DIALOG_ENTER;
}

static bool smg_detail_update_cb(odroid_dialog_choice_t *option,
                                odroid_dialog_event_t event, uint32_t repeat)
{
    (void)repeat;
    unsigned mode = smg_game_get_view_mode();
    unsigned count = sizeof smg_detail_names / sizeof *smg_detail_names;
    if (event == ODROID_DIALOG_PREV || event == ODROID_DIALOG_NEXT) {
        mode = (mode + (event == ODROID_DIALOG_PREV ? count - 1 : 1)) % count;
        smg_game_set_view_mode(mode);
        odroid_settings_app_int32_set("scene_detail", mode);
    }
    snprintf(option->value, sizeof smg_detail_value, "%s", smg_detail_names[mode]);
    return event == ODROID_DIALOG_ENTER;
}

static bool smg_depth_update_cb(odroid_dialog_choice_t *option,
                               odroid_dialog_event_t event, uint32_t repeat)
{
    (void)repeat;
    bool enabled = smg_game_get_depth_buffer();
    if (event == ODROID_DIALOG_PREV || event == ODROID_DIALOG_NEXT) {
        enabled = !enabled;
        smg_game_set_depth_buffer(enabled);
        odroid_settings_app_int32_set("depth_buffer", enabled);
    }
    snprintf(option->value, sizeof smg_depth_value, "%s", enabled ? "On" : "Off");
    return event == ODROID_DIALOG_ENTER;
}

static bool smg_mapping_update_cb(odroid_dialog_choice_t *option,
                                 odroid_dialog_event_t event, uint32_t repeat)
{
    (void)repeat;
    bool enabled = smg_game_get_perspective();
    if (event == ODROID_DIALOG_PREV || event == ODROID_DIALOG_NEXT) {
        enabled = !enabled;
        smg_game_set_perspective(enabled);
        odroid_settings_app_int32_set("tex_mapping", enabled);
    }
    snprintf(option->value, sizeof smg_mapping_value, "%s", enabled ? "Accurate" : "Fast");
    return event == ODROID_DIALOG_ENTER;
}

static void smg_apply_frame_rate(void)
{
    if (smg_applied_fps == smg_target_fps) return;
    audio_stop_playing();
    common_emu_state.frame_time_10us = 100000 / smg_target_fps;
    audio_start_playing(SAMPLE_RATE / smg_target_fps);
    unsigned pause_after_frames = common_emu_state.pause_after_frames;
    common_emu_frame_loop_reset();
    common_emu_state.pause_after_frames = pause_after_frames;
    smg_applied_fps = smg_target_fps;
}
#endif

/* Pause-menu demo option (persisted per app id via settings). */
static int beep_enabled = 1;
static char beep_value[8];

static void blit(void);

/* --- i18n tables (English required; others optional) ---------------------- */

static const gw_i18n_entry_t i18n_title[] = {
#if defined(PROJECT_KIND_HOMEBREW)
    { "en", "SMG port bring-up" },
    { "fr", "SMG port bring-up" },
    { "es", "SMG port bring-up" },
    { "de", "SMG port bring-up" },
#else
    { "en", "Example core" },
    { "fr", "Core exemple" },
    { "es", "Núcleo de ejemplo" },
    { "de", "Beispiel-Core" },
#endif
    GW_I18N_END
};

static const gw_i18n_entry_t i18n_hold_beep[] = {
    { "en", "Hold a button for a beep:" },
    { "fr", "Maintenir un bouton pour un bip :" },
    { "es", "Mantén un botón para un pitido:" },
    { "de", "Taste halten für Piepton:" },
    GW_I18N_END
};

static const gw_i18n_entry_t i18n_beep[] = {
    { "en", "Button beep" },
    { "fr", "Bip boutons" },
    { "es", "Pitido" },
    { "de", "Tasten-Piep" },
    GW_I18N_END
};

static const gw_i18n_entry_t i18n_on[] = {
    { "en", "ON" },
    { "fr", "OUI" },
    { "es", "SÍ" },
    { "de", "AN" },
    GW_I18N_END
};

static const gw_i18n_entry_t i18n_off[] = {
    { "en", "OFF" },
    { "fr", "NON" },
    { "es", "NO" },
    { "de", "AUS" },
    GW_I18N_END
};

#if defined(PROJECT_KIND_CORE) && CHEAT_CODES == 1
static const gw_i18n_entry_t i18n_cheats[] = {
    { "en", "Cheats on" },
    { "fr", "Cheats actifs" },
    { "es", "Trampas activas" },
    { "de", "Cheats an" },
    GW_I18N_END
};
#endif

static void beep_value_sync(void)
{
    strncpy(beep_value,
            beep_enabled ? gw_i18n(i18n_on) : gw_i18n(i18n_off),
            sizeof(beep_value) - 1);
    beep_value[sizeof(beep_value) - 1] = '\0';
}

static bool beep_update_cb(odroid_dialog_choice_t *option,
                           odroid_dialog_event_t event, uint32_t repeat)
{
    (void)repeat;
    if (event == ODROID_DIALOG_PREV || event == ODROID_DIALOG_NEXT) {
        beep_enabled = !beep_enabled;
        odroid_settings_app_int32_set("beep", beep_enabled);
    }
    beep_value_sync();
    strcpy(option->value, beep_value);
    return event == ODROID_DIALOG_ENTER;
}

/* Hz per button — first match wins (A over B over D-pad…). */
static uint16_t tone_hz(void)
{
    if (!beep_enabled)
        return 0;
    if (pad.values[ODROID_INPUT_A])     return 440;  /* A4 */
    if (pad.values[ODROID_INPUT_B])     return 523;  /* C5 */
#if defined(PROJECT_KIND_CORE)
    if (pad.values[ODROID_INPUT_X])     return 659;  /* E5  (START) */
    if (pad.values[ODROID_INPUT_Y])     return 784;  /* G5  (SELECT) */
#endif
    if (pad.values[ODROID_INPUT_UP])    return 330;
    if (pad.values[ODROID_INPUT_DOWN])  return 294;
    if (pad.values[ODROID_INPUT_LEFT])  return 262;
    if (pad.values[ODROID_INPUT_RIGHT]) return 349;
    return 0;
}

#if defined(PROJECT_KIND_CORE)
/* --- ROM: RAM if it fits, else QSPI flash (same policy as other cores) --- */

static bool load_rom(void)
{
    uint32_t size;
    uint8_t *dest;

    if (!ACTIVE_FILE || !ACTIVE_FILE->path[0]) {
        printf("smg: no ACTIVE_FILE\n");
        return false;
    }

    size = ACTIVE_FILE->size;

    if (size > 0 && size <= ram_get_free_size()) {
        dest = ram_malloc(size);
        if (!dest)
            return false;
        if (odroid_overlay_cache_file_in_ram(ACTIVE_FILE->path, dest) != size)
            return false;
        rom_data = dest;
        rom_in_ram = true;
    } else {
        dest = odroid_overlay_cache_file_in_flash(ACTIVE_FILE->path, &size, false);
        if (!dest || size == 0)
            return false;
        rom_data = dest;
        rom_in_ram = false;
    }

    rom_size = size;
    printf("smg: ROM %lu bytes in %s @ %p\n",
           (unsigned long)size, rom_in_ram ? "RAM" : "FLASH", (void *)dest);
    return true;
}
#endif

/* --- System callbacks ----------------------------------------------------- */

static bool LoadState(const char *savePathName)
{
    (void)savePathName;
    /* TODO: fopen(savePathName, "rb"), read your snapshot, apply it.
     * Return true on success so the pause menu can confirm the load. */
    return false;
}

static bool SaveState(const char *savePathName)
{
    (void)savePathName;
    /* TODO: serialize state into a buffer, fwrite to savePathName.
     * Return true on success. */
    return false;
}

static void *Screenshot(void)
{
    /* TODO: wait for vblank, redraw one clean frame into the active LCD
     * buffer (no HUD if you prefer), then return lcd_get_active_buffer().
     * The firmware copies that RGB565 bitmap to the screenshot file. */
    lcd_wait_for_vblank();
    blit();
    return lcd_get_active_buffer();
}

static void Shutdown(void)
{
    /* Called on power-off from the pause menu. Flush config / open files. */
    odroid_settings_app_int32_set("beep", beep_enabled);
#if defined(SMG_GAME_RUNTIME)
    if(smg_home_allocation){
        smg_game_set_home_cache(NULL, 0);
        free(smg_home_allocation);
        smg_home_allocation = NULL;
    }
#endif
}

static void SleepWake(void)
{
#if defined(SMG_GAME_RUNTIME)
    smg_game_invalidate_vertex_cache();
    smg_game_restore_texture_cache();
    smg_game_restore_visibility_cache();
    smg_visibility_init();
    if(!smg_home_allocation) smg_allocate_home_cache();
    smg_game_restore_home_cache();
#endif
    /* After deep sleep the firmware restores clocks; re-arm SAI/DMA at the
     * sample rate so audio does not stay silent or at the wrong pitch. */
    odroid_audio_init(SAMPLE_RATE);
    audio_clear_buffers();
    audio_start_playing(AUDIO_LENGTH);
}

static void SramSave(void)
{
    /* TODO (cores): write battery-backed cart RAM (ODROID_PATH_SAVE_SRAM)
     * when dirty. Called on pause / power paths. Homebrews may ignore. */
}

#if defined(PROJECT_KIND_CORE) && CHEAT_CODES == 1
/* Re-apply enabled codes whenever the user confirms the Cheats submenu.
 * Real cores parse ACTIVE_FILE->cheat_codes[i] into the emulator; here we
 * only count how many slots are on so the HUD can show it. */
static void update_cheats(void)
{
    int n = 0;

    if (ACTIVE_FILE) {
        int i;
        for (i = 0; i < MAX_CHEAT_CODES && i < ACTIVE_FILE->cheat_count; i++) {
            if (odroid_settings_ActiveGameGenieCodes_is_enabled(ACTIVE_FILE->path, i))
                n++;
        }
    }
    cheats_on = n;
    printf("smg: %d cheat slot(s) enabled\n", n);
}
#endif

/* --- Input: after common_emu_input_loop (MENU/VOLUME stay with firmware) -- */

static void input_read(const odroid_gamepad_state_t *joy)
{
    pad = *joy;
}

/* --- Video ---------------------------------------------------------------- */

static void blit(void)
{
#if defined(SMG_GAME_RUNTIME)
    uint16_t *dst = (uint16_t *)lcd_get_active_buffer();
    /* Upscale the world raster, then composite the HUD at native LCD size.
     * Full-res menus retain their original 240x160 coordinate space. */
    unsigned src_width = smg_full_res ? 240 : FRAME_WIDTH;
    unsigned src_height = smg_full_res ? 160 : FRAME_HEIGHT;
    uint32_t started = smg_perf_now();
    if (src_width * 2 == GW_LCD_WIDTH && src_height * 2 == GW_LCD_HEIGHT) {
        for (unsigned y = 0; y < src_height; y++) {
            const uint8_t *src = smg_framebuffer + y * FRAME_STRIDE;
            uint32_t *top = (uint32_t *)(dst + 2 * y * GW_LCD_WIDTH);
            uint32_t *bottom = top + GW_LCD_WIDTH / 2;
            for (unsigned x = 0; x < src_width; x++) {
                uint32_t colour = smg_palette_565[src[x]];
                top[x] = bottom[x] = colour | colour << 16;
            }
        }
    } else for (unsigned y = 0; y < GW_LCD_HEIGHT; y++) {
        const uint8_t *src = smg_framebuffer + (y * src_height / GW_LCD_HEIGHT) * FRAME_STRIDE;
        uint16_t *row = dst + y * GW_LCD_WIDTH;
        for (unsigned x = 0; x < GW_LCD_WIDTH; x++)
            row[x] = smg_palette_565[src[x * src_width / GW_LCD_WIDTH]];
    }
    smg_perf_add(SMG_PERF_SCALE, started);
    started = smg_perf_now();
    smg_game_render_ui(dst, GW_LCD_WIDTH);
    common_ingame_overlay();
    smg_perf_add(SMG_PERF_HUD, started);
#else
    uint16_t *fb = lcd_get_active_buffer();
    char line[80];
    int y = 8;

    memset(fb, 0, WIDTH * HEIGHT * sizeof(uint16_t));

    odroid_overlay_draw_text(8, y, 0, gw_i18n(i18n_title), 0xFFFF, 0x0000);
    y += 20;

    {
        const char *name = (ACTIVE_FILE && ACTIVE_FILE->name[0])
                               ? ACTIVE_FILE->name
#if defined(PROJECT_KIND_HOMEBREW)
                               : "(no file)";
#else
                               : "(no rom)";
#endif
        snprintf(line, sizeof(line), "%.70s", name);
        odroid_overlay_draw_text(8, y, 0, line, 0xFFFF, 0x0000);
        y += 16;
    }

#if defined(PROJECT_KIND_CORE)
    snprintf(line, sizeof(line), "%lu bytes  %s",
             (unsigned long)rom_size, rom_in_ram ? "RAM" : "FLASH");
    odroid_overlay_draw_text(8, y, 0, line, 0xFFFF, 0x0000);
    y += 16;
#endif

    snprintf(line, sizeof(line), "frame %lu", (unsigned long)frame);
    odroid_overlay_draw_text(8, y, 0, line, 0xFFFF, 0x0000);
    y += 16;

#if defined(PROJECT_KIND_CORE) && CHEAT_CODES == 1
    snprintf(line, sizeof(line), "%s: %d", gw_i18n(i18n_cheats), cheats_on);
    odroid_overlay_draw_text(8, y, 0, line, 0xFFFF, 0x0000);
    y += 16;
#endif

    y += 8;
    odroid_overlay_draw_text(8, y, 0, gw_i18n(i18n_hold_beep), 0xFFFF, 0x0000);
    y += 16;

    /* One line listing every currently pressed gameplay button. */
    line[0] = '\0';
    if (pad.values[ODROID_INPUT_UP])     strcat(line, "UP ");
    if (pad.values[ODROID_INPUT_DOWN])   strcat(line, "DOWN ");
    if (pad.values[ODROID_INPUT_LEFT])   strcat(line, "LEFT ");
    if (pad.values[ODROID_INPUT_RIGHT])  strcat(line, "RIGHT ");
    if (pad.values[ODROID_INPUT_A])      strcat(line, "A ");
    if (pad.values[ODROID_INPUT_B])      strcat(line, "B ");
#if defined(PROJECT_KIND_CORE)
    if (pad.values[ODROID_INPUT_X])      strcat(line, "START ");
    if (pad.values[ODROID_INPUT_Y])      strcat(line, "SELECT ");
    if (pad.values[ODROID_INPUT_START])  strcat(line, "GAME ");
    if (pad.values[ODROID_INPUT_SELECT]) strcat(line, "TIME ");
#endif
    if (line[0] == '\0')
        strcpy(line, "(none)");
    odroid_overlay_draw_text(8, y, 0, line, 0xFFFF, 0x0000);

    /* Volume/brightness/turbo/… HUD drawn by the firmware — must run after
     * painting, or a full-framebuffer clear hides it and PAUSE+UP/DOWN
     * look broken. */
    common_ingame_overlay();
#endif
}

/* Fill one DMA half-buffer: silence, or a mono square wave at tone_hz.
 * Real cores write emulator PCM here the same way. */
static void submit_audio(void)
{
    int16_t *buf;
    uint16_t len;
    uint16_t hz;
    uint32_t step;
    int32_t vol;
    uint16_t i;

    if (common_emu_sound_loop_is_muted())
        return;

    buf = audio_get_active_buffer();
    len = audio_get_buffer_length();
    if (!buf || !len)
        return;

#if defined(SMG_GAME_RUNTIME)
    hz = 0; /* Template button tones are not game audio. PCM is still pending. */
#else
    hz = tone_hz();
#endif
    if (hz == 0) {
        memset(buf, 0, len * sizeof(int16_t));
        return;
    }

    /* 16.16 fixed phase so the wave continues cleanly across frames. */
    step = ((uint32_t)hz << 16) / SAMPLE_RATE;
    vol = common_emu_sound_get_volume(); /* 0..255 from the volume menu */

    for (i = 0; i < len; i++) {
        int16_t sample = (audio_phase & 0x8000u) ? 8000 : -8000;
        buf[i] = (int16_t)((sample * vol) / 255);
        audio_phase = (audio_phase + step) & 0xffffu;
    }
}

/* --- Main ----------------------------------------------------------------- */

#if defined(SMG_GAME_RUNTIME)
static void smg_allocate_dtcm_caches(void)
{
    /* Textures keep priority; these CPU-only buffers never serve DMA.
     * The allocator already excludes firmware/stack space. Preserve another
     *16KiB after each request, then use the smaller source cache if needed. */
    const uint32_t reserve = 16 * 1024;
    uint32_t texture_bytes = smg_game_texture_cache_bytes();
    void* texture_memory = dtc_get_free_size() >= texture_bytes + reserve
        ? dtc_malloc(texture_bytes) : NULL;
    smg_game_set_texture_cache(texture_memory, texture_bytes);
    uint32_t vertex_bytes = smg_game_vertex_cache_bytes();
    void* vertex_memory = dtc_get_free_size() >= vertex_bytes + reserve
        ? dtc_malloc(vertex_bytes) : NULL;
    if(!vertex_memory){
        vertex_bytes = smg_game_vertex_cache_min_bytes();
        vertex_memory = dtc_get_free_size() >= vertex_bytes + reserve
            ? dtc_malloc(vertex_bytes) : NULL;
    }
    smg_game_set_vertex_cache(vertex_memory, vertex_bytes);
#if defined(SMG_ZBIN_DTCM) && SMG_ZBIN_DTCM
    /* The depth-bin LUT is read for each emitted stage face. Give it only the
     * DTCM left after texture/vertex caches and preserve the normal 16 KiB
     * firmware/stack safety margin. A failed allocation falls back to XIP. */
    const uint32_t zbin_bytes = 2048;
    void* zbin_memory = dtc_get_free_size() >= zbin_bytes + reserve
        ? dtc_malloc(zbin_bytes) : NULL;
    if(!smg_visibility_set_zbin_storage(zbin_memory, zbin_bytes))
        printf("smg: depth-bin LUT remains in XIP; DTCM free=%lu\n",
               (unsigned long)dtc_get_free_size());
#endif
}
/* End DTCM cache allocation. */

static void smg_allocate_home_cache(void)
{
    const uint32_t bytes = smg_game_home_cache_bytes(), reserve = 16 * 1024;
    void* memory = ahb_get_free_size() >= bytes + reserve ? ahb_malloc(bytes) : NULL;
    /* AHB is a freeable heap. Account for its metadata/fragmentation rather
     * than assuming an allocation reduces the largest free span by bytes. */
    if(memory && ahb_get_free_size() < reserve){
        free(memory);
        memory = NULL;
    }
    smg_home_allocation = memory;
    smg_game_set_home_cache(memory, bytes);
}
/* End AHB home-cache allocation. */
#endif


void app_main(uint8_t load_state, uint8_t start_paused, int8_t save_slot)
{
    odroid_gamepad_state_t joystick;
    odroid_dialog_choice_t options[5];

    gw_core_bridge_init();
    memset(&pad, 0, sizeof(pad));
    audio_phase = 0;
#if defined(PROJECT_KIND_CORE)
    cheats_on = 0;
#endif

    if (start_paused) {
        common_emu_state.pause_after_frames = 2;
        odroid_audio_mute(true);
    } else {
        common_emu_state.pause_after_frames = 0;
    }
    common_emu_state.frame_time_10us = (uint16_t)(100000 / FPS + 0.5f);
    lcd_set_refresh_rate(FPS);

    odroid_system_init(APP_ID, SAMPLE_RATE);
    odroid_system_emu_init(&LoadState, &SaveState, &Screenshot,
                           &Shutdown, &SleepWake, &SramSave,
#if defined(PROJECT_KIND_CORE) && CHEAT_CODES == 1
                           &update_cheats
#else
                           NULL
#endif
                           );

    /* App-scoped settings need odroid_system_init (sets current app id). */
#if defined(SMG_GAME_RUNTIME)
    smg_target_fps = odroid_settings_app_int32_get("render_fps", 30) == 60 ? 60 : 30;
    options[0] = (odroid_dialog_choice_t){.id = 101, .label = "Render rate",
        .value = smg_fps_value, .enabled = 1, .update_cb = smg_fps_update_cb};
    snprintf(smg_fps_value, sizeof smg_fps_value, "%u FPS", smg_target_fps);
    options[1] = (odroid_dialog_choice_t){.id = 102, .label = "Scene detail",
        .value = smg_detail_value, .enabled = 1, .update_cb = smg_detail_update_cb};
    options[2] = (odroid_dialog_choice_t){.id = 103, .label = "Depth buffer",
        .value = smg_depth_value, .enabled = 1, .update_cb = smg_depth_update_cb};
    options[3] = (odroid_dialog_choice_t){.id = 104, .label = "Texture mapping",
        .value = smg_mapping_value, .enabled = 1, .update_cb = smg_mapping_update_cb};
    options[4] = (odroid_dialog_choice_t)ODROID_DIALOG_CHOICE_LAST;
#else
    beep_enabled = odroid_settings_app_int32_get("beep", 1) ? 1 : 0;
    beep_value_sync();

    /* Game options appear under the pause menu. Labels are looked up once
     * at start — reopen the menu after a language change to refresh. */
    options[0].id = 100;
    options[0].label = gw_i18n(i18n_beep);
    options[0].value = beep_value;
    options[0].enabled = 1;
    options[0].update_cb = &beep_update_cb;
    options[1] = (odroid_dialog_choice_t)ODROID_DIALOG_CHOICE_LAST;
#endif

    audio_start_playing(AUDIO_LENGTH);

#if defined(SMG_GAME_RUNTIME)
    if (!smg_xip_load()) {
        printf("smg: XIP asset sidecar missing or invalid\n");
        odroid_system_switch_app(0);
    }
    smg_visibility_init();
    lcd_setup_framebuffers(LCD_MODE_RGB565);
    smg_allocate_dtcm_caches();
    smg_allocate_home_cache();
    /* Reusable immutable chunk tables, allocated before any data is copied.
     * Lower-memory/native-raster builds keep the original flash descriptors. */
    uint32_t visibility_bytes = smg_game_visibility_cache_bytes();
    /* Keep useful whole-stage caches for smaller stages when larger geometry
     * buffers leave less than the maximum slab. Big stages retain XIP fallback. */
    uint32_t available = ram_get_free_size();
    uint32_t cache_budget = available > 32 * 1024 + 32
        ? (available - 32 * 1024 - 32) & ~31u : 0;
    if(visibility_bytes > cache_budget) visibility_bytes = cache_budget;
    uint32_t visibility_reservation = visibility_bytes + 32;
    void* visibility_raw = visibility_bytes && ram_get_free_size() >= visibility_reservation + 32 * 1024
        ? ram_malloc(visibility_reservation) : NULL;
    /* Each32B descriptor then occupies one M7 data-cache line. */
    void* visibility_memory = visibility_raw
        ? (void*)(((uintptr_t)visibility_raw + 31) & ~(uintptr_t)31) : NULL;
    smg_game_set_visibility_cache(visibility_memory, visibility_bytes);
    smg_game_init();
    bool depth = odroid_settings_app_int32_get("depth_buffer", 1) != 0;
    smg_game_set_depth_buffer(depth);
    snprintf(smg_depth_value, sizeof smg_depth_value, "%s", depth ? "On" : "Off");
    bool mapping = odroid_settings_app_int32_get("tex_mapping", 1) != 0;
    smg_game_set_perspective(mapping);
    snprintf(smg_mapping_value, sizeof smg_mapping_value, "%s", mapping ? "Accurate" : "Fast");
    int detail = odroid_settings_app_int32_get("scene_detail", 1);
    if (detail < 0 || detail >= (int)(sizeof smg_detail_names / sizeof *smg_detail_names)) detail = 1;
    smg_game_set_view_mode((unsigned)detail);
    snprintf(smg_detail_value, sizeof smg_detail_value, "%s", smg_detail_names[detail]);
    smg_palette_sync();
    if (gw_firmware_abi()->size >= offsetof(gw_firmware_abi_t, get_SystemCoreClock) + sizeof(void *))
        smg_clock_hz = gw_firmware_abi()->get_SystemCoreClock();
    smg_apply_frame_rate();
#endif

#if defined(PROJECT_KIND_CORE)
    if (!load_rom()) {
        rom_data = NULL;
        rom_size = 0;
    }

#if CHEAT_CODES == 1
    /* Apply any slots already enabled for this ROM (resume / prior session). */
    update_cheats();
#endif
#endif

    if (load_state) {
        /* When LoadState is implemented, this applies the chosen slot. */
        odroid_system_emu_load_state(save_slot);
    } else {
        lcd_clear_buffers();
    }

#if defined(SMG_GAME_RUNTIME)
    smg_memory_sample();
#endif

    while (1) {
        wdog_refresh();

        bool draw_frame = common_emu_frame_loop();

        odroid_input_read_gamepad(&joystick);
        uint32_t menu_started = get_elapsed_time();
        common_emu_input_loop(&joystick, options, &blit);
        uint32_t menu_ms = get_elapsed_time_since(menu_started);
        common_emu_input_loop_handle_turbo(&joystick);

        input_read(&joystick);

        frame++;

#if defined(SMG_GAME_RUNTIME)
        smg_apply_frame_rate();
        memset(smg_perf_frame_cycles, 0, sizeof smg_perf_frame_cycles);
        uint32_t work_started = smg_perf_now();
        uint32_t game_started = get_elapsed_time();
        smg_full_res = smg_game_frame(smg_buttons(), smg_frame_dt(menu_ms), smg_framebuffer, draw_frame);
        if (draw_frame) {
            uint32_t started = get_elapsed_time();
            uint32_t palette_started = smg_perf_now();
            smg_palette_sync();
            smg_perf_add(SMG_PERF_PALETTE, palette_started);
            smg_render_stats.game_ms += get_elapsed_time_since(game_started);
            (void)started;
            started = get_elapsed_time();
#else
        if (draw_frame) {
#endif
            blit();
#if defined(SMG_GAME_RUNTIME)
            smg_render_stats.blit_ms += get_elapsed_time_since(started);
            smg_render_stats.frames++;
            smg_perf_commit(smg_perf_now() - work_started);
#endif
            lcd_swap();
        }

        submit_audio();
        common_emu_sound_sync(false);
#if defined(SMG_GAME_RUNTIME)
        smg_memory_sample();
#endif
    }
}
