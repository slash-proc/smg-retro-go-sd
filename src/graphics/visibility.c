static EWRAM_BSS FramePoint g_frame_points[MAX_POINTS];
static EWRAM_BSS uint32_t g_screen_positions[MAX_POINTS];

static EWRAM_BSS uint16_t g_visible_triangles[VISIBLE_CAP];
static EWRAM_BSS uint16_t g_point_list[MAX_POINTS];
#if defined(__GNW__)
#include "compact_points.h"
static EWRAM_BSS uint16_t g_compact_point_buckets[1024];
struct {
    uint32_t version, calls, candidate_faces, admitted_faces, vertices, map_hits, coordinate_hits;
} smg_compact_point_stats = {.version = 1};
#endif
static void* geometry_point_scratch(void){ return g_point_list; }

static EWRAM_BSS uint32_t g_rank_bits[(STAGE_MAX_RANKS / 3 + 31) / 32];
static EWRAM_BSS uint16_t g_visible_leaves[STAGE_MAX_LEAVES];

static EWRAM_BSS uint8_t g_point_marks[(DIRECT_POINTS + 3) & ~3];

static EWRAM_BSS uint8_t g_far_point_marks[(FAR_DIRECT_MARKS + 4) & ~3];
_Static_assert(STAGE_FAR_MAX_POINTS <= DIRECT_POINTS || STAGE_MAX_POINTS <= STAGE_DIRECT_POINTS,
               "far LOD points must fit g_point_marks");

static EWRAM_BSS uint8_t g_leaf_stamps[(STAGE_MAX_HOME_LEAVES + 4) & ~3];
static EWRAM_BSS int16_t g_leaf_delta[STAGE_MAX_HOME_LEAVES + 1];

static int g_far_reserve;
static int g_far_base;
_Static_assert(STAGE_FAR_MAX_POINTS <= STAGE_FAR_POINT_LIMIT &&
                   STAGE_DIRECT_POINTS + POINTS_SPLIT_RESERVE + STAGE_FAR_DIRECT_POINTS + 1 <= MAX_POINTS &&
                   POINTS_SPLIT_RESERVE + STAGE_FAR_POINT_LIMIT + 1 + 2048 <= MAX_POINTS,
               "point pool must hold the stage, split points and far LOD");

EWRAM_BSS uint16_t g_triangle_bin_heads[2 * BIN_COUNT];
static EWRAM_BSS Figure* g_figure_bin_heads[BIN_COUNT];
static uint8_t g_point_stamp;

#if defined(__GNW__) && defined(SMG_ZBIN_RAM) && SMG_ZBIN_RAM && \
    !(defined(SMG_ZBIN_DTCM) && SMG_ZBIN_DTCM)
/* This byte table is consulted for each emitted stage face. Keep the hot copy
 * in AXI SRAM so a mapped-XIP lookup does not sit on the inner emission path. */
EWRAM_BSS uint8_t smg_lut_zbin_ram[2048];
#endif

#if defined(__GNW__) && defined(SMG_ZBIN_DTCM) && SMG_ZBIN_DTCM
/* DTCM is faster for CPU reads than AXI SRAM on STM32H7. The XIP table remains
 * authoritative and is also the fallback when the optional allocation fails. */
static uint8_t* g_lut_zbin_dtcm;
struct {
    uint32_t version, bytes, active, restore_count;
} smg_zbin_dtcm_stats = {.version = 1};
#endif

void smg_visibility_init(void){
#if defined(__GNW__) && defined(SMG_ZBIN_DTCM) && SMG_ZBIN_DTCM
    if(g_lut_zbin_dtcm){
        memcpy(g_lut_zbin_dtcm, lut_zbin, 2048);
        smg_zbin_dtcm_stats.restore_count++;
    }
#elif defined(__GNW__) && defined(SMG_ZBIN_RAM) && SMG_ZBIN_RAM
    memcpy(smg_lut_zbin_ram, lut_zbin, sizeof smg_lut_zbin_ram);
#endif
}

bool smg_visibility_set_zbin_storage(void* memory, uint32_t bytes){
#if defined(__GNW__) && defined(SMG_ZBIN_DTCM) && SMG_ZBIN_DTCM
    if(!memory || bytes < 2048){
        g_lut_zbin_dtcm = NULL;
        smg_zbin_dtcm_stats.bytes = 0;
        smg_zbin_dtcm_stats.active = 0;
        return false;
    }
    g_lut_zbin_dtcm = memory;
    memcpy(g_lut_zbin_dtcm, lut_zbin, 2048);
    smg_zbin_dtcm_stats.bytes = 2048;
    smg_zbin_dtcm_stats.active = 1;
    return true;
#else
    (void)memory;
    (void)bytes;
    return false;
#endif
}

IWRAM_CODE static inline uint32_t lowest_set_bit_index(uint32_t bits){
    static const uint8_t debruijn[32] = {0,  1,  28, 2,  29, 14, 24, 3, 30, 22, 20, 15, 25, 17, 4,  8,
                                         31, 27, 13, 23, 21, 19, 16, 7, 26, 12, 18, 6,  11, 5,  10, 9};
    return debruijn[((bits & -bits) * 0x077CB531u) >> 27];
}

static int g_point_count, g_farthest_visible_bin;

IWRAM_CODE static int depth_bin_for_z(fx z){
    uint32_t i = (uint32_t)fx_max(z, 0) >> 6;
    i = i > 2047 ? 2047 : i;
#if defined(__GNW__) && defined(SMG_ZBIN_DTCM) && SMG_ZBIN_DTCM
    return g_lut_zbin_dtcm ? g_lut_zbin_dtcm[i] : lut_zbin[i];
#elif defined(__GNW__) && defined(SMG_ZBIN_RAM) && SMG_ZBIN_RAM
    return smg_lut_zbin_ram[i];
#else
    return lut_zbin[i];
#endif
}

