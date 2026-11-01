#define REG_WAITCNT  REG16(0x04000204)
#define REG_MEMCTRL  REG32(0x04000800)

#define DMA_32        0x04000000u
#define DMA_SRC_FIXED 0x01000000u

#if defined(__GNW__)
#include <string.h>

static inline void dma3_copy32(void* dst, const void* src, uint32_t words){
    memcpy(dst, src, (size_t)words * 4);
}

static inline void dma3_fill32(void* dst, const volatile uint32_t* value, uint32_t words){
    uint32_t fill = *value;
    uint32_t* out = dst;
    while(words--) *out++ = fill;
}

static void memory_set_fast_waitstates(void){}
#else
static inline void dma3_copy32(void* dst, const void* src, uint32_t words){
    REG_DMA3SAD = (uint32_t)src;
    REG_DMA3DAD = (uint32_t)dst;
    REG_DMA3CNT = DMA_ENABLE | DMA_32 | words;
}

static inline void dma3_fill32(void* dst, const volatile uint32_t* value, uint32_t words){
    REG_DMA3SAD = (uint32_t)value;
    REG_DMA3DAD = (uint32_t)dst;
    REG_DMA3CNT = DMA_ENABLE | DMA_32 | DMA_SRC_FIXED | words;
}

static void memory_set_fast_waitstates(void){
    static volatile uint32_t probe EWRAM_BSS;
    REG_MEMCTRL = 1 << 5 | 14 << 24;
    probe = 1;
    if(probe != 1) REG_MEMCTRL = 1 << 5 | 13 << 24;

    static const volatile uint32_t pattern = 0xCCAAF0C3;
    uint16_t old = REG_WAITCNT;
    REG_WAITCNT = 2 << 0 | 2 << 2 | 1 << 4 | 2 << 5 | 1 << 7 | 2 << 8 | 1 << 10 | 1 << 14;
    for(int i = 0; i < 1024; i++)
        if(pattern != 0xCCAAF0C3){
            REG_WAITCNT = old;
            break;
        }
}
#endif
