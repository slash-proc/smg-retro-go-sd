static const uint8_t* const stage_blobs[STAGE_COUNT] = STAGE_BLOBS;
static const uint8_t* const stage_far_blobs[STAGE_COUNT] = STAGE_FAR_BLOBS;
static const uint8_t* const stage_occluders[STAGE_COUNT] = STAGE_OCCLUDERS;

static EWRAM_BSS const StageOccluder* g_stage_occluder;

static inline int stage_water_texture(const StageHeader* h, int texture){
    return h->texture_water ? ((const uint16_t*)((const uint8_t*)h + h->texture_water))[texture] : texture;
}

static EWRAM_BSS Stage g_far_stage;
static bool g_has_far_stage;

#if defined(__GNW__)
static uint8_t* g_visibility_cache_memory;
static uint32_t g_visibility_cache_capacity;
static volatile bool g_visibility_cache_enabled = true;
static bool g_visibility_cache_dirty, g_visibility_cache_requested;
static const StageHeader *g_visibility_cache_main, *g_visibility_cache_far;
struct {
    uint32_t version, capacity, active, main_bytes, far_bytes, fills, copied_bytes, bypasses;
} smg_visibility_cache_stats = {.version = 1};

#if defined(SMG_PARTIAL_CHUNK_CACHE) && SMG_PARTIAL_CHUNK_CACHE
volatile bool smg_partial_chunk_cache_enabled = true;
static bool g_visibility_partial_cache_requested;
#endif

uint32_t smg_game_visibility_cache_bytes(void){ return 128u * 1024; }

/* Bind only SDK-owned CPU memory, after allocation and before stage loading. */
void smg_game_set_visibility_cache(void* memory, uint32_t bytes){
    g_visibility_cache_memory = memory && !((uintptr_t)memory & 3) ? memory : NULL;
    g_visibility_cache_capacity = g_visibility_cache_memory ? bytes : 0;
    smg_visibility_cache_stats.capacity = g_visibility_cache_capacity;
    g_visibility_cache_dirty = true;
}

static void stage_bind_visibility_cache(void){
    const StageHeader* main = g_game.stage.header;
    const StageHeader* far = g_has_far_stage ? g_far_stage.header : NULL;
    g_visibility_cache_main = main;
    g_visibility_cache_far = far;
    g_visibility_cache_requested = g_visibility_cache_enabled;
#if defined(SMG_PARTIAL_CHUNK_CACHE) && SMG_PARTIAL_CHUNK_CACHE
    g_visibility_partial_cache_requested = smg_partial_chunk_cache_enabled;
#endif
    g_visibility_cache_dirty = false;
    smg_visibility_cache_stats.active = 0;
    smg_visibility_cache_stats.main_bytes = smg_visibility_cache_stats.far_bytes = 0;
#if defined(SMG_PARTIAL_CHUNK_CACHE) && SMG_PARTIAL_CHUNK_CACHE
    smg_visibility_set_partial_chunk_cache(NULL, 0, NULL, 0, NULL, 0);
#endif
    if(!main) return;
    /* Reconstruct canonical sources even when a previous stage or toggle
     * left the live descriptors bound to the reusable SRAM slab. */
    const StageChunk* main_source = (const StageChunk*)((const uint8_t*)main + main->chunks);
    const StageChunk* far_source = far ? (const StageChunk*)((const uint8_t*)far + far->chunks) : NULL;
    g_game.stage.chunks = main_source;
    if(far) g_far_stage.chunks = far_source;
    if(!g_visibility_cache_requested) return;
    uint32_t main_count = main->chunk_count, far_count = far ? far->chunk_count : 0;
    uint32_t slots = g_visibility_cache_capacity / sizeof(StageChunk);
    if(!g_visibility_cache_memory || main_count > slots || far_count > slots - main_count){
        smg_visibility_cache_stats.bypasses++;
#if defined(SMG_PARTIAL_CHUNK_CACHE) && SMG_PARTIAL_CHUNK_CACHE
        if(g_visibility_cache_requested && g_visibility_partial_cache_requested)
            smg_visibility_set_partial_chunk_cache(g_visibility_cache_memory,
                                                   g_visibility_cache_capacity,
                                                   main_source, main_count,
                                                   far_source, far_count);
#endif
        return;
    }
    uint32_t main_bytes = main_count * sizeof(StageChunk), far_bytes = far_count * sizeof(StageChunk);
    if(main_bytes) memcpy(g_visibility_cache_memory, main_source, main_bytes);
    if(far_bytes) memcpy(g_visibility_cache_memory + main_bytes, far_source, far_bytes);
    g_game.stage.chunks = (const StageChunk*)g_visibility_cache_memory;
    if(far) g_far_stage.chunks = (const StageChunk*)(g_visibility_cache_memory + main_bytes);
    smg_visibility_cache_stats.active = 1;
    smg_visibility_cache_stats.main_bytes = main_bytes;
    smg_visibility_cache_stats.far_bytes = far_bytes;
    smg_visibility_cache_stats.fills++;
    smg_visibility_cache_stats.copied_bytes += main_bytes + far_bytes;
}

