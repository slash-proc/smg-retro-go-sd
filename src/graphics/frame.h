#include "frame_dimensions.h"
#define VIEW_CLIP_X  1648
#define VIEW_CLIP_Y  2472

IWRAM_CODE static inline v2 project_to_screen(v3 p);
IWRAM_CODE static inline uint32_t screen_pack_position(v2 s);
static inline int32_t screen_x(uint32_t s);
static inline int32_t screen_y(uint32_t s);
static inline uint8_t frame_get(const uint8_t* fb, int x, int y);
static inline void frame_put(uint8_t* fb, int x, int y, unsigned colour);