#if defined(__GNW__)
/* Cortex-M7 is Thumb-only; the GBA's ARM-state hierarchy walk cannot be used. */
static volatile bool g_cull_narrow_enabled = true;
struct {
    uint32_t version, narrow_jobs, wide_jobs;
} smg_cull_math_stats = {.version = 1};

#if defined(SMG_PARTIAL_CHUNK_CACHE) && SMG_PARTIAL_CHUNK_CACHE
typedef struct {
    uint32_t tag;
    StageChunk chunk;
} PartialChunkEntry;
_Static_assert(sizeof(PartialChunkEntry) == 36, "partial chunk-cache entry layout");

static PartialChunkEntry* g_partial_chunk_entries;
static PartialChunkEntry* g_partial_chunk_far_entries;
static uint8_t* g_partial_chunk_main_lru;
static uint32_t g_partial_chunk_main_count, g_partial_chunk_far_count;
static const StageChunk* g_partial_chunk_main_source;
static const StageChunk* g_partial_chunk_far_source;
struct {
    uint32_t version, active, bytes, entries, main_entries, far_entries, main_ways;
    uint32_t main_hits, main_misses, far_hits, far_misses, resets;
} smg_partial_chunk_cache_stats = {.version = 3};

static uint32_t partial_floor_power_two(uint32_t value){
    uint32_t out = 0;
    while(value){out = value; value &= value - 1;}
    return out;
}
static uint32_t partial_ceil_power_two(uint32_t value){
    if(value <= 1) return value;
    value--;
    value |= value >> 1;
    value |= value >> 2;
    value |= value >> 4;
    value |= value >> 8;
    value |= value >> 16;
    return value + 1;
}

void smg_visibility_set_partial_chunk_cache(void* memory, uint32_t bytes,
                                             const StageChunk* main_source, uint32_t main_count,
                                             const StageChunk* far_source, uint32_t far_count){
    g_partial_chunk_entries = NULL;
    g_partial_chunk_far_entries = NULL;
    g_partial_chunk_main_lru = NULL;
    g_partial_chunk_main_count = 0;
    g_partial_chunk_far_count = 0;
    g_partial_chunk_main_source = NULL;
    g_partial_chunk_far_source = NULL;
    smg_partial_chunk_cache_stats.active = 0;
    smg_partial_chunk_cache_stats.bytes = 0;
    smg_partial_chunk_cache_stats.entries = 0;
    smg_partial_chunk_cache_stats.main_entries = 0;
    smg_partial_chunk_cache_stats.far_entries = 0;
    smg_partial_chunk_cache_stats.main_ways = 0;
    smg_partial_chunk_cache_stats.main_hits = 0;
    smg_partial_chunk_cache_stats.main_misses = 0;
    smg_partial_chunk_cache_stats.far_hits = 0;
    smg_partial_chunk_cache_stats.far_misses = 0;
    smg_partial_chunk_cache_stats.resets++;
    if(!memory || !main_source || !main_count || bytes < sizeof(PartialChunkEntry) || ((uintptr_t)memory & 3))
        return;

    uint32_t available = bytes / sizeof(PartialChunkEntry);
    uint32_t far_entries = 0;
    if(far_source && far_count){
        uint32_t far_budget = partial_floor_power_two(available / 4);
        far_entries = partial_ceil_power_two(far_count);
        if(far_entries > far_budget) far_entries = far_budget;
    }
    uint32_t main_available = available - far_entries;
    uint32_t main_entries = partial_ceil_power_two(main_count);
    uint32_t main_budget = partial_floor_power_two(main_available);
    if(main_entries > main_budget) main_entries = main_budget;
    if(!main_entries) return;

    PartialChunkEntry* cache = memory;
    for(uint32_t i = 0; i < main_entries + far_entries; i++) cache[i].tag = UINT32_MAX;
    uint32_t main_ways = main_entries >= 2 ? 2 : 1;
    uint32_t main_sets = main_entries / main_ways;
    uint8_t* main_lru = (uint8_t*)(cache + main_entries + far_entries);
    uint32_t entry_bytes = (main_entries + far_entries) * sizeof(PartialChunkEntry);
    uint32_t metadata_bytes = main_ways == 2 ? main_sets : 0;
    if(entry_bytes + metadata_bytes > bytes){
        main_ways = 1;
        main_lru = NULL;
        metadata_bytes = 0;
    }else if(main_lru){
        memset(main_lru, 0, metadata_bytes);
    }
    g_partial_chunk_entries = cache;
    g_partial_chunk_far_entries = far_entries ? cache + main_entries : NULL;
    g_partial_chunk_main_lru = main_lru;
    g_partial_chunk_main_count = main_entries;
    g_partial_chunk_far_count = far_entries;
    g_partial_chunk_main_source = main_source;
    g_partial_chunk_far_source = far_source;
    smg_partial_chunk_cache_stats.active = 1;
    smg_partial_chunk_cache_stats.bytes = entry_bytes + metadata_bytes;
    smg_partial_chunk_cache_stats.entries = main_entries + far_entries;
    smg_partial_chunk_cache_stats.main_entries = main_entries;
    smg_partial_chunk_cache_stats.far_entries = far_entries;
    smg_partial_chunk_cache_stats.main_ways = main_ways;
}