static void stage_sync_visibility_cache(void){
    if(g_visibility_cache_dirty || g_visibility_cache_requested != g_visibility_cache_enabled ||
#if defined(SMG_PARTIAL_CHUNK_CACHE) && SMG_PARTIAL_CHUNK_CACHE
       g_visibility_partial_cache_requested != smg_partial_chunk_cache_enabled ||
#endif
       g_visibility_cache_main != g_game.stage.header ||
       g_visibility_cache_far != (g_has_far_stage ? g_far_stage.header : NULL))
        stage_bind_visibility_cache();
}

void smg_game_restore_visibility_cache(void){ stage_bind_visibility_cache(); }
#endif

/* GNW home-table cache. */
#if defined(__GNW__)
static uint16_t* g_home_cache_memory;
static uint32_t g_home_cache_capacity;
static volatile bool g_home_cache_enabled = true;
static volatile bool g_compact_points_enabled = true;
static bool g_home_cache_dirty, g_home_cache_requested;
static bool g_home_cache_compact_requested;
static const StageHeader* g_home_cache_header;
struct {
    uint32_t version, capacity, active, bytes, fills, copied_bytes, bypasses, compact;
} smg_home_cache_stats = {.version = 2};

uint32_t smg_game_home_cache_bytes(void){ return 64u * 1024; }

void smg_game_set_home_cache(void* memory, uint32_t bytes){
    g_home_cache_memory = memory && !((uintptr_t)memory & 3) ? memory : NULL;
    g_home_cache_capacity = g_home_cache_memory ? bytes : 0;
    smg_home_cache_stats.capacity = g_home_cache_capacity;
    smg_home_cache_stats.active = smg_home_cache_stats.bytes = smg_home_cache_stats.compact = 0;
    g_home_cache_dirty = true;
}

static void stage_bind_home_cache(void){
    const StageHeader* h = g_game.stage.header;
    g_home_cache_header = h;
    g_home_cache_requested = g_home_cache_enabled;
    g_home_cache_compact_requested = g_compact_points_enabled;
    g_home_cache_dirty = false;
    smg_home_cache_stats.active = smg_home_cache_stats.bytes = smg_home_cache_stats.compact = 0;
    if(!h || !h->point_home || !h->point_count || !g_home_cache_requested) return;
    if(!g_home_cache_memory || h->point_count > g_home_cache_capacity / sizeof(uint16_t)){
        smg_home_cache_stats.bypasses++;
        return;
    }
    uint32_t bytes = h->point_count * sizeof(uint16_t);
    /* Fresh bindings clear the mutable point->slot map once. Later frames
     * validate old entries against current slots and exact source XYZ. */
    if(g_home_cache_compact_requested){
        memset(g_home_cache_memory, 0xff, bytes);
        smg_home_cache_stats.compact = 1;
    }
    else memcpy(g_home_cache_memory, (const uint8_t*)h + h->point_home, bytes);
    smg_home_cache_stats.active = 1;
    smg_home_cache_stats.bytes = bytes;
    smg_home_cache_stats.fills++;
    if(!smg_home_cache_stats.compact) smg_home_cache_stats.copied_bytes += bytes;
}

static void stage_sync_home_cache(void){
    if(g_home_cache_dirty || g_home_cache_requested != g_home_cache_enabled ||
       g_home_cache_compact_requested != g_compact_points_enabled ||
       g_home_cache_header != g_game.stage.header)
        stage_bind_home_cache();
}

