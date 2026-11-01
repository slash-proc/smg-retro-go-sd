static EWRAM_BSS CollisionWorld g_collision;

static uint32_t g_col_parts_hidden;
static EWRAM_BSS CollisionTriangle g_col_dyn[COL_DYN_MAX];
static EWRAM_BSS ColDynSet g_col_dyn_sets[COL_DYN_SETS];
static int g_col_dyn_sets_used, g_col_dyn_used;
#if defined(__GNW__)
CollisionIncident smg_collision_incidents[COLLISION_INCIDENTS] EWRAM_BSS;
volatile uint32_t smg_collision_incident_sequence;

static void collision_record_support_loss(v3 old_feet, v3 feet, v3 up, v3 from, v3 to,
                                          const CollisionCandidates* candidates,
                                          bool ground_hit, const RayHit* hit){
    uint32_t sequence = smg_collision_incident_sequence + 1;
    CollisionIncident* incident = &smg_collision_incidents[(sequence - 1) % COLLISION_INCIDENTS];
    incident->sequence = 0;
    incident->tick = g_game.tick;
    incident->stage = g_game.stage_index;
    incident->scenario = g_game.scenario;
    incident->hidden_parts = g_col_parts_hidden;
    incident->flags = (ground_hit ? 1u : 0u) | (g_game.player.has_floor_below ? 2u : 0u);
    incident->old_feet = old_feet;
    incident->feet = feet;
    incident->up = up;
    incident->from = from;
    incident->to = to;
    incident->hit_triangle = ground_hit ? hit->tri : -1;
    incident->hit_t = ground_hit ? hit->t : -1;
    incident->hit_normal = ground_hit ? hit->normal : V3(0, 0, 0);
    incident->candidates.count = candidates->count;
    memcpy(incident->candidates.index, candidates->index,
           (unsigned)candidates->count * sizeof candidates->index[0]);
    __asm__ volatile("" ::: "memory");
    incident->sequence = sequence;
    smg_collision_incident_sequence = sequence;
}
#endif
__attribute__((noinline)) static const CollisionTriangle* col_face_expand(int i){
    CollisionTriangle* t = &g_icestep_triangle;
    const CollisionFace* f = &g_collision.faces[i];
    for(int k = 0; k < 3; k++){
        const int16_t* p = g_collision.points + 3 * f->point[k];
        t->v[k][0] = p[0];
        t->v[k][1] = p[1];
        t->v[k][2] = p[2];
        t->normal[k] = f->normal[k];
    }
    t->attr = f->attr;
    t->floor_code = f->floor_code;
    t->wall_code = f->wall_code;
    return t;
}

__attribute__((noinline)) static const CollisionTriangle* col_tri_dynamic(int i){
    if((uint32_t)i < g_collision.triangle_count) return col_face_expand(i);
    if(dyncol_deferred_part((uint32_t)(i - DYNCOL_BASE)))
        dyncol_build_deferred(g_dyncol_tri_pending[i - DYNCOL_BASE] - 1);
    if(icestep_stage() && g_icestep_alive && i >= ICESTEP_BASE &&
       i < ICESTEP_BASE + ICESTEP_SLOTS * ICESTEP_PRISMS)
        return icestep_tri(i);
    return i >= COL_DYN_BASE  ? &g_col_dyn[i - COL_DYN_BASE]
           : i >= DYNCOL_BASE ? &g_dyncol_tris[i - DYNCOL_BASE]
                              : col_face_expand(i);
}
static inline const CollisionTriangle* col_tri(int i){
    if(i < DYNCOL_BASE) return col_face_expand(i);
    return col_tri_dynamic(i);
}