static inline const StageChunk* partial_chunk_for_job(const CullJob* job, uint32_t index){
    if(!g_partial_chunk_entries)
        return &job->chunks[index];
    PartialChunkEntry* bank;
    uint32_t count;
    bool far;
    if(job->chunks == g_partial_chunk_main_source){
        bank = g_partial_chunk_entries;
        count = g_partial_chunk_main_count;
        far = false;
    }else if(job->chunks == g_partial_chunk_far_source && g_partial_chunk_far_entries){
        bank = g_partial_chunk_far_entries;
        count = g_partial_chunk_far_count;
        far = true;
    }else return &job->chunks[index];
    if(!far && g_partial_chunk_main_lru && smg_partial_chunk_cache_stats.main_ways == 2){
        uint32_t set = index & ((count >> 1) - 1);
        PartialChunkEntry* way0 = &bank[set * 2];
        PartialChunkEntry* way1 = way0 + 1;
        PartialChunkEntry* entry;
        if(way0->tag == index){
            entry = way0;
            g_partial_chunk_main_lru[set] = 1;
            smg_partial_chunk_cache_stats.main_hits++;
        }else if(way1->tag == index){
            entry = way1;
            g_partial_chunk_main_lru[set] = 0;
            smg_partial_chunk_cache_stats.main_hits++;
        }else{
            unsigned victim = g_partial_chunk_main_lru[set] & 1;
            entry = victim ? way1 : way0;
            entry->chunk = job->chunks[index];
            entry->tag = index;
            g_partial_chunk_main_lru[set] = victim ^ 1;
            smg_partial_chunk_cache_stats.main_misses++;
        }
        return &entry->chunk;
    }
    PartialChunkEntry* entry = &bank[index & (count - 1)];
    if(entry->tag != index){
        entry->chunk = job->chunks[index];
        entry->tag = index;
        if(far) smg_partial_chunk_cache_stats.far_misses++;
        else smg_partial_chunk_cache_stats.main_misses++;
    }else{
        if(far) smg_partial_chunk_cache_stats.far_hits++;
        else smg_partial_chunk_cache_stats.main_hits++;
    }
    return &entry->chunk;
}
#endif

static bool cull_narrow_planes(const CullJob* job){
    for(unsigned p = 0; p < 5; p++){
        const int32_t* plane = job->planes[p];
        uint32_t sums[2] = {0, 0};
        for(unsigned i = 0; i < 6; i++){
            uint32_t magnitude = plane[i] < 0 ? 0u - (uint32_t)plane[i] : (uint32_t)plane[i];
            if(magnitude > 32767u - sums[i / 3]) return false;
            sums[i / 3] += magnitude;
        }
        /* Include room for interval shifts and the +/-64 plane tolerance. */
        if(plane[6] < INT32_MIN + 8388672 || plane[6] > INT32_MAX - 8388672) return false;
    }
    return true;
}

static inline void cull_narrow_interval(const int32_t center[3], const int32_t extent[3],
        const int32_t plane[7], int32_t* low, int32_t* high){
    /* |center|<=65536, |extent|<=65535 and each coefficient L1<=32767.
     * All products, partial sums and the sum/difference of halved dots fit
     * signed32. Retain both original halvings before the final Q8 shift. */
    int32_t midpoint = 0, radius = 0;
    for(unsigned axis = 0; axis < 3; axis++){
        midpoint += center[axis] * plane[axis];
        radius += extent[axis] * plane[axis + 3];
    }
    *low = (((midpoint >> 1) - (radius >> 1)) >> 8) + plane[6];
    *high = (((midpoint >> 1) + (radius >> 1)) >> 8) + plane[6];
}

/* The cone test excludes deltas at +/-2^17 and StageChunk centers are int16.
 * If any camera component is outside [-163839,163838], no int16 center can
 * produce an accepted delta on that axis, so the entire job can skip cones.
 * Within that range camera-center subtraction is safe signed32. StageChunk
 * stores signed8 normals and unsigned8 extents, bounding the three-term dot
 * and radius sums by50,331,264 and100,269,315. Keep the final plane-limit
 * expression in64 bits because that field spans int32. */
__attribute__((always_inline)) static inline bool chunk_cone_rejected(
        const StageChunk* chunk, const int32_t camera[3], bool camera_reachable){
    if(chunk->cone_plane_limit == CONE_NONE || !camera_reachable) return false;
    int32_t dot = 0, radius = 0;
    for(unsigned axis = 0; axis < 3; axis++){
        int32_t delta = camera[axis] - chunk->cone_center[axis];
        if(delta <= -0x20000 || delta >= 0x20000) return false;
        int32_t normal = chunk->cone_normal[axis];
        int32_t extent = chunk->cone_normal_extent[axis];
        dot += delta * normal;
        radius += (delta < 0 ? -delta : delta) * extent;
    }
    return (int64_t)chunk->cone_plane_limit + 2 * ((int64_t)dot + radius) <= -768;
}

