#ifndef SMG_GAME_H
#define SMG_GAME_H

#include <stdbool.h>
#include <stdint.h>

typedef int32_t smg_fx;

enum {
    SMG_KEY_A = 1u << 0,
    SMG_KEY_B = 1u << 1,
    SMG_KEY_SELECT = 1u << 2,
    SMG_KEY_START = 1u << 3,
    SMG_KEY_RIGHT = 1u << 4,
    SMG_KEY_LEFT = 1u << 5,
    SMG_KEY_UP = 1u << 6,
    SMG_KEY_DOWN = 1u << 7,
    SMG_KEY_R = 1u << 8,
    SMG_KEY_L = 1u << 9,
};

void smg_game_init(void);
uint32_t smg_game_texture_cache_bytes(void);
void smg_visibility_init(void);
/* Optional copy of the depth-bin lookup into caller-owned fast memory. */
bool smg_visibility_set_zbin_storage(void* memory, uint32_t bytes);
void smg_game_set_texture_cache(void* memory, uint32_t bytes);
void smg_game_restore_texture_cache(void);
/* Preferred 32 KiB; setter also accepts the 16 KiB fallback. */
uint32_t smg_game_vertex_cache_bytes(void);
uint32_t smg_game_vertex_cache_min_bytes(void);
void smg_game_set_vertex_cache(void* memory, uint32_t bytes);
void smg_game_invalidate_vertex_cache(void);
uint32_t smg_game_visibility_cache_bytes(void);
void smg_game_set_visibility_cache(void* memory, uint32_t bytes);
void smg_game_restore_visibility_cache(void);
unsigned smg_game_get_view_mode(void);
void smg_game_set_view_mode(unsigned mode);
bool smg_game_get_depth_buffer(void);
void smg_game_set_depth_buffer(bool enabled);
bool smg_game_get_perspective(void);
void smg_game_set_perspective(bool enabled);
/* Indexed buffer uses FRAME_STRIDE; simulation advances on skipped draws too. */
bool smg_game_frame(uint16_t buttons, smg_fx dt, uint8_t* framebuffer, bool render);
void smg_game_get_palette(uint32_t out_rgb888[256]);
void smg_game_render_ui(uint16_t* rgb565, unsigned stride);
bool smg_xip_load(void);

uint32_t smg_game_home_cache_bytes(void);
void smg_game_set_home_cache(void* memory, uint32_t bytes);
void smg_game_restore_home_cache(void);

#endif