typedef struct {
    const int16_t* v[3];
    const int16_t* normal;
    uint16_t attr;
} ColFaceRef;
__attribute__((noinline)) static void col_face_ref_dynamic(int i, ColFaceRef* r){
    const CollisionTriangle* t = col_tri_dynamic(i);
    r->v[0] = t->v[0];
    r->v[1] = t->v[1];
    r->v[2] = t->v[2];
    r->normal = t->normal;
    r->attr = t->attr;
}
IWRAM_CODE static inline void col_face_ref(int i, ColFaceRef* r){
    if(i >= DYNCOL_BASE){
        _Static_assert(DYNCOL_BASE + DYNCOL_MAX <= ICESTEP_BASE && DYNCOL_BASE + DYNCOL_MAX <= COL_DYN_BASE,
                       "dynamic collision pool must not overlap other ranges");
        uint32_t k = (uint32_t)(i - DYNCOL_BASE);
        if(k < DYNCOL_MAX && (uint32_t)i >= g_collision.triangle_count){
            if(g_dyncol_tri_pending[k]) dyncol_build_deferred(g_dyncol_tri_pending[k] - 1);
            const CollisionTriangle* t = &g_dyncol_tris[k];
            r->v[0] = t->v[0];
            r->v[1] = t->v[1];
            r->v[2] = t->v[2];
            r->normal = t->normal;
            r->attr = t->attr;
            return;
        }
        col_face_ref_dynamic(i, r);
        return;
    }
    const CollisionFace* f = &g_collision.faces[i];
    const int16_t* p = g_collision.points;
    r->v[0] = p + 3 * f->point[0];
    r->v[1] = p + 3 * f->point[1];
    r->v[2] = p + 3 * f->point[2];
    r->normal = f->normal;
    r->attr = f->attr;
}
#undef COL_TRI
#define COL_TRI(i) col_tri(i)

static void collision_load(const StageHeader* header){
    const uint8_t* blob = (const uint8_t*)header;
    g_collision.faces = (const CollisionFace*)(blob + header->collision_triangles);
    g_collision.points = (const int16_t*)(blob + header->points);
    g_collision.nodes = (const CollisionNode*)(blob + header->collision_nodes);
    g_collision.node_count = header->collision_node_count;
    g_collision.triangle_count = header->collision_triangle_count;
    g_collision.shift = header->point_shift;
    g_collision.scenario =
        header->surface_features & STAGE_SCENARIO_SURFACES ? (uint16_t)(1u << (g_game.scenario + 9)) : 0;
    g_col_parts_hidden = 0;
    g_col_dyn_sets_used = g_col_dyn_used = 0;
}

static inline int32_t clamp16(int32_t x){ return x < -32768 ? -32768 : x > 32767 ? 32767 : x; }

IWRAM_FN static void collision_gather(v3 lo, v3 hi, CollisionCandidates* out){
    uint16_t scenario = g_collision.scenario;
    int s = g_collision.shift, r = (1 << s) - 1;
    int32_t l0 = clamp16(lo.x >> s), l1 = clamp16(lo.y >> s), l2 = clamp16(lo.z >> s);
    int32_t h0 = clamp16((hi.x + r) >> s), h1 = clamp16((hi.y + r) >> s), h2 = clamp16((hi.z + r) >> s);
    const CollisionNode* nodes = g_collision.nodes;
    uint32_t n = g_collision.node_count, i = 0;
    int count = 0;
    while(i < n && count < COL_MAX_CANDIDATES){
        const CollisionNode* node = &nodes[i];
        if(node->min[0] > h0 || node->max[0] < l0 || node->min[1] > h1 || node->max[1] < l1 ||
           node->min[2] > h2 || node->max[2] < l2 ||
           (node->part && (g_col_parts_hidden >> (node->part - 1) & 1))){
            i = node->count ? i + 1 : node->next;
            continue;
        }
        if(node->count){
            if(scenario){
                for(int k = 0; k < node->count && count < COL_MAX_CANDIDATES; k++){
                    uint16_t tri = (uint16_t)(node->next + k);
                    uint16_t flags = g_collision.faces[tri].attr;
                    if(!(flags & COL_SCENARIO_MASK) || (flags & scenario)) out->index[count++] = tri;
                }
            } else {
                for(int k = 0; k < node->count && count < COL_MAX_CANDIDATES; k++)
                    out->index[count++] = (uint16_t)(node->next + k);
            }
        }
        i++;
    }
    for(int k = 0; k < g_col_dyn_sets_used; k++){
        const ColDynSet* d = &g_col_dyn_sets[k];
        if(!d->on || d->lo[0] > h0 || d->hi[0] < l0 || d->lo[1] > h1 || d->hi[1] < l1 || d->lo[2] > h2 ||
           d->hi[2] < l2)
            continue;

        for(int t = 0; t < d->count && count < COL_MAX_CANDIDATES; t++)
            out->index[count++] = (uint16_t)(COL_DYN_BASE + d->first + t);
    }
    out->count = dyncol_gather(lo, hi, out->index, count, COL_MAX_CANDIDATES);
}