int geometry_cull_chunks(CullJob* output){
    /* The job's inputs are immutable for the complete traversal; the only
     * output field is written after the walk. Read in place instead of
     * memcpying the 176-byte descriptor into this hot path's stack frame. */
    const CullJob* job = output;
    const bool narrow = g_cull_narrow_enabled && cull_narrow_planes(job);
    if(narrow) smg_cull_math_stats.narrow_jobs++;
    else smg_cull_math_stats.wide_jobs++;
    uint32_t out = 0;
    uint32_t i = 0;
    /* Preorder nodes carry the index after their subtree. Restore inherited
     * planes at that boundary, including when rejection skips descendants. */
    enum { CULL_MASK_STACK_CAPACITY = 32 };
    struct { uint32_t end, planes; } stack[CULL_MASK_STACK_CAPACITY];
    unsigned depth = 0;
    uint32_t planes = 31;
    bool cone_camera_reachable = true;
    for(unsigned axis = 0; axis < 3; axis++)
        if(job->cam[axis] < -163839 || job->cam[axis] > 163838)
            cone_camera_reachable = false;
    while(i < job->n){
        while(depth && i >= stack[depth - 1].end)
            planes = stack[--depth].planes;
#if defined(SMG_PARTIAL_CHUNK_CACHE) && SMG_PARTIAL_CHUNK_CACHE
        const StageChunk* chunk = partial_chunk_for_job(job, i);
#else
        const StageChunk* chunk = &job->chunks[i];
#endif
        SMG_COUNT(chunks_visited);
        int32_t center[3], extent[3];
        for(int axis = 0; axis < 3; axis++){
            center[axis] = (int32_t)chunk->min[axis] + chunk->max[axis];
            extent[axis] = (int32_t)chunk->max[axis] - chunk->min[axis];
        }

        bool visible = true;
        uint32_t child_planes = planes;
        for(int plane = 0; plane < 5; plane++){
            uint32_t bit = UINT32_C(1) << plane;
            if(!(planes & bit)) continue;
            SMG_COUNT(planes_tested);
            const int32_t* p = job->planes[plane];
            int64_t low, high;
            if(narrow){
                int32_t lo, hi;
                cull_narrow_interval(center, extent, p, &lo, &hi);
                low = lo;
                high = hi;
            }else{
                int64_t midpoint = 0, radius = 0;
                for(int axis = 0; axis < 3; axis++){
                    midpoint += (int64_t)center[axis] * p[axis];
                    radius += (int64_t)extent[axis] * p[axis + 3];
                }
                /* The assembly halves center/extents, then shifts the result by 8. */
                int64_t center = midpoint >> 1;
                int64_t spread = radius >> 1;
                low = ((center - spread) >> 8) + p[6];
                high = ((center + spread) >> 8) + p[6];
            }
            if(plane < 4){
                if(low > 64){
                    visible = false;
                    break;
                }
                if(high < -64) child_planes &= ~bit;
            }else if(low - 64 >= job->z_drop ||
                     (job->z_inner != 0 && high < job->z_inner)){
                visible = false;
                break;
            }else if(job->z_inner == 0 && high + 64 < job->z_drop){
                child_planes &= ~bit;
            }
        }

        if(!visible) SMG_COUNT(chunks_frustum_rejected);
        /* Match the original whole-chunk normal cone rejection. Outside the
         * original safe coordinate range, retain the chunk conservatively. */
        if(visible && chunk_cone_rejected(chunk, job->cam, cone_camera_reachable)){
            SMG_COUNT(chunks_cone_rejected);
            visible = false;
        }

        if(!visible){
            i = chunk->skip;
            continue;
        }
        if(chunk->leaf != UINT16_MAX)
            job->leaves[out++] = chunk->leaf;
        else if(child_planes != planes){
            stack[depth++] = (typeof(*stack)){chunk->skip, planes};
            planes = child_planes;
        }
        i++;
    }
    output->leaf_count = out;
    return (int)out;
}
#endif

_Static_assert(offsetof(CullJob, z_inner) == 172, "visibility_bvh.s assumes the CullJob layout");

static int collect_visible_chunks(const Stage* st, const CameraView* cv, v3 cam, int farthest_bin,
                                  int inner_bin){
    const m3* m = &cv->to_screen.m;
    v3 o = cv->to_screen.o;
    v3 rx = m->r[0], ry = m->r[1], rz = m->r[2];
    const v3 n[5] = {v3_sub(rx, rz), v3_sub(v3_neg(rx), rz), v3_sub(ry, rz), v3_sub(v3_neg(ry), rz), rz};
    const int32_t k[5] = {o.x - o.z, -o.x - o.z, o.y - o.z, -o.y - o.z, o.z};

    int s = st->point_shift;
    CullJob job = {st->chunks,
                   st->header->chunk_count,
                   {{0}},
                   lut_zdrop[farthest_bin],
                   {cam.x >> s, cam.y >> s, cam.z >> s},
                   g_visible_leaves,
                   0,
                   inner_bin ? lut_zdrop[inner_bin] : 0};
    for(int p = 0; p < 5; p++){
        int32_t* pl = job.planes[p];
        pl[0] = n[p].x << s;
        pl[1] = n[p].y << s;
        pl[2] = n[p].z << s;
        pl[3] = fx_abs(n[p].x) << s;
        pl[4] = fx_abs(n[p].y) << s;
        pl[5] = fx_abs(n[p].z) << s;
        pl[6] = k[p];
    }
    return geometry_cull_chunks(&job);
}

IWRAM_CODE static int view_nearest_axis(v3 dir){
    fx d[6] = {-dir.x, dir.x, -dir.y, dir.y, -dir.z, dir.z};
    int best = 0;
    for(int k = 1; k < 6; k++)
        if(d[k] >= d[best]) best = k;
    return best;
}

_Static_assert(offsetof(CollectJob, plist_end) == 60 && offsetof(CollectJob, home) == 64,
               "visibility_bvh.s assumes the CollectJob layout");
_Static_assert(offsetof(CollectJob, points) == 68 && offsetof(CollectJob, triangles) == 72,
               "assembly assumes the CollectJob pointer offsets");
_Static_assert(offsetof(CollectJob, axis_coord) == 80 && offsetof(CollectJob, axis_sign) == 84,
               "visibility_bvh.s assumes the CollectJob axis offsets");

#if defined(__GNW__)
static volatile bool g_collect_narrow_enabled = true;
struct {
    uint32_t version, narrow_jobs, wide_jobs;
} smg_collect_math_stats = {.version = 1};

static bool collect_narrow_camera(const CollectJob* job){
    for(unsigned k = 0; k < 3; k++)
        if(job->cam[k] < INT16_MIN || job->cam[k] > INT16_MAX) return false;
    return true;
}

