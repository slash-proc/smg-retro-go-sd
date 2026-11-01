typedef struct {
    const StageHeader* header;
    const int16_t (*points)[3];
    const StageTriangle* triangles;
    const TextureReference* textures;
    const StageChunk* chunks;
    const uint16_t* leaf_triangle_offsets;
    const LeafTriangle* leaf_triangles;
    const uint16_t* leaf_draw_ranks;
    const uint16_t* rank_triangles;
    const uint8_t* sky;
    const Gravity* gravities;
    const StageObject* objects;
    const StageStart* starts;
    const StageRail* rails;
    const StageRailPoint* rail_points;
    const StageRailPart* rail_parts;
    const StageArea* areas;
    const StageCamera* cameras;
    int point_shift;
} Stage;

#define STAGE_MAX_TEXTURES STAGE_TEXTURE_LIMIT
#ifndef SMG_TEXTURE_CACHE_SLOTS
#define SMG_TEXTURE_CACHE_SLOTS 54
#endif
#define TEXCACHE_SLOTS SMG_TEXTURE_CACHE_SLOTS

static const uint8_t* const stage_blobs[STAGE_COUNT];
static EWRAM_BSS const StageOccluder* g_stage_occluder;
static EWRAM_BSS Stage g_far_stage;
static bool g_has_far_stage;
static EWRAM_BSS uint8_t g_texture_average[STAGE_MAX_TEXTURES];
static EWRAM_BSS uint8_t g_far_texture_average[STAGE_MAX_TEXTURES];
static EWRAM_BSS uint8_t g_far_detail_average[STAGE_MAX_TEXTURES];
static EWRAM_BSS uint8_t g_texture_cache_slot[STAGE_MAX_TEXTURES];
EWRAM_BSS TextureReference g_texture_cache_reference[TEXCACHE_SLOTS];

static uint8_t palette_nearest(int r, int g, int b);
static inline uint8_t* texture_cache_address(unsigned slot);
static void stage_restore_texture_cache(void);
static const TextureReference* stage_far_raw_texture(unsigned index);
static void stage_load_blob(Stage* stage, const uint8_t* blob);
static void stage_load_far(int index);
#if defined(__GNW__)
static void stage_bind_visibility_cache(void);
static void stage_sync_visibility_cache(void);
#endif
static bool stage_water_level_at(v3 position, fx* level);

static const uint16_t* stage_point_home(const Stage* stage);
#if defined(__GNW__)
static void stage_bind_home_cache(void);
static void stage_sync_home_cache(void);
#endif