static void collision_gather_around(v3 a, v3 b, fx radius, CollisionCandidates* out){
    v3 lo = V3(fx_min(a.x, b.x) - radius, fx_min(a.y, b.y) - radius, fx_min(a.z, b.z) - radius);
    v3 hi = V3(fx_max(a.x, b.x) + radius, fx_max(a.y, b.y) + radius, fx_max(a.z, b.z) + radius);
    collision_gather(lo, hi, out);
}

IWRAM_CODE static inline v3 col_vertex(const CollisionTriangle* t, int i, v3 origin){
    int s = g_collision.shift;
    return V3((t->v[i][0] << s) - origin.x, (t->v[i][1] << s) - origin.y, (t->v[i][2] << s) - origin.z);
}
IWRAM_CODE static inline v3 col_ref_vertex(const ColFaceRef* r, int i, v3 origin){
    int s = g_collision.shift;
    const int16_t* p = r->v[i];
    return V3((p[0] << s) - origin.x, (p[1] << s) - origin.y, (p[2] << s) - origin.z);
}

IWRAM_CODE static inline int64_t dot64(v3 a, v3 b){
    return (int64_t)a.x * b.x + (int64_t)a.y * b.y + (int64_t)a.z * b.z;
}

#if defined(__GNW__)
/* Only the sign is needed. Keep cross products unrounded: Q8 component
 * truncation can reverse a near-edge result and reject a real floor hit.
 * The third vector is the ray direction, not a rounded intersection normal.
 * Typical local edges fit an int64 sum; a two-limb fallback preserves the
 * sign even for full signed-32 inputs without overflowing triple products. */
IWRAM_CODE static int col_ray_edge_sign(v3 a, v3 b, v3 n){
    int64_t cross[3] = {
        n.x ? (int64_t)a.y * b.z - (int64_t)a.z * b.y : 0,
        n.y ? (int64_t)a.z * b.x - (int64_t)a.x * b.z : 0,
        n.z ? (int64_t)a.x * b.y - (int64_t)a.y * b.x : 0};
    uint64_t magnitude[3];
    for(unsigned i = 0; i < 3; i++)
        magnitude[i] = cross[i] < 0 ? 0 - (uint64_t)cross[i] : (uint64_t)cross[i];
    uint64_t combined = magnitude[0] | magnitude[1] | magnitude[2];
    uint32_t normal_size = (n.x < 0 ? 0u - (uint32_t)n.x : (uint32_t)n.x) |
        (n.y < 0 ? 0u - (uint32_t)n.y : (uint32_t)n.y) |
        (n.z < 0 ? 0u - (uint32_t)n.z : (uint32_t)n.z);
    if((!(normal_size >> 15) && !(combined >> 46)) || !(combined >> 30)){
        int64_t value = cross[0] * n.x + cross[1] * n.y + cross[2] * n.z;
        return (value > 0) - (value < 0);
    }
    const int32_t normal[3] = {n.x, n.y, n.z};
    uint64_t low = 0;
    int64_t high = 0;
    for(unsigned i = 0; i < 3; i++){
        uint32_t factor = normal[i] < 0 ? 0u - (uint32_t)normal[i] : (uint32_t)normal[i];
        uint64_t p0 = (uint64_t)(uint32_t)magnitude[i] * factor;
        uint64_t p1 = (magnitude[i] >> 32) * factor;
        uint64_t part_low = p0 + (p1 << 32);
        int64_t part_high = (int64_t)(p1 >> 32) + (part_low < p0);
        if((cross[i] < 0) != (normal[i] < 0)){
            part_low = 0 - part_low;
            part_high = -part_high - (part_low != 0);
        }
        uint64_t previous = low;
        low += part_low;
        high += part_high + (low < previous);
    }
    return high < 0 ? -1 : (high || low ? 1 : 0);
}
#endif