static inline int32_t collect_facing_narrow(const int32_t anchor[3],
        const int32_t normal[3], const int32_t camera[3]){
    /* Anchor and camera are signed16; normals are signed16 (or doubled
     * signed8 for indexed records). |delta| <= 65535, so each product fits
     * signed32, including normal -32768. Shift each product separately:
     * shifting a combined dot product changes rounding/rejection. The sum
     * of three shifted products also fits signed32. */
    int32_t facing = 0;
    for(unsigned k = 0; k < 3; k++)
        facing += ((anchor[k] - camera[k]) * normal[k]) >> 8;
    return facing;
}

/* End collect facing helpers. */

__attribute__((always_inline)) static inline void geometry_collect_leaves_mode(
        CollectJob* job, bool indexed, bool narrow){
    CollectJob* output = job;
    CollectJob parameters = *job;
    job = &parameters;
    const int axis = job->axis_coord / 2;
    /* Publish totals once per completed call, including capacity exits. This
     * avoids global stats reloads/stores across potentially aliasing marks. */
    uint32_t tested = 0, rejected = 0;
    for(uint32_t li = 0; li < (uint32_t)job->leaf_count; li++){
        uint16_t leaf = job->leaves[li];
        uint32_t first = job->leaf_triangle_offsets[leaf];
        uint32_t end = job->leaf_triangle_offsets[leaf + 1];
        for(uint32_t i = first; i < end; i++){
            if(job->plist >= job->plist_end){
                SMG_COUNT(point_list_capacity_hits);
                goto complete;
            }
            tested++;

            uint16_t points[3];
            int32_t anchor[3], normal[3];
            if(indexed){
                const StageTriangle* triangle = &job->triangles[i];
                for(unsigned k = 0; k < 3; k++)
                    points[k] = triangle->v[k].point;
                for(unsigned k = 0; k < 3; k++){
                    anchor[k] = job->points[points[0]][k];
                    normal[k] = (int32_t)triangle->normal[k] * 2;
                }
            }else{
                const LeafTriangle* record = &job->recs[i];
                for(unsigned k = 0; k < 3; k++){
                    points[k] = record->points[k];
                    anchor[k] = record->a[k];
                    normal[k] = record->normal[k];
                }
            }

            int32_t facing;
            if(narrow) facing = collect_facing_narrow(anchor, normal, job->cam);
            else {
                facing = 0;
                for(unsigned k = 0; k < 3; k++){
                    int32_t delta = anchor[k] - job->cam[k];
                    facing += (int32_t)(((int64_t)delta * normal[k]) >> 8);
                }
            }
            /* Original COLLECT_INDEXED/geometry_collect_leaves branches to
             * the next triangle on nonnegative facing, before axis testing. */
            bool reject = facing >= 0;
            if(facing < 0){
                int32_t farthest = anchor[axis];
                if(job->axis_sign) farthest = -farthest;
                if(farthest > job->limit){
                    for(unsigned k = 1; k < 3; k++){
                        int32_t coordinate = job->points[points[k]][axis];
                        if(job->axis_sign) coordinate = -coordinate;
                        if(farthest > coordinate) farthest = coordinate;
                    }
                    reject = farthest != 32768 && farthest > job->limit;
                }
            }
            if(reject){
                rejected++;
                continue;
            }

            if(job->home == NULL){
                for(unsigned k = 0; k < 3; k++){
                    uint16_t point = points[k];
                    if(job->marks[point] != (uint8_t)job->stamp){
                        job->marks[point] = (uint8_t)job->stamp;
                        *job->plist++ = point;
                    }
                }
            }

            uint16_t rank = job->ranks[i];
            uint32_t word = rank >> 5;
            job->bits[word] |= UINT32_C(1) << (rank & 31);
            if(word < job->lo) job->lo = word;
            if(word > job->hi) job->hi = word;
        }
    }
complete:
    SMG_ADD(leaf_triangles_tested, tested);
    SMG_ADD(leaf_triangles_rejected, rejected);
    output->plist = job->plist;
    output->lo = job->lo;
    output->hi = job->hi;
}

static void geometry_collect_leaves_c(CollectJob* job, bool indexed){
    /* Specialize before the triangle loop so narrow jobs do not carry wide
     * product temporaries or a per-triangle arithmetic-mode branch. */
    if(g_collect_narrow_enabled && collect_narrow_camera(job)){
        smg_collect_math_stats.narrow_jobs++;
        geometry_collect_leaves_mode(job, indexed, true);
    }else{
        smg_collect_math_stats.wide_jobs++;
        geometry_collect_leaves_mode(job, indexed, false);
    }
}

void geometry_collect_leaves(CollectJob* job){
    geometry_collect_leaves_c(job, false);
}
#endif

__attribute__((noinline)) static void visibility_copy_overlay(const uint32_t* source, unsigned words){
    uint32_t* target = (uint32_t*)(uintptr_t)geometry_collect_leaves;
    for(unsigned i = 0; i < words; i += 64){
        unsigned n = words - i;
        if(n > 64) n = 64;
        dma3_copy32(target + i, source + i, n);
    }
}
__attribute__((noinline)) static void visibility_collect_indexed(CollectJob* job){
#if defined(__GNW__)
    geometry_collect_leaves_c(job, true);
#else
    if(job->home)
        visibility_copy_overlay(__visibility_overlay_start,
                                (unsigned)(__visibility_overlay_end - __visibility_overlay_start));
    else
        visibility_copy_overlay(
            __visibility_direct_overlay_start,
            (unsigned)(__visibility_direct_overlay_end - __visibility_direct_overlay_start));
    geometry_collect_leaves(job);
    const uint32_t* original =
        __iwram_lma + ((uintptr_t)geometry_collect_leaves - (uintptr_t)__iwram_start) / 4;
    unsigned words = ((uintptr_t)geometry_collect_leaves_end - (uintptr_t)geometry_collect_leaves) / 4;
    visibility_copy_overlay(original, words);
#endif
}

_Static_assert(offsetof(ClaimJob, slot_limit) == 44, "visibility_bvh.s assumes the ClaimJob layout");