void smg_game_restore_home_cache(void){ stage_bind_home_cache(); }
#endif

static const uint16_t* stage_point_home(const Stage* stage){
    const StageHeader* h = stage->header;
    if(!h || !h->point_home) return NULL;
#if defined(__GNW__)
    if(h == g_home_cache_header && smg_home_cache_stats.active && !smg_home_cache_stats.compact)
        return g_home_cache_memory;
#endif
    return (const uint16_t*)((const uint8_t*)h + h->point_home);
}
#if defined(__GNW__)
static uint16_t* stage_point_slots(const Stage* stage){
    return stage->header == g_home_cache_header && smg_home_cache_stats.active &&
           smg_home_cache_stats.compact ? g_home_cache_memory : NULL;
}
#endif
/* End home-table cache. */

static EWRAM_BSS uint8_t g_texture_average[STAGE_MAX_TEXTURES];
static EWRAM_BSS uint8_t g_far_texture_average[STAGE_MAX_TEXTURES];

static uint8_t palette_nearest(int r, int g, int b){
    int best = 1, best_d = 1 << 30;
    for(int i = 1; i < 256; i++){
        uint16_t c = game_palette[i];
        int dr = (c & 31) - r, dg = (c >> 5 & 31) - g, db = (c >> 10 & 31) - b;
        int d = dr * dr * 3 + dg * dg * 4 + db * db * 2;
        if(d < best_d){
            best_d = d;
            best = i;
        }
    }
    return (uint8_t)best;
}

#define TEXCACHE_LOW_SLOTS 18
static EWRAM_BSS uint8_t g_texture_cache_slot[STAGE_MAX_TEXTURES];
EWRAM_BSS TextureReference g_texture_cache_reference[TEXCACHE_SLOTS];
#if defined(__GNW__)
static EWRAM_BSS uint8_t g_texture_cache[TEXCACHE_SLOTS][TEX_TEXELS];
static uint8_t* g_texture_cache_dtcm;
static volatile bool g_texture_dtcm_enabled = true;
static bool g_texture_dtcm_active;

uint32_t smg_game_texture_cache_bytes(void){ return sizeof g_texture_cache; }

/* Initialization-only binding. Stage cache descriptors are populated later
 * by stage loading; a failed SDK allocation retains the original AXI array. */
void smg_game_set_texture_cache(void* memory, uint32_t bytes){
    g_texture_cache_dtcm = memory && !((uintptr_t)memory & 3) && bytes >= sizeof g_texture_cache
        ? memory : NULL;
    g_texture_dtcm_active = false;
}

static inline uint8_t* texture_cache_address(unsigned slot){
    return g_texture_dtcm_active ? g_texture_cache_dtcm + slot * TEX_TEXELS : g_texture_cache[slot];
}
#else
static inline uint8_t* texture_cache_address(unsigned slot){
    return (uint8_t*)VRAM + (slot < TEXCACHE_LOW_SLOTS
                                 ? 0x4B00 + slot * TEX_TEXELS
                                 : PAGE_SIZE + (slot - TEXCACHE_LOW_SLOTS) * TEX_TEXELS);
}
_Static_assert(PAGE_SIZE + (TEXCACHE_SLOTS - TEXCACHE_LOW_SLOTS) * TEX_TEXELS <= 0x13300,
               "texture cache must end below the second pose tail");
#endif

static void stage_copy_texture(unsigned slot, const TextureReference* texture){
    dma3_copy32(texture_cache_address(slot), (const void*)(uintptr_t)texture->mip[0], TEX_TEXELS / 4);
    g_texture_cache_reference[slot].mip[0] = (uint32_t)texture_cache_address(slot);
    g_texture_cache_reference[slot].mip[1] = texture->mip[1];
}

