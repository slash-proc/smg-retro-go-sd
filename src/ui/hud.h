#if defined(__GNW__)
/* Software-backed GBA OBJ state. The homebrew compositor consumes the same
 * tile/palette/OAM formats without accessing the GBA's unmapped memory. */
extern volatile uint16_t smg_obj_tiles[512 * 16];
extern volatile uint16_t smg_obj_palette[256];
extern volatile uint16_t smg_obj_oam[128 * 4];
#define OBJ_TILES        smg_obj_tiles
#define OBJ_PALETTE      smg_obj_palette
#define OAM              smg_obj_oam
#else
#define OBJ_TILES        ((volatile uint16_t*)0x06014000)
#define OBJ_PALETTE      ((volatile uint16_t*)0x05000200)
#define OAM              ((volatile uint16_t*)0x07000000)
#endif
#define REG_BLDCNT       REG16(0x04000050)
#define REG_BLDY         REG16(0x04000054)
#define HUD_STATIC_TILES 160

enum { SLOT_GALAXY, SLOT_SCENARIO, SLOT_BANNER, SLOT_COUNT };

typedef struct {
    uint16_t attr0, attr1, attr2, pad;
} OamEntry;

typedef struct {
    int shown;
    int visible_timer;
    int slide;
    int step;
} HudCounter;

enum { COUNTER_COINS, COUNTER_STAR_BITS, COUNTER_LIVES, COUNTER_STARS, COUNTER_COUNT };

typedef struct {
    HudCounter counters[COUNTER_COUNT];
    int still_frames;
    int meter_timer, meter_slide, last_health;
    int16_t slot_art[SLOT_COUNT];
    uint8_t bee_ground_ticks;
} HudState;

static OamEntry g_oam[128] EWRAM_BSS;
static HudState g_hud EWRAM_BSS;
static int g_oam_count;

static void hud_copy_tiles(int tile, const uint8_t* data, int tiles);
static void hud_init(void);
static void hud_load_text(int slot, int art);
static void hud_begin(void);
static void hud_piece(int x, int y, int tile, int pal, int obj);
static void hud_art(int art, int x, int y, int slot);
static void hud_art_centred(int art, int cy, int slot);
static void hud_end(void);
IWRAM_CODE void hud_vblank(void);
static void hud_set_fade(int level, bool white);
static void hud_tick(const Game* game, const KeyState* keys);
static void hud_reset_counters(const Game* game);
static int hud_slide_offset(int slide);
static void hud_draw_counters(const Game* game);
static void hud_draw_fly_meter(const Game* game);
static void hud_draw_air_meter(const Game* game);
static void hud_draw_stage_title(int galaxy_art, int scenario_art);
static void hud_draw_too_bad(int frame);