#if defined(__GNW__)
_Static_assert(sizeof(ClaimJob) == 52 && offsetof(ClaimJob, slot_fast_limit) == 48,
               "GNW point-claim capture assumes the extended ARM layout");
#if defined(SMG_CAPTURE_POINT_CLAIMS)
__attribute__((noinline, noclone))
#endif
int geometry_claim_points(ClaimJob* job){
    const uint8_t stamp = (uint8_t)job->stamp;
    const uint16_t* visible = job->visible;
    uint32_t count = 0;

    for(; count < (uint32_t)job->n; count++){
        const StageTriangle* triangle = &job->tris[visible[count]];
        if(job->next_slot >= job->slot_fast_limit){
            /* Usually the compiler's worst-case leaf margin proves a whole
             * triangle fits. Near capacity, check its actual distinct leaves
             * before changing stamps or mappings. Already claimed vertices
             * must remain usable even when the pool is exactly full. */
            uint16_t leaves[3];
            uint32_t needed = 0;
            for(unsigned vertex = 0; vertex < 3; vertex++){
                uint16_t leaf = job->home[triangle->v[vertex].point];
                leaves[vertex] = leaf;
                if(job->leaf_stamps[leaf] == stamp ||
                   (vertex > 0 && leaves[0] == leaf) ||
                   (vertex > 1 && leaves[1] == leaf)) continue;
                needed += job->leaf_first[leaf + 1] - job->leaf_first[leaf];
            }
            if(needed > job->slot_limit - job->next_slot) break;
        }
        for(unsigned vertex = 0; vertex < 3; vertex++){
            uint16_t point = triangle->v[vertex].point;
            uint16_t leaf = job->home[point];

            if(job->leaf_stamps[leaf] != stamp){
                job->leaf_stamps[leaf] = stamp;
                uint16_t first = job->leaf_first[leaf];
                uint16_t end = job->leaf_first[leaf + 1];
                job->leaf_delta[leaf] = (int16_t)(job->next_slot - first);
                job->next_slot += end - first;
            }

            FramePoint* slot = &job->pool[(int32_t)job->leaf_delta[leaf] + point];
            if(slot->mark != stamp){
                slot->mark = stamp;
                *job->plist++ = point;
            }
        }
    }
    if(count < (uint32_t)job->n) SMG_COUNT(point_pool_capacity_hits);
    SMG_PEAK(point_pool_peak, job->next_slot);
    return (int)count;
}
#endif

__attribute__((noinline)) IWRAM_CODE static void reverse_visible(int from, int to){
    for(to--; from < to; from++, to--){
        uint16_t t = g_visible_triangles[from];
        g_visible_triangles[from] = g_visible_triangles[to];
        g_visible_triangles[to] = t;
    }
}

__attribute__((noinline)) static uint32_t rank_segment_next(
    const uint16_t* segs, uint32_t* seg, uint32_t pos, int* seg_out, int n){
    reverse_visible(*seg_out, n);
    *seg_out = n;
    uint32_t end;
    do end = ++*seg < segs[0] ? segs[*seg + 1] : UINT32_MAX;
    while(pos >= end);
    return end;
}

IWRAM_FN __attribute__((optimize("Os"))) static int rank_bits_to_triangles(
    const uint16_t* ranked, uint32_t lo, uint32_t hi, const uint16_t* segs){
    int n = 0, seg_out = 0;
    uint32_t seg = 1, seg_end = segs && segs[0] > 1 ? segs[2] : UINT32_MAX;
    for(uint32_t w = lo; w <= hi && lo <= hi; w++){
        uint32_t bits = g_rank_bits[w];
        if(!bits) continue;
        g_rank_bits[w] = 0;
        while(bits){
            uint32_t b = lowest_set_bit_index(bits), pos = w << 5 | b;
            bits &= bits - 1;
            if(pos >= seg_end) seg_end = rank_segment_next(segs, &seg, pos, &seg_out, n);
            if(n < VISIBLE_CAP) g_visible_triangles[n++] = ranked[pos];
            else SMG_COUNT(visible_capacity_hits);
        }
    }
    if(segs) reverse_visible(seg_out, n);
    return n;
}

GBA_ARM_TARGET_ATTR __attribute__((noinline)) static int rank_bits_to_triangles_masked(
    const Stage* st, const uint16_t* ranked, uint32_t lo, uint32_t hi, const uint16_t* segs){
    int n = 0, seg_out = 0;
    uint32_t seg = 1, seg_end = segs && segs[0] > 1 ? segs[2] : UINT32_MAX;
    uint16_t scenario = (uint16_t)(1u << (g_game.scenario + 9));
    for(uint32_t w = lo; w <= hi && lo <= hi; w++){
        uint32_t bits = g_rank_bits[w];
        if(!bits) continue;
        g_rank_bits[w] = 0;
        while(bits){
            uint32_t b = lowest_set_bit_index(bits), pos = w << 5 | b;
            bits &= bits - 1;
            if(pos >= seg_end) seg_end = rank_segment_next(segs, &seg, pos, &seg_out, n);
            uint16_t tri = ranked[pos];
            uint16_t flags = st->triangles[tri].texture;
            if(!(flags & SURF_SCENARIO_MASK) || (flags & scenario)){
                if(n < VISIBLE_CAP) g_visible_triangles[n++] = tri;
                else SMG_COUNT(visible_capacity_hits);
            }
        }
    }
    if(segs) reverse_visible(seg_out, n);
    return n;
}