IWRAM_CODE static int32_t ratio14(int64_t num, int64_t den){
    if(den <= 0) return 0;
    while(den >= (1 << 17)){
        num >>= 1;
        den >>= 1;
    }
    if(den == 0) return 0;
    if(num >= den) return N14_ONE;
    if(num <= 0) return 0;

    uint32_t rem = (uint32_t)num, d = (uint32_t)den, q = 0;
    for(int k = 0; k < 14; k++){
        rem <<= 1;
        q <<= 1;
        if(rem >= d){
            rem -= d;
            q |= 1;
        }
    }
    return (int32_t)q;
}

IWRAM_CODE static v3 lerp_point(v3 a, v3 b, int64_t num, int64_t den){
    if(den == 0) return a;
    int32_t t = ratio14(num, den);
    return V3(a.x + mul14(b.x - a.x, t), a.y + mul14(b.y - a.y, t), a.z + mul14(b.z - a.z, t));
}

IWRAM_CODE static v3 closest_point_to_origin(v3 a, v3 b, v3 c, v3 projected){
    v3 ab = v3_sub(b, a), ac = v3_sub(c, a);
    int64_t d1 = -dot64(ab, a), d2 = -dot64(ac, a);
    if(d1 <= 0 && d2 <= 0) return a;
    int64_t d3 = -dot64(ab, b), d4 = -dot64(ac, b);
    if(d3 >= 0 && d4 <= d3) return b;
    int64_t d5 = -dot64(ab, c), d6 = -dot64(ac, c);

    uint64_t big = (uint64_t)((d1 ^ (d1 >> 63)) | (d2 ^ (d2 >> 63)) | (d3 ^ (d3 >> 63)) | (d4 ^ (d4 >> 63)) |
                              (d5 ^ (d5 >> 63)) | (d6 ^ (d6 >> 63)));
    int sh = big >> 30 ? 34 - __builtin_clzll(big) : 0;
    int64_t s1 = d1 >> sh, s2 = d2 >> sh, s3 = d3 >> sh, s4 = d4 >> sh;
    int64_t vc = s1 * s4 - s3 * s2;
    if(vc <= 0 && d1 >= 0 && d3 <= 0) return lerp_point(a, b, d1, d1 - d3);
    if(d6 >= 0 && d5 <= d6) return c;
    int64_t s5 = d5 >> sh, s6 = d6 >> sh;
    int64_t vb = s5 * s2 - s1 * s6;
    if(vb <= 0 && d2 >= 0 && d6 <= 0) return lerp_point(a, c, d2, d2 - d6);
    int64_t va = s3 * s6 - s5 * s4;
    if(va <= 0 && (d4 - d3) >= 0 && (d5 - d6) >= 0) return lerp_point(b, c, d4 - d3, (d4 - d3) + (d5 - d6));
    return projected;
}

