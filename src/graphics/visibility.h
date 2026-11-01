#ifndef SMG_POINT_CAPACITY
#if defined(__GNW__)
#define SMG_POINT_CAPACITY 5120
#else
#define SMG_POINT_CAPACITY 4096
#endif
#endif
#define MAX_POINTS              SMG_POINT_CAPACITY
#define BIN_COUNT               128
#define BIN_END                 0xFFFF
#define MAX_TRIANGLE_DEPTH_SPAN FXC(4.0f)
#define POINTS_SPLIT_RESERVE    384

typedef struct {
    v3 position;
    uint8_t clip, mark;
    uint16_t pad;
} FramePoint;

#ifndef SMG_VISIBLE_CAPACITY
#if defined(__GNW__)
#define SMG_VISIBLE_CAPACITY 8192
#else
#define SMG_VISIBLE_CAPACITY 4096
#endif
#endif
#define MAX_VISIBLE_TRIANGLES SMG_VISIBLE_CAPACITY
_Static_assert(SMG_POINT_CAPACITY < 65536 && SMG_VISIBLE_CAPACITY < 65536,
               "geometry indices must fit in uint16_t");

#define VISIBLE_CAP   (STAGE_MAX_TRIS < MAX_VISIBLE_TRIANGLES ? STAGE_MAX_TRIS : MAX_VISIBLE_TRIANGLES)
#define DIRECT_POINTS (STAGE_MAX_POINTS < STAGE_DIRECT_POINTS ? STAGE_MAX_POINTS : STAGE_DIRECT_POINTS)

#define FAR_DIRECT_MARKS                                                                                     \
    (STAGE_FAR_MAX_POINTS < STAGE_FAR_DIRECT_POINTS ? STAGE_FAR_MAX_POINTS : STAGE_FAR_DIRECT_POINTS)

#define FAR_POINTS_RESERVE g_far_reserve

/* The culler reads this descriptor in place and publishes leaf_count after
 * traversal. Its leaves output storage must not overlap the descriptor. */
typedef struct {
    const StageChunk* chunks;
    uint32_t n;
    int32_t planes[5][7];
    int32_t z_drop;
    int32_t cam[3];
    uint16_t* leaves;
    uint32_t leaf_count;
    int32_t z_inner;
} CullJob;

typedef struct {
    const uint16_t* leaves;
    int32_t leaf_count;
    const uint16_t* leaf_triangle_offsets;
    const LeafTriangle* recs;
    const uint16_t* ranks;
    int32_t limit;
    int32_t cam[3];
    uint8_t* marks;
    uint32_t stamp;
    uint32_t* bits;
    uint32_t lo, hi;
    uint16_t* plist;
    const uint16_t* plist_end;
    const uint16_t* home;
    const int16_t (*points)[3];
    const StageTriangle* triangles;
    uint32_t flags;
    int32_t axis_coord;
    int32_t axis_sign;
} CollectJob;

extern const uint32_t geometry_collect_leaves_end[], __iwram_start[], __iwram_lma[];
extern const uint32_t __visibility_overlay_start[], __visibility_overlay_end[];
extern const uint32_t __visibility_direct_overlay_start[], __visibility_direct_overlay_end[];

#if defined(__GNW__) && defined(SMG_PARTIAL_CHUNK_CACHE) && SMG_PARTIAL_CHUNK_CACHE
void smg_visibility_set_partial_chunk_cache(void* memory, uint32_t bytes,
                                             const StageChunk* main_source, uint32_t main_count,
                                             const StageChunk* far_source, uint32_t far_count);
#endif

typedef struct {
    const uint16_t* visible;
    int32_t n;
    const StageTriangle* tris;
    const uint16_t* home;
    const uint16_t* leaf_first;
    uint8_t* leaf_stamps;
    int16_t* leaf_delta;
    FramePoint* pool;
    uint32_t stamp;
    uint16_t* plist;
    uint32_t next_slot;
    uint32_t slot_limit;
#if defined(__GNW__)
    uint32_t slot_fast_limit;
#endif
} ClaimJob;

typedef struct {
    const int16_t (*src)[3];
    FramePoint* dst;
    uint32_t* screen;
    const uint16_t* list;
    int32_t n;
    int32_t m[3][4];
    const uint16_t* home;
    const int16_t* delta;
} TransformJob;

static EWRAM_BSS FramePoint g_frame_points[MAX_POINTS];
static EWRAM_BSS uint32_t g_screen_positions[MAX_POINTS];
static EWRAM_BSS uint16_t g_visible_triangles[VISIBLE_CAP];
static EWRAM_BSS uint16_t g_point_list[MAX_POINTS];
static EWRAM_BSS uint16_t g_visible_leaves[STAGE_MAX_LEAVES];
static EWRAM_BSS uint8_t g_point_marks[(DIRECT_POINTS + 3) & ~3];
static EWRAM_BSS uint8_t g_far_point_marks[(FAR_DIRECT_MARKS + 4) & ~3];
static EWRAM_BSS uint8_t g_leaf_stamps[(STAGE_MAX_HOME_LEAVES + 4) & ~3];
static EWRAM_BSS int16_t g_leaf_delta[STAGE_MAX_HOME_LEAVES + 1];
static int g_far_reserve;
static int g_far_base;
EWRAM_BSS uint16_t g_triangle_bin_heads[2 * BIN_COUNT];
static EWRAM_BSS Figure* g_figure_bin_heads[BIN_COUNT];
static uint8_t g_point_stamp;
static int g_point_count, g_farthest_visible_bin;

void smg_visibility_init(void);
static void* geometry_point_scratch(void);
IWRAM_CODE static int depth_bin_for_z(fx z);
int geometry_cull_chunks(CullJob* job);
static int collect_visible_chunks(const Stage* st, const CameraView* cv, v3 cam, int farthest_bin,
                                  int inner_bin);
IWRAM_CODE static int view_nearest_axis(v3 dir);
void geometry_collect_leaves(CollectJob* job);
int geometry_claim_points(ClaimJob* job);
static int collect_visible_triangles(const Stage* st, int leaf_count, int axis, int32_t limit, v3 cam,
                                     int* n_plist, int* pool_used, uint8_t* marks, int pool_free);
void engine_transform_points(const TransformJob* job);
static TransformJob geometry_transform_job(const int16_t (*src)[3], FramePoint* dst, uint32_t* scr,
                                           const uint16_t* list, int n, const xform* t, int shift);