static int collect_visible_triangles(const Stage* st, int leaf_count, int axis, int32_t limit, v3 cam,
                                     int* n_plist, int* pool_used, uint8_t* marks, int pool_free){
    int s = st->point_shift;
    const StageHeader* h = st->header;
    const uint8_t* blob = (const uint8_t*)h;
    bool by_leaf = h->point_home != 0;
    const uint16_t* home = stage_point_home(st);
    bool indexed = (h->surface_features & STAGE_INDEXED_LEAVES) != 0;
    CollectJob job = {g_visible_leaves,
                      leaf_count,
                      st->leaf_triangle_offsets,
                      st->leaf_triangles,
                      st->leaf_draw_ranks + (axis >> 1) * h->leaf_triangle_count,
                      fx_min(limit >> s, 32767),
                      {cam.x >> s, cam.y >> s, cam.z >> s},
                      marks,
                      g_point_stamp,
                      g_rank_bits,
                      UINT32_MAX,
                      0,
                      g_point_list,
                      g_point_list + fx_max(pool_free - 48, 0),
                      home,
                      st->points,
                      st->triangles,
                      h->surface_features,
                      axis / 2 * 2,
                      axis & 1 ? -1 : 0};

    if(indexed){
        if((limit >> s) >= -32768) visibility_collect_indexed(&job);
    } else if((limit >> s) >= -32768) geometry_collect_leaves(&job);

    const uint16_t* ranked = st->rank_triangles + (uint32_t)(axis >> 1) * (h->rank_count / 3);
    const uint16_t* segs = axis & 1 ? st->rank_triangles + h->rank_count : NULL;
    int n = h->surface_features & STAGE_SCENARIO_SURFACES
                ? rank_bits_to_triangles_masked(st, ranked, job.lo, job.hi, segs)
                : rank_bits_to_triangles(ranked, job.lo, job.hi, segs);
    SMG_PEAK(visible_peak, n);
    if(!by_leaf){
        *n_plist = *pool_used = (int)(job.plist - g_point_list);
        return n;
    }
#if defined(__GNW__)
    uint16_t* slots = stage_point_slots(st);
    if(slots){
        CompactPointJob compact = {.visible = g_visible_triangles, .count = (uint32_t)n,
            .triangles = st->triangles, .source = st->points, .slots = slots,
            .pool = g_frame_points, .list = g_point_list, .buckets = g_compact_point_buckets,
            .bucket_count = 1024, .capacity = (uint32_t)pool_free, .stamp = g_point_stamp};
        geometry_claim_compact_points(&compact);
        smg_compact_point_stats.calls++;
        smg_compact_point_stats.candidate_faces += (uint32_t)n;
        smg_compact_point_stats.admitted_faces += compact.admitted;
        smg_compact_point_stats.vertices += compact.used;
        smg_compact_point_stats.map_hits += compact.map_hits;
        smg_compact_point_stats.coordinate_hits += compact.position_hits;
        if(compact.admitted < (uint32_t)n) SMG_COUNT(point_pool_capacity_hits);
        SMG_PEAK(point_pool_peak, compact.used);
        *n_plist = *pool_used = (int)compact.used;
        return (int)compact.admitted;
    }
#endif

    ClaimJob claim = {g_visible_triangles,
                      n,
                      st->triangles,
                      home,
                      (const uint16_t*)(blob + h->leaf_point_first),
                      g_leaf_stamps,
                      g_leaf_delta,
                      g_frame_points,
                      g_point_stamp,
                      g_point_list,
                      0,
                      (uint32_t)fx_max(pool_free - h->leaf_slot_max, 0)};
#if defined(__GNW__)
    claim.slot_fast_limit = claim.slot_limit;
    claim.slot_limit = (uint32_t)pool_free;
#endif
    n = geometry_claim_points(&claim);
    *n_plist = (int)(claim.plist - g_point_list);
    *pool_used = (int)claim.next_slot;
    return n;
}

_Static_assert(offsetof(TransformJob, home) == 68, "raster.s assumes the TransformJob layout");

static TransformJob geometry_transform_job(const int16_t (*src)[3], FramePoint* dst, uint32_t* scr,
                                           const uint16_t* list, int n, const xform* t, int shift){
    TransformJob job = {src, dst, scr, list, n, {{0}}, NULL, NULL};
    for(int r = 0; r < 3; r++){
        job.m[r][0] = t->m.r[r].x << shift;
        job.m[r][1] = t->m.r[r].y << shift;
        job.m[r][2] = t->m.r[r].z << shift;
        job.m[r][3] = ((const fx*)&t->o)[r];
    }
    return job;
}

#if defined(__GNW__)
#define STAGE_VERTEX_CACHE_ENTRIES 4096u
#define STAGE_VERTEX_CACHE_MIN_ENTRIES 2048u
typedef struct {
    uint16_t point;
    int16_t xyz[3];
} StageVertexCacheEntry;
_Static_assert(sizeof(StageVertexCacheEntry) == 8, "bounded source-vertex cache entry");
static StageVertexCacheEntry* g_stage_vertex_cache;
static uint32_t g_stage_vertex_cache_mask;
static const int16_t (*g_stage_vertex_cache_source)[3];
static volatile bool g_stage_vertex_cache_enabled = true;
/* Packed setup amortizes on16+ vertices; keep the exact matrix guard and
 * scalar fallback for smaller or wider jobs. Debug control can disable it. */
static volatile bool g_transform_dsp_enabled = true;
struct {
    uint32_t version, hits, misses, bypassed, invalidations;
} smg_vertex_cache_stats = {.version = 1};

uint32_t smg_game_vertex_cache_bytes(void){
    return STAGE_VERTEX_CACHE_ENTRIES * sizeof(StageVertexCacheEntry);
}

uint32_t smg_game_vertex_cache_min_bytes(void){
    return STAGE_VERTEX_CACHE_MIN_ENTRIES * sizeof(StageVertexCacheEntry);
}

void smg_game_invalidate_vertex_cache(void){
    g_stage_vertex_cache_source = NULL;
}