IWRAM_FN static bool collision_sphere_triangle(int tri, v3 centre, fx radius, SphereContact* out){
    if(tri >= DYNCOL_BASE && (uint32_t)tri >= g_collision.triangle_count){
        const DynColGroup* d = dyncol_deferred_part((uint32_t)(tri - DYNCOL_BASE));
        if(d && !dyncol_sphere_meets(d, centre, radius)) return false;
    }
    ColFaceRef r;
    col_face_ref(tri, &r);
    const ColFaceRef* t = &r;
    v3 n = v3_from_i16(t->normal);
    v3 a = col_ref_vertex(t, 0, centre);
    fx dist = -v3_dot14(a, n);

    if(dist > radius || dist < 0) return false;
    v3 b = col_ref_vertex(t, 1, centre), c = col_ref_vertex(t, 2, centre);
    v3 q = closest_point_to_origin(a, b, c, v3_scale14(n, -dist));
    uint64_t d2 = v3_length_squared64(q);
    if(d2 > (uint64_t)((int64_t)radius * radius)) return false;
    fx d = (fx)isqrt64(d2);
    out->normal = d > 2 && dist > 0 ? v3_direction14(v3_neg(q), n) : n;
    out->depth = dist >= 0 ? radius - d : radius + d;
    out->point = v3_add(centre, q);
    out->attr = t->attr;
    out->tri = (int16_t)tri;
    return true;
}

IWRAM_FN static bool collision_raycast(const CollisionCandidates* cand, v3 from, v3 to, RayHit* hit,
                                       uint16_t skip_attr){
    v3 d = v3_sub(to, from);
    int32_t best = N14_ONE + 1;
    const DynColGroup* missed = NULL;
    for(int i = 0; i < cand->count; i++){
        int tri = cand->index[i];
        if(tri >= DYNCOL_BASE && (uint32_t)tri >= g_collision.triangle_count){
            const DynColGroup* g = dyncol_deferred_part((uint32_t)(tri - DYNCOL_BASE));
            if(g && (g == missed || !dyncol_segment_meets(g, from, to))){
                missed = g;
                continue;
            }
        }
        ColFaceRef r;
        col_face_ref(tri, &r);
        const ColFaceRef* t = &r;
        if(t->attr & skip_attr) continue;
        v3 n = v3_from_i16(t->normal);
        v3 a = col_ref_vertex(t, 0, from);
        /* Preserve signed sub-unit distances until after the crossing test.
         * Per-component Q14 truncation can turn a point just below a sloping
         * floor into zero, incorrectly rejecting a valid support ray. The
         * common normal scale cancels in ratio14(). Include endpoint contact,
         * but reject coplanar segments rather than reporting arbitrary t=0. */
        int64_t d0 = -dot64(a, n);
        int64_t d1 = d0 + dot64(d, n);
        if(d0 < 0 || d1 > 0 || d0 == d1) continue;
        int32_t tt = ratio14(d0, (int64_t)d0 - d1);
        if(tt >= best) continue;

        v3 p = V3((int32_t)(((int64_t)d.x * tt + 8192) >> 14), (int32_t)(((int64_t)d.y * tt + 8192) >> 14),
                  (int32_t)(((int64_t)d.z * tt + 8192) >> 14));
        v3 b = col_ref_vertex(t, 1, from), c = col_ref_vertex(t, 2, from);
#if defined(__GNW__)
        /* Oriented volumes test the line against the actual triangle edges.
         * Rounding p first can move it outside a thin triangle. Crossing and
         * returned contact remain governed by the existing plane/Q14 rules. */
        int w0 = col_ray_edge_sign(a, b, d), w1 = col_ray_edge_sign(b, c, d),
            w2 = col_ray_edge_sign(c, a, d);
        if(!(w0 || w1 || w2)) continue; /* coplanar line / degenerate face */
#else
        v3 pa = v3_sub(a, p), pb = v3_sub(b, p), pc = v3_sub(c, p);
        int64_t w0 = dot64(v3_cross(pa, pb), n), w1 = dot64(v3_cross(pb, pc), n),
                w2 = dot64(v3_cross(pc, pa), n);
#endif
        if(!((w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0))) continue;
        best = tt;
        hit->t = tt;
        hit->point = v3_add(from, p);
        hit->normal = n;
        hit->attr = t->attr;
        hit->tri = (int16_t)tri;
    }
    return best <= N14_ONE;
}

static bool collision_ray(v3 from, v3 to, RayHit* hit, uint16_t skip_attr){
    CollisionCandidates cand;
    collision_gather_around(from, to, FXC(0.05f), &cand);
    return collision_raycast(&cand, from, to, hit, skip_attr);
}