static void stage_cache_textures(const Stage* stage){
#if defined(__GNW__)
    g_texture_dtcm_active = g_texture_cache_dtcm && g_texture_dtcm_enabled;
#endif
    int n = (int)stage->header->texture_count;
    if(n > STAGE_MAX_TEXTURES) n = STAGE_MAX_TEXTURES;

    _Static_assert(sizeof g_game.actors >= STAGE_MAX_TEXTURES * 4,
                   "texture weights must fit the actor array");
    uint32_t* weight = (uint32_t*)(void*)g_game.actors;
    for(int t = 0; t < n; t++){
        weight[t] = 0;
        g_texture_cache_slot[t] = 0;
    }

    for(uint32_t i = 0; i < stage->header->triangle_count; i++){
        const StageTriangle* st = &stage->triangles[i];
        const int16_t* a = stage->points[st->v[0].point];
        const int16_t* b = stage->points[st->v[1].point];
        const int16_t* c = stage->points[st->v[2].point];
        int32_t ext[3];
        for(int k = 0; k < 3; k++){
            int32_t lo = a[k], hi = a[k];
            if(b[k] < lo) lo = b[k];
            if(b[k] > hi) hi = b[k];
            if(c[k] < lo) lo = c[k];
            if(c[k] > hi) hi = c[k];
            ext[k] = (hi - lo) >> 4;
        }
        uint32_t area = (uint32_t)(ext[0] * ext[1] + ext[1] * ext[2] + ext[0] * ext[2]);
        int texture = (int)stage_triangle_texture(st);
        if(texture < n) weight[texture] += area;
        int water = stage_water_texture(stage->header, texture);
        if(water < n && water != texture) weight[water] += area / 4;
    }
    for(int slot = 0; slot < TEXCACHE_SLOTS; slot++){
        int best = -1;
        for(int t = 0; t < n; t++)
            if(weight[t] && (best < 0 || weight[t] > weight[best])) best = t;
        if(best < 0) break;
        weight[best] = 0;
        stage_copy_texture(slot, &stage->textures[best]);
        g_texture_cache_slot[best] = (uint8_t)(slot + 1);
    }
}

static void stage_restore_texture_cache(void){
    const Stage* stage = &g_game.stage;
    unsigned n = stage->header->texture_count;
    if(n > STAGE_MAX_TEXTURES) n = STAGE_MAX_TEXTURES;
    for(unsigned t = 0; t < n; t++){
        unsigned slot = g_texture_cache_slot[t];
        if(!slot) continue;
        const TextureReference* r = &g_texture_cache_reference[slot - 1];
        if(r->mip[0] != r->mip[1]) stage_copy_texture(slot - 1, &stage->textures[t]);
        else {
            volatile uint32_t zero = 0;
            uint8_t* address = texture_cache_address(slot - 1);
            dma3_fill32(address, &zero, TEX_TEXELS / 4);
#if defined(__GNW__)
            /* Marble's hidden-glass slot is intentionally all-zero at both
             * mip levels. Preserve that binding when migrating cache pools. */
            g_texture_cache_reference[slot - 1].mip[0] =
                g_texture_cache_reference[slot - 1].mip[1] = (uint32_t)address;
#endif
        }
    }
}


#if defined(__GNW__)
void smg_game_restore_texture_cache(void){
    if(g_game.stage.header) stage_restore_texture_cache();
}

/* Debug A/B requests migrate/rebind once at a game-frame boundary. Drawing
 * sees stable descriptors; changing the flag never adds a per-texel branch. */
static void stage_sync_texture_cache(void){
    bool requested = g_texture_cache_dtcm && g_texture_dtcm_enabled;
    if(requested != g_texture_dtcm_active){
        g_texture_dtcm_active = requested;
        smg_game_restore_texture_cache();
    }
}
#endif

static const uint32_t* stage_far_records(const Stage* stage){
    return (const uint32_t*)(uintptr_t)((const uint32_t*)stage->textures)[-2];
}
static const TextureReference* stage_far_raw_texture(unsigned index){
    if(!(g_far_stage.header->surface_features & STAGE_PACKED_FAR_TEXTURES))
        return &g_far_stage.textures[index];
    uint32_t record = stage_far_records(&g_far_stage)[index];
    if(!far_texture_has_pixels(record)) return NULL;
    return &g_far_stage.textures[index];
}

static __attribute__((noinline)) uint8_t stage_raw_texture_average(const uint8_t* texels){
    int r = 0, g = 0, b = 0, count = 0;
    for(int i = 0; i < TEX_TEXELS; i += 3){
        uint8_t idx = texels[i];
        if(!idx) continue;
        uint16_t c = game_palette[idx];
        r += c & 31;
        g += c >> 5 & 31;
        b += c >> 10 & 31;
        count++;
    }
    return count ? palette_nearest(r / count, g / count, b / count) : 0;
}