void smg_game_set_vertex_cache(void* memory, uint32_t bytes){
    uint32_t entries = bytes >= smg_game_vertex_cache_bytes() ? STAGE_VERTEX_CACHE_ENTRIES
        : bytes >= smg_game_vertex_cache_min_bytes() ? STAGE_VERTEX_CACHE_MIN_ENTRIES : 0;
    g_stage_vertex_cache = memory && !((uintptr_t)memory & 3) && entries ? memory : NULL;
    g_stage_vertex_cache_mask = g_stage_vertex_cache ? entries - 1 : 0;
    smg_game_invalidate_vertex_cache();
}

/* A row L1 bound keeps every intermediate signed product/sum representable
 * for all int16 source coordinates. Scaled or otherwise wider matrices use
 * the scalar path. Pack only the XY coefficients; Z remains a scalar MLA. */
static bool transform_pack_matrix(const TransformJob* job, uint32_t packed[3]){
    for(unsigned row = 0; row < 3; row++){
        uint32_t magnitude = 0;
        for(unsigned k = 0; k < 3; k++){
            int32_t value = job->m[row][k];
            if(value < INT16_MIN || value > INT16_MAX) return false;
            magnitude += value < 0 ? 0u - (uint32_t)value : (uint32_t)value;
        }
        if(magnitude > 65535u) return false;
        packed[row] = (uint16_t)job->m[row][0] | (uint32_t)(uint16_t)job->m[row][1] << 16;
    }
    return true;
}

static inline int32_t transform_dot_packed(uint32_t xy, int32_t z, uint32_t coefficients, int32_t mz){
    int32_t dot = mz * z;
#if defined(__arm__) && defined(__ARM_FEATURE_DSP)
    __asm__("smlad %0, %1, %2, %0" : "+r"(dot) : "r"(xy), "r"(coefficients));
#else
    dot += (int16_t)xy * (int16_t)coefficients +
           (int16_t)(xy >> 16) * (int16_t)(coefficients >> 16);
#endif
    return dot;
}

void engine_transform_points(const TransformJob* job){
    /* Snapshot immutable job inputs once. Output writes must not force matrix
     * and pointer reloads for every vertex in the C fallback. */
    const TransformJob parameters = *job;
    job = &parameters;
    uint32_t packed_matrix[3];
    const bool packed = job->n >= 16 && g_transform_dsp_enabled &&
                        transform_pack_matrix(job, packed_matrix);
    /* Only the immutable main-stage source array is eligible. Figure/pose
     * scratch can change at the same address, and far transforms must not
     * evict the main-stage working set between frames. Cache coordinates,
     * never camera-dependent transformed positions or destination slots. */
    StageVertexCacheEntry* const cache = g_stage_vertex_cache;
    const uint32_t cache_mask = g_stage_vertex_cache_mask;
    const bool cached = cache && g_stage_vertex_cache_enabled &&
                        job->src == g_game.stage.points;
    if(cached && job->n > 0 && g_stage_vertex_cache_source != job->src){
        for(unsigned i = 0; i <= cache_mask; i++)
            cache[i].point = UINT16_MAX;
        g_stage_vertex_cache_source = job->src;
        smg_vertex_cache_stats.invalidations++;
    }
    if(!cached && job->n > 0) smg_vertex_cache_stats.bypassed += (uint32_t)job->n;
    for(int i = 0; i < job->n; i++){
        uint16_t point = job->list ? job->list[i] : (uint16_t)i;
        uint32_t slot = point;
        if(job->home){
            uint16_t mapped = job->home[point];
            slot = job->delta ? (uint32_t)((int32_t)job->delta[mapped] + point) : mapped;
        }

        const int16_t* src = job->src[point];
        /* UINT16_MAX is the empty tag. Bypass that index rather than allowing
         * an empty entry to look like a hit, even for an oversized source. */
        if(cached && point != UINT16_MAX){
            StageVertexCacheEntry* entry = &cache[point & cache_mask];
            if(entry->point == point) smg_vertex_cache_stats.hits++;
            else {
                entry->xyz[0] = src[0];
                entry->xyz[1] = src[1];
                entry->xyz[2] = src[2];
                entry->point = point;
                smg_vertex_cache_stats.misses++;
            }
            src = entry->xyz;
        } else if(cached) smg_vertex_cache_stats.bypassed++;
        FramePoint* dst = &job->dst[slot];
        int32_t* out = (int32_t*)&dst->position;
        if(packed){
            /* Coordinates have two-byte alignment (six-byte source stride),
             * so retain halfword loads under the SDK's no-unaligned policy. */
            uint32_t xy = (uint16_t)src[0] | (uint32_t)(uint16_t)src[1] << 16;
            int32_t sz = src[2];
            for(unsigned row = 0; row < 3; row++)
                out[row] = job->m[row][3] +
                    (transform_dot_packed(xy, sz, packed_matrix[row], job->m[row][2]) >> 8);
        } else {
            int32_t sx = src[0], sy = src[1], sz = src[2];
            for(unsigned row = 0; row < 3; row++){
                const int32_t* m = job->m[row];
                int32_t dot = m[0] * sx + m[1] * sy + m[2] * sz;
                out[row] = m[3] + (dot >> 8);
            }
        }

        int32_t x = dst->position.x, y = dst->position.y, z = dst->position.z;
        uint8_t clip = 0;
        if(-x > z) clip |= 2;
        if(x > z) clip |= 1;
        if(-y > z) clip |= 4;
        if(y > z) clip |= 8;
        int32_t guard = z << 2;
        if(z < 160 || x > guard || -x > guard || y > guard || -y > guard)
            clip |= 16;
        dst->clip = clip;
        if(!(clip & 16))
            job->screen[slot] = screen_pack_position(project_to_screen(dst->position));
    }
}
#endif /* __GNW__ transforms */
