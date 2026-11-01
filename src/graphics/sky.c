#if defined(__GNW__)
#include <string.h>
#endif

static inline void dma3_copy16(void* dst, const void* src, uint32_t halfwords){
#if defined(__GNW__)
    memcpy(dst, src, (size_t)halfwords * sizeof(uint16_t));
#else
    REG_DMA3SAD = (uint32_t)src;
    REG_DMA3DAD = (uint32_t)dst;
    REG_DMA3CNT = DMA_ENABLE | halfwords;
#endif
}

IWRAM_FN static void draw_sky(uint8_t* framebuffer, const uint8_t* sky, fx yaw, fx pitch){
    const fx scale = divide_euclidean(divide_euclidean(SKY_DIM << 8, 2), FRAME_HEIGHT);
    int32_t x = fx_mul(fx_mul(yaw, FXC(-4.0f / TAU_F)), SKY_W << 8) >> 8;

    uint32_t col = (uint32_t)(x + (x & 1)) & (SKY_W - 2);
    for(int row = 0; row < FRAME_HEIGHT; row++){
        int32_t y =
            ((fx_mul(-pitch, FXC(0.55f)) * SKY_DIM + fx_mul((row - FRAME_HEIGHT / 2) * FX_ONE, scale)) >> 8) +
            SKY_DIM / 2;
        y &= SKY_DIM - 1;

        const uint8_t* line = sky + y * SKY_W;
        uint8_t* out = framebuffer + row * FRAME_STRIDE;
        /* The128-pixel source row can wrap more than once at either raster
         * width. Never copy its remainder across the next row/asset boundary. */
        uint32_t copied = 0, source_col = col;
        while(copied < FRAME_WIDTH){
            uint32_t count = SKY_W - source_col;
            if(count > FRAME_WIDTH - copied) count = FRAME_WIDTH - copied;
            dma3_copy16(out + copied, line + source_col, count / 2);
            copied += count;
            source_col = 0;
        }
    }
}
