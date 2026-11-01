#define COL_MAX_CANDIDATES 128

typedef struct {
    const CollisionFace* faces;
    const int16_t* points;
    const CollisionNode* nodes;
    uint32_t node_count;
    uint32_t triangle_count;
    uint16_t scenario;
    uint8_t shift;
} CollisionWorld;

typedef struct {
    uint16_t index[COL_MAX_CANDIDATES];
    int count;
} CollisionCandidates;

#if defined(__GNW__)
/* Eight recent losses of standing support, retained across respawns. The
 * sequence is published last; inspect while halted for a coherent record. */
#define COLLISION_INCIDENTS 8
typedef struct {
    volatile uint32_t sequence;
    uint32_t tick;
    int32_t stage, scenario;
    uint32_t hidden_parts, flags;
    v3 old_feet, feet, up, from, to;
    int32_t hit_triangle, hit_t;
    v3 hit_normal;
    CollisionCandidates candidates;
} CollisionIncident;
extern CollisionIncident smg_collision_incidents[COLLISION_INCIDENTS];
extern volatile uint32_t smg_collision_incident_sequence;
#endif

typedef struct {
    v3 normal;
    v3 point;
    fx depth;
    uint16_t attr;
    int16_t tri;
} SphereContact;

typedef struct {
    v3 point, normal;
    int32_t t;
    uint16_t attr;
    int16_t tri;
} RayHit;

#if defined(__GNW__)
static void collision_record_support_loss(v3 old_feet, v3 feet, v3 up, v3 from, v3 to,
                                          const CollisionCandidates* candidates,
                                          bool ground_hit, const RayHit* hit);
#endif

#define COL_DYN_BASE 30720
#define COL_DYN_MAX  192
#define COL_DYN_SETS 24

typedef struct {
    int16_t lo[3], hi[3];
    uint16_t first, count;
    uint8_t on, pad;
} ColDynSet;

static EWRAM_BSS CollisionWorld g_collision;
static uint32_t g_col_parts_hidden;
static EWRAM_BSS CollisionTriangle g_col_dyn[COL_DYN_MAX];
static EWRAM_BSS ColDynSet g_col_dyn_sets[COL_DYN_SETS];
static int g_col_dyn_sets_used, g_col_dyn_used;

static inline const CollisionTriangle* col_tri(int i);
static void collision_load(const StageHeader* header);
IWRAM_FN static void collision_gather(v3 lo, v3 hi, CollisionCandidates* out);
static void collision_gather_around(v3 a, v3 b, fx radius, CollisionCandidates* out);
IWRAM_CODE static inline v3 col_vertex(const CollisionTriangle* t, int i, v3 origin);
IWRAM_CODE static inline int64_t dot64(v3 a, v3 b);
IWRAM_CODE static int32_t ratio14(int64_t num, int64_t den);
IWRAM_CODE static v3 lerp_point(v3 a, v3 b, int64_t num, int64_t den);
IWRAM_CODE static v3 closest_point_to_origin(v3 a, v3 b, v3 c, v3 projected);
IWRAM_FN static bool collision_sphere_triangle(int tri, v3 centre, fx radius, SphereContact* out);
IWRAM_FN static bool collision_raycast(const CollisionCandidates* cand, v3 from, v3 to, RayHit* hit,
                                       uint16_t skip_attr);
static bool collision_ray(v3 from, v3 to, RayHit* hit, uint16_t skip_attr);
