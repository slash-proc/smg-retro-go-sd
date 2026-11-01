#if defined(__GNW__)
/* Retain the table as a debug A/B reference until hardware timing is available. */
static volatile bool g_projection_divide_enabled = true;
#endif

IWRAM_CODE static inline v2 project_to_screen(v3 p){
    uint32_t zi = (uint32_t)(p.z > FXC(0.5f) ? p.z : FXC(0.5f)) >> 2;
    if(zi > 0xFFFF) zi = 0xFFFF;
#if defined(__GNW__)
    /* Every reachable table entry is exactly floor(2^20 / zi). Keep the
     * existing z clamp, product shift and screen scaling/rounding unchanged. */
    int32_t r = g_projection_divide_enabled ? (int32_t)((UINT32_C(1) << 20) / zi) : lut_project[zi];
#else
    int32_t r = lut_project[zi];
#endif
    return V2(((r * p.x) >> 14) * (FRAME_WIDTH / 2) + (FRAME_WIDTH / 2) * FX_ONE,
              ((r * p.y) >> 14) * (FRAME_HEIGHT / 2) + (FRAME_HEIGHT / 2) * FX_ONE);
}
IWRAM_CODE static inline uint32_t screen_pack_position(v2 s){
    return ((uint32_t)(s.x >> 4) & 0xFFFF) | (uint32_t)(s.y >> 4) << 16;
}
static inline int32_t screen_x(uint32_t s){ return (int32_t)(s << 16) >> 16; }
static inline int32_t screen_y(uint32_t s){ return (int32_t)s >> 16; }

static inline uint8_t frame_get(const uint8_t* fb, int x, int y){ return fb[y * FRAME_STRIDE + x]; }
static inline void frame_put(uint8_t* fb, int x, int y, unsigned colour){
    uintptr_t at = (uintptr_t)(fb + y * FRAME_STRIDE + x);
    volatile uint16_t* h = (volatile uint16_t*)(at & ~(uintptr_t)1);
    *h = (uint16_t)(at & 1 ? (*h & 0x00FF) | colour << 8 : (*h & 0xFF00) | colour);
}
