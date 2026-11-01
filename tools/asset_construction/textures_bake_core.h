#ifndef SMG_TEXTURES_BAKE_CORE_H
#define SMG_TEXTURES_BAKE_CORE_H

#include <stddef.h>

/* In-memory entry point for the geometry-backed texture renderer. The caller
 * owns input and must copy output before calling reset. */
void textures_bake_core_set_arena(unsigned char *arena, size_t arena_size);
int textures_bake_core_run(const unsigned char *input, size_t input_size);
const unsigned char *textures_bake_core_output(void);
size_t textures_bake_core_output_len(void);
void textures_bake_core_reset(void);

#endif