static void stage_compute_texture_averages(const Stage* stage, uint8_t* average){
    int n = (int)stage->header->texture_count;
    if(n > STAGE_MAX_TEXTURES) n = STAGE_MAX_TEXTURES;
    for(int t = 0; t < n; t++){
        const uint8_t* texels;
        if(stage->header->surface_features & STAGE_PACKED_FAR_TEXTURES){
            uint32_t record = stage_far_records(stage)[t];

            if(record & 0x80000000u){
                average[t] = (uint8_t)record;
                continue;
            }
            texels = (const uint8_t*)stage->header + record;
        } else {
            texels = (const uint8_t*)(uintptr_t)stage->textures[t].mip[0];
        }
        average[t] = stage_raw_texture_average(texels);
    }
}

static void stage_load_blob(Stage* stage, const uint8_t* blob){
    const StageHeader* header = (const StageHeader*)blob;
    stage->header = header;
    stage->points = (const int16_t (*)[3])(blob + header->points);
    stage->triangles = (const StageTriangle*)(blob + header->triangles);
    stage->textures = (const TextureReference*)(blob + header->textures);
    stage->chunks = (const StageChunk*)(blob + header->chunks);
    stage->leaf_triangle_offsets = (const uint16_t*)(blob + header->leaf_triangle_offsets);
    stage->leaf_triangles = (const LeafTriangle*)(blob + header->leaf_triangles);
    stage->leaf_draw_ranks = (const uint16_t*)(blob + header->leaf_draw_ranks);
    stage->rank_triangles = (const uint16_t*)(blob + header->rank_triangles);
    stage->sky = blob + header->sky;
    stage->gravities = (const Gravity*)(blob + header->gravities);
    stage->objects = (const StageObject*)(blob + header->objects);
    stage->starts = (const StageStart*)(blob + header->starts);
    stage->rails = (const StageRail*)(blob + header->rails);
    stage->rail_points = (const StageRailPoint*)(blob + header->rail_points);
    stage->rail_parts = (const StageRailPart*)(blob + header->rail_parts);
    stage->areas = (const StageArea*)(blob + header->areas);
    stage->cameras = (const StageCamera*)(blob + header->cameras);
    stage->point_shift = header->point_shift;
    collision_load(header);
    stage_compute_texture_averages(stage, g_texture_average);
    stage_cache_textures(stage);
}

static void stage_load_far(int index){
    g_stage_occluder = (const StageOccluder*)stage_occluders[index];
    const uint8_t* blob = stage_far_blobs[index];
    g_has_far_stage = blob != NULL;
    if(!blob) return;
    const StageHeader* header = (const StageHeader*)blob;
    Stage* stage = &g_far_stage;
    stage->header = header;
    stage->points = (const int16_t (*)[3])(blob + header->points);
    stage->triangles = (const StageTriangle*)(blob + header->triangles);
    stage->textures = (const TextureReference*)(blob + header->textures);
    stage->chunks = (const StageChunk*)(blob + header->chunks);
    stage->leaf_triangle_offsets = (const uint16_t*)(blob + header->leaf_triangle_offsets);
    stage->leaf_triangles = (const LeafTriangle*)(blob + header->leaf_triangles);
    stage->leaf_draw_ranks = (const uint16_t*)(blob + header->leaf_draw_ranks);
    stage->rank_triangles = (const uint16_t*)(blob + header->rank_triangles);
    stage->point_shift = header->point_shift;
    stage_compute_texture_averages(stage, g_far_texture_average);
    for(unsigned t = 0; t < header->texture_count && t < STAGE_MAX_TEXTURES; t++)
        g_far_detail_average[t] = stage_far_raw_texture(t) ? 0 : g_far_texture_average[t];
}

static bool stage_water_level_at(v3 position, fx* level){
    const StageHeader* header = g_game.stage.header;
    if(!header->has_water) return false;
    if(header->has_water_region){
        v2 d = V2(position.x - header->water_centre[0], position.y - header->water_centre[1]);
        if(v2_length_squared(d) > header->water_radius_squared || position.z < header->water_min_z)
            return false;
    }
    *level = header->water_level;
    return true;
}
