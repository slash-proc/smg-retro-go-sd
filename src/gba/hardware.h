#define REG16(a)      (*(volatile uint16_t*)(a))
#define REG32(a)      (*(volatile uint32_t*)(a))
#define REG_DISPCNT   REG16(0x04000000)
#define REG_DISPSTAT  REG16(0x04000004)
#define REG_BG2PA     REG16(0x04000020)
#define REG_BG2PD     REG16(0x04000026)
#define REG_BG2X      REG32(0x04000028)
#define REG_DMA3SAD   REG32(0x040000D4)
#define REG_DMA3DAD   REG32(0x040000D8)
#define REG_DMA3CNT   REG32(0x040000DC)
#define REG_TM2CNT_L  REG16(0x04000108)
#define REG_TM2CNT_H  REG16(0x0400010A)
#define REG_KEYINPUT  REG16(0x04000130)
#define REG_IE        REG16(0x04000200)
#define REG_IF        REG16(0x04000202)
#define REG_IME       REG16(0x04000208)
#define DCNT_MODE4    0x0004
#define DCNT_PAGE     0x0010
#define DCNT_BG2      0x0400
#define DSTAT_VBL_IRQ 0x0008
#define IRQ_VBLANK    0x0001
#define DMA_ENABLE    0x80000000u
#define TM_ENABLE     0x0080
#define TM_FREQ_1024  0x0003
#define VRAM          ((uint16_t*)0x06000000)
#define PAGE_SIZE     0xA000
#if defined(__GNW__)
static uint16_t g_bg_palette[256];
#define BG_PALETTE    g_bg_palette
#else
#define BG_PALETTE    ((volatile uint16_t*)0x05000000)
#endif
#define KEY_A         0x0001
#define KEY_B         0x0002
#define KEY_SELECT    0x0004
#define KEY_START     0x0008
#define KEY_RIGHT     0x0010
#define KEY_LEFT      0x0020
#define KEY_UP        0x0040
#define KEY_DOWN      0x0080
#define KEY_R         0x0100
#define KEY_L         0x0200
#define KEY_MASK      0x03FF
#if defined(__GNW__)
/* Cortex-M7 executes Thumb/Thumb-2 only and uses the GWHB linker map rather
 * than GBA EWRAM/IWRAM sections. Keep these as ordinary target code/data. */
#define EWRAM_BSS
#define IWRAM_CODE
#define IWRAM_FN      __attribute__((noinline))
#define GBA_ARM_TARGET_ATTR
#else
#define EWRAM_BSS     __attribute__((section(".ewram_bss")))
#define IWRAM_CODE    __attribute__((section(".iwram.text"), target("arm")))
#define IWRAM_FN      IWRAM_CODE __attribute__((noinline))
#define GBA_ARM_TARGET_ATTR __attribute__((target("arm")))
#endif

static inline void dma3_copy32(void* dst, const void* src, uint32_t words);
static inline void dma3_fill32(void* dst, const volatile uint32_t* value, uint32_t words);
void display_wait_vblank(void);
static void memory_set_fast_waitstates(void);
