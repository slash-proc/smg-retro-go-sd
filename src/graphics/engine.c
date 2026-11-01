_Static_assert(sizeof(DrawEntry) == 32, "raster.s assumes 32-byte draw entries");

#ifndef SMG_ARENA_CAPACITY
#if defined(__GNW__)
#define SMG_ARENA_CAPACITY 3584
#else
#define SMG_ARENA_CAPACITY 2048
#endif
#endif
#define ENGINE_ARENA_MAX SMG_ARENA_CAPACITY
_Static_assert(ENGINE_ARENA_MAX < 65535, "draw indices must leave BIN_END reserved");
DrawEntry g_arena[ENGINE_ARENA_MAX] EWRAM_BSS;
int g_arena_count;
uint8_t* g_engine_fb;

#if defined(__GNW__)
static bool g_depth_enabled;
static bool g_depth_active;
/* Global textured-surface mapping choice. Unconfigured runtimes default to
 * Accurate; Fast remains available from the pause menu. */
static volatile bool g_perspective_enabled = true;
static bool g_perspective_active;
static uint16_t g_depth_buffer[FRAME_WIDTH * FRAME_HEIGHT] EWRAM_BSS;
static uint32_t g_arena_depth[ENGINE_ARENA_MAX][3] EWRAM_BSS;
static uint32_t g_pending_depth[3];
static const uint32_t* g_raster_depth;
#define DEPTH_EFFECT_MAX 256
typedef struct {
    DrawEntry entry;
    uint32_t depth[3];
    TextureReference texture;
    const uint16_t* additive_rows;
    const uint8_t* additive_table;
} DepthEffect;
static DepthEffect g_depth_effects[DEPTH_EFFECT_MAX] EWRAM_BSS;
static unsigned g_depth_effect_count;
static bool g_depth_flushing;

static inline void engine_set_depth(fx a, fx b, fx c){
    if(!g_depth_active && !g_perspective_active) return;
    const fx z[3] = {a, b, c};
    for(unsigned i = 0; i < 3; i++)
        g_pending_depth[i] = (UINT32_C(1) << 30) / (uint32_t)(z[i] < CLIP_NEAR_Z ? CLIP_NEAR_Z : z[i]);
}

static void engine_depth_begin(void){
    g_depth_active = g_depth_enabled;
    g_perspective_active = g_perspective_enabled;
    g_depth_effect_count = 0;
    g_depth_flushing = false;
    if(g_depth_active) memset(g_depth_buffer, 0, sizeof g_depth_buffer);
}
static void engine_depth_end(void);
#else
#define engine_set_depth(a,b,c) ((void)0)
#define engine_depth_begin() ((void)0)
#define engine_depth_end() ((void)0)
#endif

int32_t g_flat_small = 1;
static const uint8_t g_no_average[STAGE_MAX_TEXTURES] = {0};

IWRAM_CODE static inline int32_t screen_area(uint32_t a, uint32_t b, uint32_t c){
    int32_t ax = screen_x(a), ay = screen_y(a), bx = screen_x(b), by = screen_y(b), cx = screen_x(c),
            cy = screen_y(c);
    return (ay - cy) * (ax - bx) - (ax - cx) * (ay - by);
}

IWRAM_CODE static inline bool screen_rowless(uint32_t a, uint32_t b, uint32_t c){
    int32_t y0 = screen_y(a), y1 = screen_y(b), y2 = screen_y(c);
    int32_t lo = y0 < y1 ? y0 : y1, hi = y0 < y1 ? y1 : y0;
    if(y2 < lo) lo = y2;
    if(y2 > hi) hi = y2;
    return ((lo + 7) >> 4) >= ((hi + 7) >> 4) || hi < -7 || lo >= FRAME_HEIGHT * 16 - 8;
}

IWRAM_CODE static inline bool screen_small(uint32_t a, uint32_t b, uint32_t c){
    int32_t x0 = screen_x(a), x1 = screen_x(b), x2 = screen_x(c);
    int32_t lo = x0 < x1 ? x0 : x1, hi = x0 < x1 ? x1 : x0;
    if(x2 < lo) lo = x2;
    if(x2 > hi) hi = x2;
    if(hi - lo > 4 * 16) return false;
    int32_t y0 = screen_y(a), y1 = screen_y(b), y2 = screen_y(c);
    lo = y0 < y1 ? y0 : y1;
    hi = y0 < y1 ? y1 : y0;
    if(y2 < lo) lo = y2;
    if(y2 > hi) hi = y2;
    return hi - lo <= 3 * 16;
}

static int g_engine_bin_offset;

IWRAM_CODE static void engine_push(int bin, uint32_t sa, uint32_t sb, uint32_t sc, uint32_t uva, uint32_t uvb,
                                   uint32_t uvc, unsigned mode, unsigned colour,
                                   const TextureReference* texture){
    int n = g_arena_count;
    SMG_COUNT(arena_attempts);
    if(n >= ENGINE_ARENA_MAX){
        SMG_COUNT(arena_dropped);
        return;
    }
    g_arena_count = n + 1;
    SMG_PEAK(arena_peak, n + 1);
    DrawEntry* e = &g_arena[n];
    bin += g_engine_bin_offset;
    e->next = g_triangle_bin_heads[bin];
    g_triangle_bin_heads[bin] = (uint16_t)n;
    e->mode = (uint8_t)mode;
    e->colour = (uint8_t)colour;
    e->s[0] = sa;
    e->s[1] = sb;
    e->s[2] = sc;
    e->uv[0] = uva;
    e->uv[1] = uvb;
    e->uv[2] = uvc;
    e->texture = texture;
#if defined(__GNW__)
    if(g_depth_active || g_perspective_active) memcpy(g_arena_depth[n], g_pending_depth, sizeof g_pending_depth);
#endif
}

IWRAM_CODE static void engine_out(int bin, uint32_t sa, uint32_t sb, uint32_t sc, uint32_t uva, uint32_t uvb,
                                  uint32_t uvc, unsigned mode, unsigned colour,
                                  const TextureReference* texture){
    if(bin >= 0){
        engine_push(bin, sa, sb, sc, uva, uvb, uvc, mode, colour, texture);
        return;
    }
    DrawEntry e = {0xFFFF, (uint8_t)mode, (uint8_t)colour, {sa, sb, sc}, {uva, uvb, uvc}, texture};
#if defined(__GNW__)
    g_raster_depth = g_pending_depth;
#endif
    engine_raster(&e);
}

IWRAM_FN static int32_t engine_ratio16(uint32_t num, uint32_t den){
    if(!den) return 0;
#if defined(__GNW__)
    uint64_t ratio = ((uint64_t)num << 16) / den;
    return ratio > 65536 ? 65536 : (int32_t)ratio;
#else
    int e = 0;
    uint32_t m = den;
    while(m >= 512){
        m >>= 1;
        e++;
    }
    while(m < 256){
        m <<= 1;
        e--;
    }
    uint64_t r = (uint64_t)num * engine_recip_table[m - 256];
    int sh = 8 + e;
    int32_t t = sh >= 0 ? (int32_t)(r >> sh) : (int32_t)(r << -sh);
    return t > 65536 ? 65536 : t;
#endif
}

#if defined(__GNW__)
__attribute__((always_inline)) static inline int32_t clip_distance(const v3* p, int k){
#else
IWRAM_FN static int32_t clip_distance(const v3* p, int k){
#endif
    switch(k){
    case 0: return p->z - CLIP_NEAR_Z;
    case 1: return (p->z << 2) - p->x;
    case 2: return (p->z << 2) + p->x;
    case 3: return (p->z << 2) - p->y;
    default: return (p->z << 2) + p->y;
    }
}

IWRAM_FN static void clip_lerp(ClipVertex* c, const ClipVertex* s, const ClipVertex* o, int32_t t, int k){
    const int32_t* sp = &s->p.x;
    const int32_t* op = &o->p.x;
    int32_t* cp = &c->p.x;
    for(int i = 0; i < 5; i++) cp[i] = sp[i] + (int32_t)(((int64_t)(op[i] - sp[i]) * t) >> 16);
    if(k == 0 && c->p.z < CLIP_NEAR_Z) c->p.z = CLIP_NEAR_Z;
}
_Static_assert(sizeof(ClipVertex) == 20 && offsetof(ClipVertex, v) == 16,
               "clip_lerp walks a ClipVertex as 5 words");

IWRAM_FN static int clip_polygon(ClipVertex* out, const ClipVertex* in, int n, int k){
    int m = 0;
    const ClipVertex* a = &in[n - 1];
    int32_t da = clip_distance(&a->p, k);
    for(int i = 0; i < n; i++){
        const ClipVertex* b = &in[i];
        int32_t db = clip_distance(&b->p, k);

        if(da >= 0 && db < 0)
            clip_lerp(&out[m++], a, b, engine_ratio16((uint32_t)da, (uint32_t)(da - db)), k);
        else if(da < 0 && db >= 0)
            clip_lerp(&out[m++], b, a, engine_ratio16((uint32_t)db, (uint32_t)(db - da)), k);
        if(db >= 0) out[m++] = *b;
        a = b;
        da = db;
    }
    return m;
}

static inline uint32_t pack_uv(int32_t u, int32_t v){ return ((uint32_t)u & 0xFFFF) | (uint32_t)v << 16; }

IWRAM_FN static void engine_emit_clipped(int bin, const ClipVertex* tri, unsigned mode, unsigned colour,
                                         const TextureReference* texture){
    ClipVertex buf[2][10];
    int n = 3;
    for(int i = 0; i < 3; i++) buf[0][i] = tri[i];
    int cur = 0;
    for(int k = 0; k < 5 && n >= 3; k++){
        bool out = false;
        for(int i = 0; i < n; i++)
            if(clip_distance(&buf[cur][i].p, k) < 0){
                out = true;
                break;
            }
        if(!out) continue;
        n = clip_polygon(buf[cur ^ 1], buf[cur], n, k);
        cur ^= 1;
    }
    if(n < 3) return;
    uint32_t s[10], uv[10];
    for(int i = 0; i < n; i++){
        s[i] = screen_pack_position(project_to_screen(buf[cur][i].p));
        uv[i] = pack_uv(buf[cur][i].u, buf[cur][i].v);
    }
    int32_t area = 0;
    for(int i = 1; i + 1 < n; i++) area += screen_area(s[0], s[i], s[i + 1]);
    if(area <= 0) return;
    for(int i = 1; i + 1 < n; i++){
        if(screen_area(s[0], s[i], s[i + 1]) <= 0 || screen_rowless(s[0], s[i], s[i + 1])) continue;
        engine_set_depth(buf[cur][0].p.z, buf[cur][i].p.z, buf[cur][i + 1].p.z);
        engine_out(bin, s[0], s[i], s[i + 1], uv[0], uv[i], uv[i + 1], mode, colour, texture);
    }
}

static inline const uint16_t* stage_water_table(const StageHeader* h){
    return h->texture_water ? (const uint16_t*)((const uint8_t*)h + h->texture_water) : NULL;
}

_Static_assert(offsetof(EmitJob, far) == 40 && offsetof(EmitJob, slow) == 60 &&
                   offsetof(EmitJob, camera_underwater) == 84 && offsetof(EmitJob, under_bins) == 88,
               "raster.s assumes the EmitJob layout");

static inline int emit_slot(const EmitJob* j, int point){
#if defined(__GNW__)
    if(j->home && !j->delta) return j->home[point];
#endif
    return j->home ? point + j->delta[j->home[point]] : point;
}

__attribute__((noinline)) static bool emit_through_water(const EmitJob* j, const StageTriangle* st){
    const StageHeader* h = g_game.stage.header;
    if(j->water == 3) return g_game.camera_underwater || (st->flags & TRI_WATER_TINT);
    const int16_t* a = j->stage_points[st->v[0].point];
    if(j->water == 2){
        int32_t dx = a[0] - h->water_centre[0], dy = a[1] - h->water_centre[1];
        if(((dx * dx) >> 8) + ((dy * dy) >> 8) > h->water_radius_squared || a[2] < h->water_min_z)
            return false;
    }
    int32_t max_z = a[2], z;
    if((z = j->stage_points[st->v[1].point][2]) > max_z) max_z = z;
    if((z = j->stage_points[st->v[2].point][2]) > max_z) max_z = z;
    return g_game.camera_underwater ? max_z >= h->water_level : max_z < h->water_level;
}

__attribute__((noinline)) static uint32_t emit_scroll_uv(const StageTriangle* st){
    const StageHeader* h = g_game.stage.header;
    const int16_t* speed =
        (const int16_t*)((const uint8_t*)h + h->texture_scroll) + 2 * stage_triangle_texture(st);
    int16_t du = (int16_t)((fx_mul(speed[0], g_game.time) % (32 * FX_ONE)) >> 4);
    int16_t dv = (int16_t)((fx_mul(speed[1], g_game.time) % (32 * FX_ONE)) >> 4);
    return pack_uv(du, dv);
}

static inline int32_t min3(int32_t a, int32_t b, int32_t c){
    int32_t m = a < b ? a : b;
    return m < c ? m : c;
}
static inline int32_t max3(int32_t a, int32_t b, int32_t c){
    int32_t m = a > b ? a : b;
    return m > c ? m : c;
}

IWRAM_CODE static inline bool emit_needs_subdivision(fx min_z, fx max_z){
    return max_z - min_z > fx_mul(fx_max(min_z, 0) + FXC(3.0f), MAX_TRIANGLE_DEPTH_SPAN);
}

typedef struct {
    v3 p;
    int32_t u, v;
    uint32_t clip;
    uint32_t s;
} SplitVertex;

IWRAM_FN static void emit_split_vertex(SplitVertex* o, v3 p, int32_t u, int32_t v){
    o->p = p;
    o->u = u;
    o->v = v;
    o->clip = view_clip_mask(p);
    o->s = o->clip & CLIP_GUARD ? 0 : screen_pack_position(project_to_screen(p));
}

IWRAM_FN static void emit_face(const EmitJob* j, int bin, const SplitVertex* a, const SplitVertex* b,
                               const SplitVertex* c, unsigned alpha, unsigned average,
                               const TextureReference* texture){
    if(a->clip & b->clip & c->clip & CLIP_SIDES) return;
    bool flat_far = !alpha && average && (j->far || bin < j->flat_bin);
    unsigned mode = alpha == 2 ? RM_BLEND_QUARTER : alpha ? RM_ALPHA : RM_TEX;
    if((a->clip | b->clip | c->clip) & CLIP_GUARD){
        ClipVertex t[3] = {{a->p, a->u, a->v}, {b->p, b->u, b->v}, {c->p, c->u, c->v}};
        engine_emit_clipped(bin, t, flat_far ? RM_FLAT : mode, average, texture);
        return;
    }
    if(screen_area(a->s, b->s, c->s) <= 0 || screen_rowless(a->s, b->s, c->s)) return;
    if(!alpha && average && (flat_far || (j->flat_small && screen_small(a->s, b->s, c->s)))) mode = RM_FLAT;
    engine_set_depth(a->p.z, b->p.z, c->p.z);
    engine_push(bin, a->s, b->s, c->s, pack_uv(a->u, a->v), pack_uv(b->u, b->v), pack_uv(c->u, c->v), mode,
                average, texture);
}

IWRAM_FN static void emit_subdivided(const EmitJob* j, SplitVertex tri[3], unsigned alpha, unsigned average,
                                     const TextureReference* texture){
    SplitVertex stack[16][3];
    int top = 0;
    for(int i = 0; i < 3; i++) stack[0][i] = tri[i];
    top = 1;
    while(top){
        SplitVertex* t = stack[--top];
        SplitVertex a = t[0], b = t[1], c = t[2];
        if(a.clip & b.clip & c.clip & CLIP_SIDES) continue;
        bool split = false;
        for(int rot = 0; rot < 3 && !split; rot++){
            if(emit_needs_subdivision(fx_min(a.p.z, b.p.z), fx_max(a.p.z, b.p.z)) &&
               emit_needs_subdivision(min3(a.p.z, b.p.z, c.p.z), max3(a.p.z, b.p.z, c.p.z)) &&
               top + 2 <= 16){
                SplitVertex m;
                SMG_COUNT(subdivisions);
                /* Average in signed32: valid signed16 UV endpoints can sum
                 * outside signed16. Wrapping before halving invents repeats. */
                emit_split_vertex(&m, V3((a.p.x + b.p.x) >> 1, (a.p.y + b.p.y) >> 1, (a.p.z + b.p.z) >> 1),
                                  (a.u + b.u) >> 1, (a.v + b.v) >> 1);
                SplitVertex* p = stack[top++];
                p[0] = c;
                p[1] = a;
                p[2] = m;
                p = stack[top++];
                p[0] = b;
                p[1] = c;
                p[2] = m;
                split = true;
            } else {
                SplitVertex r = a;
                a = b;
                b = c;
                c = r;
            }
        }
        if(split) continue;
        int bin = depth_bin_for_z(max3(a.p.z, b.p.z, c.p.z));
        if(bin < j->cut_bin) bin = j->cut_bin;
        emit_face(j, bin, &a, &b, &c, alpha, average, texture);
    }
}

__attribute__((noinline)) static void emit_stage_face_bins(const EmitJob* j, int face){
    const StageTriangle* st = &j->tris[face];
    const FramePoint* points = j->points;
    int ia = emit_slot(j, st->v[0].point), ib = emit_slot(j, st->v[1].point),
        ic = emit_slot(j, st->v[2].point);
    const FramePoint* pa = &points[ia];
    const FramePoint* pb = &points[ib];
    const FramePoint* pc = &points[ic];
    unsigned ca = pa->clip, cb = pb->clip, cc = pc->clip;
    if(ca & cb & cc & CLIP_SIDES) return;
    fx min_z = min3(pa->position.z, pb->position.z, pc->position.z);
    fx max_z = max3(pa->position.z, pb->position.z, pc->position.z);
    bool deep = !j->far && emit_needs_subdivision(min_z, max_z);
    int bin = depth_bin_for_z(max_z);
    if(!deep && (j->far ? bin >= j->cut_bin : bin < j->cut_bin)) return;

    unsigned texture = stage_triangle_texture(st);
    if(j->water && j->water_textures && emit_through_water(j, st)) texture = j->water_textures[texture];
    unsigned alpha = st->flags & TRI_BLEND_QUARTER ? 2 : !!(st->flags & TRI_ALPHA);
    unsigned average = j->average[texture];
    const TextureReference* source = &j->textures[texture];
    if(j->cache_slots){
        unsigned slot = j->cache_slots[texture];
        if(slot) source = &g_texture_cache_reference[slot - 1];
    } else if(j->far && (g_far_stage.header->surface_features & STAGE_PACKED_FAR_TEXTURES)){
        const TextureReference* raw = stage_far_raw_texture(texture);
        if(!raw && (alpha || !average)) return;
        source = raw;
    }
    uint32_t uva = pack_uv(st->v[0].u, st->v[0].v), uvb = pack_uv(st->v[1].u, st->v[1].v),
             uvc = pack_uv(st->v[2].u, st->v[2].v);
    if(st->flags & SURF_ANIM_UV){
        uint32_t d = emit_scroll_uv(st);
        uva = pack_uv((int16_t)(uva + d), (int16_t)((uva >> 16) + (d >> 16)));
        uvb = pack_uv((int16_t)(uvb + d), (int16_t)((uvb >> 16) + (d >> 16)));
        uvc = pack_uv((int16_t)(uvc + d), (int16_t)((uvc >> 16) + (d >> 16)));
    }
    if(deep || ((ca | cb | cc) & CLIP_GUARD)){
        SplitVertex v[3];
        const FramePoint* fp[3] = {pa, pb, pc};
        const uint32_t uv[3] = {uva, uvb, uvc};
        const int slots[3] = {ia, ib, ic};
        for(int i = 0; i < 3; i++){
            v[i].p = fp[i]->position;
            v[i].u = (int16_t)uv[i];
            v[i].v = (int16_t)(uv[i] >> 16);
            v[i].clip = fp[i]->clip;
            v[i].s = j->screen[slots[i]];
        }
        if(deep) emit_subdivided(j, v, alpha, average, source);
        else emit_face(j, bin, &v[0], &v[1], &v[2], alpha, average, source);
        return;
    }
    uint32_t sa = j->screen[ia], sb = j->screen[ib], sc = j->screen[ic];
    if(screen_area(sa, sb, sc) <= 0 || screen_rowless(sa, sb, sc)) return;
    unsigned mode = alpha == 2 ? RM_BLEND_QUARTER : alpha ? RM_ALPHA : RM_TEX;
    if(!alpha && average && (j->far || bin < j->flat_bin || (j->flat_small && screen_small(sa, sb, sc))))
        mode = RM_FLAT;
    engine_set_depth(pa->position.z, pb->position.z, pc->position.z);
    engine_push(bin, sa, sb, sc, uva, uvb, uvc, mode, average, source);
}
static void emit_stage_face(const EmitJob* j, int face){
    if(j->under_bins && (j->tris[face].flags & TRI_WATER_TINT)){
        g_engine_bin_offset = BIN_COUNT;
        emit_stage_face_bins(j, face);
        g_engine_bin_offset = 0;
    } else emit_stage_face_bins(j, face);
}

static const EmitJob* g_emit_job;
static void emit_stage_slow(int face){ emit_stage_face(g_emit_job, face); }

static void emit_stage_triangles(EmitJob* j, const uint16_t* visible, int count){
    const StageHeader* h = g_game.stage.header;
    j->slow = emit_stage_slow;
    j->water_cx = h->water_centre[0];
    j->water_cy = h->water_centre[1];
    j->water_r2 = h->water_radius_squared;
    j->water_min_z = h->water_min_z;
    j->water_level = h->water_level;
    j->camera_underwater = g_game.camera_underwater;
    g_emit_job = j;
    engine_emit_stage(j, visible, count);
}

#if defined(__GNW__)
/* Debug-console A/B control; use one runtime and scene for dispatch timings. */
static volatile bool g_stage_fast_enabled = true;

/* Ordinary stage faces need neither clipping/split scratch nor material
 * animation handling. Water material selection is shared with the general
 * emitter; only special water-bin routing needs its fallback. */
static inline bool emit_stage_fast(const EmitJob* j, int face){
    const StageTriangle* st = &j->tris[face];
    if((st->flags & SURF_ANIM_UV) ||
       (j->under_bins && (st->flags & TRI_WATER_TINT))) return false;

    int ia = emit_slot(j, st->v[0].point), ib = emit_slot(j, st->v[1].point),
        ic = emit_slot(j, st->v[2].point);
    const FramePoint *a = &j->points[ia], *b = &j->points[ib], *c = &j->points[ic];
    unsigned ca = a->clip, cb = b->clip, cc = c->clip;
    if(ca & cb & cc & CLIP_SIDES) return true;
    if((ca | cb | cc) & CLIP_GUARD) return false;
    fx max_z = max3(a->position.z, b->position.z, c->position.z);
    if(!j->far && emit_needs_subdivision(min3(a->position.z, b->position.z, c->position.z), max_z))
        return false;
    int bin = depth_bin_for_z(max_z);
    if(j->far ? bin >= j->cut_bin : bin < j->cut_bin) return true;
    uint32_t sa = j->screen[ia], sb = j->screen[ib], sc = j->screen[ic];
    if(screen_area(sa, sb, sc) <= 0 || screen_rowless(sa, sb, sc)) return true;

    unsigned texture = stage_triangle_texture(st);
    if(j->water && j->water_textures && emit_through_water(j, st)) texture = j->water_textures[texture];
    unsigned average = j->average[texture];
    unsigned alpha = st->flags & TRI_BLEND_QUARTER ? 2 : !!(st->flags & TRI_ALPHA);
    unsigned mode = alpha == 2 ? RM_BLEND_QUARTER : alpha ? RM_ALPHA : RM_TEX;
    if(!alpha && average && (j->far || bin < j->flat_bin ||
                            (j->flat_small && screen_small(sa, sb, sc)))) mode = RM_FLAT;
    const TextureReference* source = &j->textures[texture];
    if(j->cache_slots){
        unsigned slot = j->cache_slots[texture];
        if(slot) source = &g_texture_cache_reference[slot - 1];
    } else if(j->far && (g_far_stage.header->surface_features & STAGE_PACKED_FAR_TEXTURES)){
        source = stage_far_raw_texture(texture);
        if(!source && (alpha || !average)) return true;
    }
    engine_set_depth(a->position.z, b->position.z, c->position.z);
    engine_push(bin, sa, sb, sc, pack_uv(st->v[0].u, st->v[0].v),
                pack_uv(st->v[1].u, st->v[1].v), pack_uv(st->v[2].u, st->v[2].v),
                mode, average, source);
    return true;
}

void engine_emit_stage(const EmitJob* job, const uint16_t* visible, int count){
    if(!g_stage_fast_enabled){
        for(int i = 0; i < count; i++){
            SMG_COUNT(emitted_slow);
            job->slow(visible[i]);
        }
        return;
    }
    for(int i = 0; i < count; i++){
        if(emit_stage_fast(job, visible[i])) SMG_COUNT(emitted_fast);
        else {
            SMG_COUNT(emitted_slow);
            job->slow(visible[i]);
        }
    }
}

#include "mip_recip_table.h"

static int64_t raster_edge(int32_t ax, int32_t ay, int32_t bx, int32_t by, int32_t px,
                           int32_t py){
    return (int64_t)(bx - ax) * (py - ay) - (int64_t)(by - ay) * (px - ax);
}

#if defined(SMG_RASTER_WORK_STATS) && SMG_RASTER_WORK_STATS
SmgRasterWorkStats smg_raster_work_stats = {.version = 1};
#define RASTER_WORK(field) (++work.field)
#define RASTER_ZERO_TEXEL(source) (work.zero_texels += !(source))
#else
#define RASTER_WORK(field) ((void)0)
#define RASTER_ZERO_TEXEL(source) ((void)(source))
#endif

static bool raster_top_left(int32_t ax, int32_t ay, int32_t bx, int32_t by, int orientation){
    int32_t dx = orientation * (bx - ax), dy = orientation * (by - ay);
    return dy < 0 || (dy == 0 && dx > 0);
}

static uint64_t raster_abs64(int64_t value){
    return (uint64_t)(value < 0 ? -value : value);
}

/* Reproduce raster.s RECIP + MIP1_STEP for the supported positive signed32
 * screen area. The reciprocal table is rounded, and negative ASR rounds down;
 * a direct |gradient| >= 6*area test differs at mip boundaries. */
static uint32_t raster_mip_reciprocal(uint32_t den, unsigned* shift){
    int exponent = 0;
    if(den >= 256){
        if(den >= 0x01000000){ den >>= 16; exponent += 16; }
        if(den >= 0x00010000){ den >>= 8; exponent += 8; }
        if(den >= 0x00001000){ den >>= 4; exponent += 4; }
        if(den >= 0x00000400){ den >>= 2; exponent += 2; }
        if(den >= 0x00000200){ den >>= 1; exponent += 1; }
    } else {
        if(den < 16){ den <<= 4; exponent -= 4; }
        if(den < 64){ den <<= 2; exponent -= 2; }
        if(den < 128){ den <<= 1; exponent -= 1; }
        den <<= 1;
        exponent -= 1;
    }
    *shift = (unsigned)(exponent + 16);
    return engine_recip_table[den - 256];
}

static int64_t raster_asr64(int64_t value, unsigned shift){
    if(!shift) return value;
    if(value >= 0) return (int64_t)((uint64_t)value >> shift);
    uint64_t magnitude = (uint64_t)(-(value + 1)) + 1;
    uint64_t rounded = (magnitude + ((UINT64_C(1) << shift) - 1)) >> shift;
    return -(int64_t)rounded;
}

static bool raster_gba_mip_gradient(int64_t numerator, uint32_t reciprocal, unsigned shift){
    if(shift >= 32){
        numerator = raster_asr64(numerator, shift - 31);
        shift = 31;
    }
    int64_t scaled = raster_asr64(numerator * reciprocal, shift);
    return raster_abs64(scaled) >= 1536;
}

static volatile bool g_raster_setup_recip_enabled = true;

/* Setup numerators are below 2^63 (signed-16 screen/UV inputs, scaled by
 * at most 2^20). For R=floor(2^63/d), floor(n*R/2^63) underestimates n/d by
 * at most one. A remainder check recovers the exact truncating quotient.
 * Compute the 128-bit product with four 32x32 multiplies for Cortex-M7. */
__attribute__((always_inline)) static inline int64_t raster_setup_div(
    int64_t numerator, int64_t denominator, uint64_t* reciprocal, bool enabled){
    if(!numerator) return 0;
    if(!enabled) return numerator / denominator;
    uint64_t n = raster_abs64(numerator), d = raster_abs64(denominator);
    if(!*reciprocal) *reciprocal = (UINT64_C(1) << 63) / d;
    uint64_t a0 = (uint32_t)n, a1 = n >> 32;
    uint64_t b0 = (uint32_t)*reciprocal, b1 = *reciprocal >> 32;
    uint64_t p00 = a0 * b0, p01 = a0 * b1, p10 = a1 * b0, p11 = a1 * b1;
    uint64_t middle = (p00 >> 32) + (uint32_t)p01 + (uint32_t)p10;
    uint64_t high = p11 + (p01 >> 32) + (p10 >> 32) + (middle >> 32);
    uint64_t quotient = (high << 1) | ((middle >> 31) & 1);
    if(n - quotient * d >= d) quotient++;
    return (numerator < 0) != (denominator < 0) ? -(int64_t)quotient : (int64_t)quotient;
}

/* Depth setup often fits the hardware divider in unsigned magnitude. Restore
 * the sign in int64 so quotients above INT32_MAX remain exact. Preserve the
 * shared wide reciprocal for larger operands.
 * Affine UV setup retains its existing path: its scaled numerators are wide. */
__attribute__((always_inline)) static inline int64_t raster_depth_setup_div(
        int64_t numerator, int64_t denominator, uint64_t* reciprocal, bool enabled){
    if(!numerator) return 0;
    if(enabled){
        uint64_t n = raster_abs64(numerator), d = raster_abs64(denominator);
        if((n | d) <= UINT32_MAX){
            int64_t quotient = (uint32_t)n / (uint32_t)d;
            return (numerator < 0) != (denominator < 0) ? -quotient : quotient;
        }
    }
    return raster_setup_div(numerator, denominator, reciprocal, enabled);
}

/* Perspective attributes use unnormalized barycentric numerators. Keeping
 * their common screen-area factor avoids setup divisions. For a rational UV
 * over a span, chord error is bounded by |du|*|dq|/(4*min(q0,q1)). Require
 * <=1 model UV unit (1/16 source texel). The final covered endpoint is
 * exact; internal endpoints may be shared by successive spans. Use double only per span; the pixel loop remains Q16.
 * Signed-16 UVs and positive depths bound coordinates/steps to +/-65536. */
typedef struct {
    unsigned count;
    uint32_t u, v, du, dv;
#if defined(SMG_RASTER_WORK_STATS) && SMG_RASTER_WORK_STATS
    unsigned divisions;
#endif
} PerspectiveSpan;

/* The bounded production span keeps the same curvature guard and
 * 1/16-texel error bound while reducing reciprocal setup work. */
#ifndef SMG_PERSPECTIVE_SPAN_MAX
#define SMG_PERSPECTIVE_SPAN_MAX 32
#endif
#if SMG_PERSPECTIVE_SPAN_MAX != 8 && SMG_PERSPECTIVE_SPAN_MAX != 16 && SMG_PERSPECTIVE_SPAN_MAX != 32
#error "SMG_PERSPECTIVE_SPAN_MAX must be 8, 16 or 32"
#endif

/* Split whole/fractional model units so Cortex-M7 uses native F64->S32
 * conversions instead of a double-to-int64 helper call. Unsigned arithmetic
 * retains Q16 wrapping, including negative UVs and two-pixel long steps. */
static uint32_t raster_uv_q16(double value){
    int32_t whole = (int32_t)value;
    int32_t fraction = (int32_t)((value - whole) * 65536);
    return ((uint32_t)whole << 16) + (uint32_t)fraction;
}

typedef struct {
    double u, v;
    bool valid;
} PerspectiveEndpoint;
static volatile bool g_perspective_reuse_enabled = false;
/* Experimental: fewer divisions can still lose to added bound arithmetic. */
static volatile bool g_perspective_precheck_enabled;

/* Along a row, duq*q-uq*dq is invariant. The maximum U/V curvature
 * includes |dq| and the inverse error budget, leaving only positive
 * products to test at each prospective span endpoint. */
static inline double raster_perspective_curvature(double q, double uq, double vq,
        double dq, double duq, double dvq){
    double u = duq * q - uq * dq, v = dvq * q - vq * dq;
    if(u < 0) u = -u;
    if(v < 0) v = -v;
    return (u > v ? u : v) * (dq < 0 ? -dq : dq) * (1.0 / 3.996);
}

__attribute__((always_inline)) static inline PerspectiveSpan raster_perspective_span(double q, double uq, double vq,
        double dq, double duq, double dvq, unsigned remaining, PerspectiveEndpoint* next,
        const double* curvature){
    unsigned count = remaining < SMG_PERSPECTIVE_SPAN_MAX ? remaining : SMG_PERSPECTIVE_SPAN_MAX;
#if defined(SMG_RASTER_WORK_STATS) && SMG_RASTER_WORK_STATS
    unsigned divisions = 0;
#endif
    double inverse, u0, v0;
    if(next && next->valid){ u0 = next->u; v0 = next->v; }
    else {
        inverse = 1 / q; u0 = uq * inverse; v0 = vq * inverse;
#if defined(SMG_RASTER_WORK_STATS) && SMG_RASTER_WORK_STATS
        divisions++;
#endif
    }
    double u1 = u0, v1 = v0;
    unsigned distance;
    bool shared;
    const bool precheck = curvature != NULL;
    for(;;){
        /* An internal endpoint is the next span's first covered pixel, so
         * its rational UV can be reused. At the row's end use the final
         * covered pixel; extrapolating beyond the triangle could cross q=0. */
        shared = next && remaining > count;
        distance = shared ? count : count - 1;
        if(!distance) break;
        double last = distance;
        double q1 = q + dq * last;
        double change = q1 > q ? q1 - q : q - q1;
        double minimum = q1 < q ? q1 : q;
        if(precheck){
            /* |u1-u0|*|q1-q| = |duq*q-uq*dq|*|dq|*L^2/(q*q1).
             * The positive denominators can be cleared before division.
             * The row constant uses a 0.1% tighter budget for rounding. */
            double bound = minimum * q * q1;
            if(count != 1 && (shared || count != 2) &&
               *curvature * (last * last) > bound){
                count = (count + 1) / 2;
                continue;
            }
        }
        inverse = 1 / q1;
#if defined(SMG_RASTER_WORK_STATS) && SMG_RASTER_WORK_STATS
        divisions++;
#endif
        u1 = (uq + duq * last) * inverse;
        v1 = (vq + dvq * last) * inverse;
        if(precheck) break;
        double eu = (u1 > u0 ? u1 - u0 : u0 - u1) * change;
        double ev = (v1 > v0 ? v1 - v0 : v0 - v1) * change;
        if(count == 1 || (!shared && count == 2) ||
           (eu <= 4 * minimum && ev <= 4 * minimum)) break;
        count = (count + 1) / 2;
    }
    PerspectiveSpan span = {count, raster_uv_q16(u0), raster_uv_q16(v0), 0, 0};
#if defined(SMG_RASTER_WORK_STATS) && SMG_RASTER_WORK_STATS
    span.divisions = divisions;
#endif
    if(count > 1){
        static const double inverse_steps[33] = {
            0, 1, .5, 1.0/3, .25, .2, 1.0/6, 1.0/7, .125,
            1.0/9, .1, 1.0/11, 1.0/12, 1.0/13, 1.0/14, 1.0/15, .0625,
            1.0/17, 1.0/18, 1.0/19, .05, 1.0/21, 1.0/22, 1.0/23,
            1.0/24, 1.0/25, 1.0/26, 1.0/27, 1.0/28, 1.0/29, 1.0/30,
            1.0/31, 1.0/32};
        double step = inverse_steps[distance];
        span.du = raster_uv_q16((u1 - u0) * step);
        span.dv = raster_uv_q16((v1 - v0) * step);
    }
    if(next){ next->u = u1; next->v = v1; next->valid = shared; }
    return span;
}

/* Keep the common default (endpoint reuse and curvature precheck both off)
 * statically specialized. Passing literal nulls lets the always-inlined span
 * builder remove its optional endpoint and precheck branches. */
__attribute__((always_inline)) static inline PerspectiveSpan raster_perspective_span_default(
        double q, double uq, double vq, double dq, double duq, double dvq, unsigned remaining){
    return raster_perspective_span(q, uq, vq, dq, duq, dvq, remaining, NULL, NULL);
}

static bool raster_span_bound(int64_t weight, int64_t step, int* first, int* last){
    /* Solve weight + x*step >= 0 once per row instead of at every pixel.
     * Packed screen coordinates are int16; at <=320 pixels the divisors and
     * relevant distances fit uint32, using the M7 hardware divider. Keep
     * 64-bit setup so extreme offscreen triangles retain exact coverage. */
    if(step > 0 && weight < 0){
        uint64_t distance = (uint64_t)-weight;
        if(distance > (uint64_t)*last * step) return false;
        unsigned at = ((uint32_t)distance + (uint32_t)step - 1) / (uint32_t)step;
        if((int)at > *first) *first = at;
    } else if(step < 0){
        if(weight < 0) return false;
        uint32_t divisor = (uint32_t)-step;
        if((uint64_t)weight < (uint64_t)*last * divisor){
            unsigned at = (uint32_t)weight / divisor;
            if((int)at < *last) *last = at;
        }
    } else if(step == 0 && weight < 0) return false;
    return *first <= *last;
}

static volatile bool g_raster_convert_narrow_enabled = true;
static volatile bool g_raster_coverage_narrow_enabled = true;

/* Packed int16 coordinates bound an edge's per-pixel step to +/-1048560.
 * The clipped row is at most 320 pixels, so relevant distances and the ceil
 * numerator fit uint32. The caller separately proves signed32 row weights. */
static bool raster_span_bound32(int32_t weight, int32_t step, int* first, int* last){
    if(step > 0 && weight < 0){
        uint32_t distance = 0u - (uint32_t)weight;
        if(distance > (unsigned)*last * (uint32_t)step) return false;
        unsigned at = (distance + (uint32_t)step - 1) / (uint32_t)step;
        if((int)at > *first) *first = at;
    } else if(step < 0){
        if(weight < 0) return false;
        uint32_t divisor = 0u - (uint32_t)step;
        if((uint32_t)weight < (unsigned)*last * divisor){
            unsigned at = (uint32_t)weight / divisor;
            if((int)at < *last) *last = at;
        }
    } else if(step == 0 && weight < 0) return false;
    return *first <= *last;
}

static bool raster_coverage_fits_s32(int64_t area, int64_t w1, int64_t w2,
        int64_t dy1, int64_t dy2, unsigned rows, int bias0, int bias1, int bias2){
    int64_t weights[3] = {w1 - bias1, w2 - bias2, area - w1 - w2 - bias0};
    int64_t steps[3] = {dy1, dy2, -dy1 - dy2};
    for(unsigned i = 0; i < 3; i++){
        int64_t end = weights[i] + steps[i] * rows;
        if(weights[i] < INT32_MIN || weights[i] > INT32_MAX ||
           end < INT32_MIN || end > INT32_MAX) return false;
    }
    return true;
}

/* Negative/wrapped and sub-unit values map to one. Positive signed values
 * shifted by 15 cannot exceed 65535, so no upper saturation is needed. */
__attribute__((always_inline)) static inline uint32_t raster_depth_quantize(uint32_t raw){
    return (int32_t)raw >= 32768 ? raw >> 15 : 1;
}

/* Every signed32 integer is exactly representable as double. Check the
 * actual area/initial edge weights, retaining wide conversion for extreme
 * offscreen coordinates. No geometric or interpolation approximation. */
__attribute__((always_inline)) static inline bool raster_coords_fit_s32(
        int64_t area, int64_t w1, int64_t w2){
    return area >= INT32_MIN && area <= INT32_MAX &&
           w1 >= INT32_MIN && w1 <= INT32_MAX &&
           w2 >= INT32_MIN && w2 <= INT32_MAX;
}

/* V selects rows, U selects columns. Mip1 has 16 rows at stride64;
 * mip0 has 32 rows at stride32. The address fields never overlap. */
__attribute__((always_inline)) static inline unsigned raster_texture_offset(
        uint32_t u, uint32_t v, unsigned row_mask){
    _Static_assert(TEX_DIM == 32 && TEX_MIP1_STRIDE == 64, "Texture address layout");
    return ((v >> 15) & row_mask) | ((u >> 20) & 31);
}

/* Constant specializations remove unused material branches from ordinary
 * opaque spans and FPU setup from affine rendering. All paths share the same
 * coverage, interpolation, depth and texel-addressing source.
 * depth_policy is -1 for dynamic, 0 for off, 1 for on. Forced-on callers
 * must establish a valid g_raster_depth before entering the shader. */
__attribute__((always_inline)) static inline void engine_raster_inner_impl(
        const DrawEntry* entry, const bool perspective, const bool opaque, const int depth_policy){
    /* Pixel writes may alias any C object: snapshot immutable material fields
     * so the compiler need not reload them after every framebuffer store. */
    const bool reuse_endpoints = perspective && g_perspective_reuse_enabled;
    const bool precheck = perspective && g_perspective_precheck_enabled;
    const unsigned mode = opaque ? RM_TEX : entry->mode;
    const uint8_t colour = entry->colour;
    int32_t x[3], y[3], u[3], v[3];
    for(unsigned i = 0; i < 3; i++){
        x[i] = screen_x(entry->s[i]);
        y[i] = screen_y(entry->s[i]);
        u[i] = (int16_t)entry->uv[i];
        v[i] = (int16_t)(entry->uv[i] >> 16);
    }

    int64_t signed_area = raster_edge(x[0], y[0], x[1], y[1], x[2], y[2]);
    if(!signed_area) return;
    bool reverse = signed_area < 0;
    int64_t area = reverse ? -signed_area : signed_area;

    int32_t min_x = x[0], max_x = x[0], min_y = y[0], max_y = y[0];
    for(unsigned i = 1; i < 3; i++){
        if(x[i] < min_x) min_x = x[i];
        if(x[i] > max_x) max_x = x[i];
        if(y[i] < min_y) min_y = y[i];
        if(y[i] > max_y) max_y = y[i];
    }
    int first_x = (min_x >> 4) - 1;
    int last_x = (max_x >> 4) + 1;
    int first_y = (min_y >> 4) - 1;
    int last_y = (max_y >> 4) + 1;
    if(first_x < 0) first_x = 0;
    if(first_y < 0) first_y = 0;
    if(last_x >= FRAME_WIDTH) last_x = FRAME_WIDTH - 1;
    if(last_y >= FRAME_HEIGHT) last_y = FRAME_HEIGHT - 1;
    if(first_x > last_x || first_y > last_y) return;

    int64_t du_dx_num = (int64_t)(u[1] - u[0]) * (y[2] - y[0]) -
                        (int64_t)(u[2] - u[0]) * (y[1] - y[0]);
    int64_t du_dy_num = (int64_t)(x[1] - x[0]) * (u[2] - u[0]) -
                        (int64_t)(x[2] - x[0]) * (u[1] - u[0]);
    int64_t dv_dx_num = (int64_t)(v[1] - v[0]) * (y[2] - y[0]) -
                        (int64_t)(v[2] - v[0]) * (y[1] - y[0]);
    int64_t dv_dy_num = (int64_t)(x[1] - x[0]) * (v[2] - v[0]) -
                        (int64_t)(x[2] - x[0]) * (v[1] - v[0]);
    bool textured = mode != RM_FLAT && mode != RM_TINT;
    if(textured && !entry->texture) return;
    const bool affine = textured && !perspective;
    const bool setup_recip = g_raster_setup_recip_enabled;
    uint64_t reciprocal = 0;
    int64_t du_dx = affine ? raster_setup_div(du_dx_num * 16 * 65536, signed_area, &reciprocal, setup_recip) : 0;
    int64_t du_dy = affine ? raster_setup_div(du_dy_num * 16 * 65536, signed_area, &reciprocal, setup_recip) : 0;
    int64_t dv_dx = affine ? raster_setup_div(dv_dx_num * 16 * 65536, signed_area, &reciprocal, setup_recip) : 0;
    int64_t dv_dy = affine ? raster_setup_div(dv_dy_num * 16 * 65536, signed_area, &reciprocal, setup_recip) : 0;
    bool mip1 = false;
    if(entry->texture && entry->texture->mip[1] != entry->texture->mip[0]){
        if(area <= INT32_MAX){
            unsigned shift;
            uint32_t reciprocal = raster_mip_reciprocal((uint32_t)area, &shift);
            /* raster.s scales U-x and V-x gradients (r1 and r5) only. */
            mip1 = raster_gba_mip_gradient(du_dx_num, reciprocal, shift) ||
                   raster_gba_mip_gradient(dv_dx_num, reciprocal, shift);
        } else {
            /* The original ARM area arithmetic is signed32. Keep the C
             * renderer's conservative wide-area behavior outside that range. */
            mip1 = raster_abs64(du_dx_num) >= (uint64_t)area * 6 ||
                   raster_abs64(du_dy_num) >= (uint64_t)area * 6 ||
                   raster_abs64(dv_dx_num) >= (uint64_t)area * 6 ||
                   raster_abs64(dv_dy_num) >= (uint64_t)area * 6;
        }
    }

    double pq[3] = {0}, pu[3] = {0}, pv[3] = {0};
    if(perspective) for(unsigned i = 0; i < 3; i++){
        pq[i] = g_raster_depth[i];
        pu[i] = pq[i] * u[i]; pv[i] = pq[i] * v[i];
    }
    int orientation = reverse ? -1 : 1;
    /* Blended shared edges belong to exactly one triangle. Bias only the
     * coverage bounds, leaving interpolation and other material modes intact. */
    const bool blend = mode == RM_BLEND_QUARTER;
    int edge0_bias = blend && !raster_top_left(x[1], y[1], x[2], y[2], orientation);
    int edge1_bias = blend && !raster_top_left(x[2], y[2], x[0], y[0], orientation);
    int edge2_bias = blend && !raster_top_left(x[0], y[0], x[1], y[1], orientation);
    int64_t edge1_x = orientation * (int64_t)(y[2] - y[0]) * 16;
    int64_t edge1_y = orientation * -(int64_t)(x[2] - x[0]) * 16;
    int64_t edge2_x = orientation * -(int64_t)(y[1] - y[0]) * 16;
    int64_t edge2_y = orientation * (int64_t)(x[1] - x[0]) * 16;
    int32_t start_sx = first_x * 16 + 8;
    int32_t start_sy = first_y * 16 + 8;
    int64_t w1_start = orientation * -raster_edge(x[0], y[0], x[2], y[2], start_sx, start_sy);
    int64_t w2_start = orientation * raster_edge(x[0], y[0], x[1], y[1], start_sx, start_sy);
    int64_t u_start = perspective ? 0 : (int64_t)u[0] * 65536 +
                      (du_dx * (start_sx - x[0]) + du_dy * (start_sy - y[0])) / 16;
    int64_t v_start = perspective ? 0 : (int64_t)v[0] * 65536 +
                      (dv_dx * (start_sx - x[0]) + dv_dy * (start_sy - y[0])) / 16;

    /* Reciprocal depth is affine in screen space. Vertices retain seven
     * fractional bits beyond the 16-bit buffer; eight more setup bits keep
     * per-pixel increments accurate without a division in the pixel loop. */
    const bool depth_test = depth_policy < 0 ? g_depth_active && g_raster_depth : depth_policy != 0;
    int32_t depth_dx = 0, depth_dy = 0;
    uint32_t depth_start = 0;
    if(depth_test){
        int64_t d0 = g_raster_depth[0];
        int64_t d1 = (int64_t)g_raster_depth[1] - d0;
        int64_t d2 = (int64_t)g_raster_depth[2] - d0;
        int64_t dx = raster_depth_setup_div((d1 * (y[2] - y[0]) - d2 * (y[1] - y[0])) * 4096,
                                      signed_area, &reciprocal, setup_recip);
        int64_t dy = raster_depth_setup_div(((x[1] - x[0]) * d2 - (x[2] - x[0]) * d1) * 4096,
                                      signed_area, &reciprocal, setup_recip);
        depth_dx = (int32_t)dx;
        depth_dy = (int32_t)dy;
        depth_start = (uint32_t)(d0 * 256 +
            (dx * (start_sx - x[0]) + dy * (start_sy - y[0])) / 16);
    }

    extern uint8_t g_shade_water[256];
    const unsigned texture_row_mask = mip1 ? 0x3c0u : 0x3e0u;
    const uint8_t* texture = NULL;
    if(entry->texture)
        texture = (const uint8_t*)(uintptr_t)entry->texture->mip[mip1 ? 1 : 0];
    uint8_t* framebuffer = g_engine_fb;
#if defined(SMG_RASTER_WORK_STATS) && SMG_RASTER_WORK_STATS
    SmgRasterModeWork work = {.triangles = 1};
#endif
    SMG_COUNT(raster_triangles);
    if(!textured) SMG_COUNT(flat_triangles);
    else if(mip1) SMG_COUNT(mip1_triangles);
    else SMG_COUNT(mip0_triangles);
    int64_t w1_row = w1_start, w2_row = w2_start;
    uint32_t u_row = (uint32_t)u_start, v_row = (uint32_t)v_start;
    uint32_t depth_row = depth_start;
    double perspective_row[3] = {0}, perspective_dx[3] = {0}, perspective_dy[3] = {0};
    if(perspective){
        double a_area, a_w1, a_w2;
        if(g_raster_convert_narrow_enabled && raster_coords_fit_s32(area, w1_start, w2_start)){
            a_area = (double)(int32_t)area;
            a_w1 = (double)(int32_t)w1_start;
            a_w2 = (double)(int32_t)w2_start;
        } else {
            a_area = (double)area;
            a_w1 = (double)w1_start;
            a_w2 = (double)w2_start;
        }
        const double* attributes[3] = {pq, pu, pv};
        for(unsigned i = 0; i < 3; i++){
            const double* a = attributes[i];
            double d1 = a[1] - a[0], d2 = a[2] - a[0];
            perspective_row[i] = a_area * a[0] + a_w1 * d1 + a_w2 * d2;
            perspective_dx[i] = edge1_x * d1 + edge2_x * d2;
            perspective_dy[i] = edge1_y * d1 + edge2_y * d2;
        }
    }
    const bool narrow_coverage = g_raster_coverage_narrow_enabled &&
        raster_coverage_fits_s32(area, w1_start, w2_start, edge1_y, edge2_y,
                                (unsigned)(last_y - first_y), edge0_bias, edge1_bias, edge2_bias);
    /* The guard proves every used row is signed32. Unsigned accumulators also
     * make the unused increment after the final row well-defined at limits. */
    uint32_t nw1 = (uint32_t)w1_start - (unsigned)edge1_bias;
    uint32_t nw2 = (uint32_t)w2_start - (unsigned)edge2_bias;
    uint32_t nw0 = (uint32_t)(area - w1_start - w2_start) - (unsigned)edge0_bias;
    for(int py = first_y; py <= last_y; py++){
        int left = 0, right = last_x - first_x;
        bool visible;
        if(narrow_coverage){
            visible = raster_span_bound32((int32_t)nw1, (int32_t)edge1_x, &left, &right) &&
                      raster_span_bound32((int32_t)nw2, (int32_t)edge2_x, &left, &right) &&
                      raster_span_bound32((int32_t)nw0,
                                          (int32_t)(-edge1_x - edge2_x), &left, &right);
        } else visible = raster_span_bound(w1_row - edge1_bias, edge1_x, &left, &right) &&
                       raster_span_bound(w2_row - edge2_bias, edge2_x, &left, &right) &&
                       raster_span_bound(area - w1_row - w2_row - edge0_bias, -edge1_x - edge2_x, &left, &right);
#if defined(SMG_RASTER_WORK_STATS) && SMG_RASTER_WORK_STATS
        if(visible) work.covered_pixels += (unsigned)(right - left + 1);
#endif
        uint32_t uw = u_row + (uint32_t)du_dx * (unsigned)left;
        uint32_t vw = v_row + (uint32_t)dv_dx * (unsigned)left;
        uint32_t depth_value = depth_row + (uint32_t)depth_dx * (unsigned)left;
        double q = 0, uq = 0, vq = 0, dq = 0, duq = 0, dvq = 0;
        PerspectiveSpan span = {0};
        PerspectiveEndpoint endpoint = {0};
        double curvature = 0;
        if(visible && perspective){
            dq = perspective_dx[0]; duq = perspective_dx[1]; dvq = perspective_dx[2];
            q = perspective_row[0] + dq * left;
            uq = perspective_row[1] + duq * left;
            vq = perspective_row[2] + dvq * left;
            if(precheck) curvature = raster_perspective_curvature(q, uq, vq, dq, duq, dvq);
        }
#define RASTER_PIXEL_BODY() do { \
            uint8_t* pixel = framebuffer + py * FRAME_STRIDE + px; \
            uint16_t* depth_pixel = NULL; \
            uint32_t depth = 0; \
            bool depth_pass = true; \
            if(depth_test){ \
                depth_pixel = &g_depth_buffer[py * FRAME_WIDTH + px]; \
                /* Equivalent forms produce different GCC schedules. Keep the \
                 * cheaper tiny-face affine form; perspective specializes away \
                 * its second per-pixel clamp. */ \
                if(perspective) depth = raster_depth_quantize(depth_value); \
                else { \
                    depth = (int32_t)depth_value > 0 ? depth_value >> 15 : 1; \
                    if(!depth) depth = 1; \
                    if(depth > 65535) depth = 65535; \
                } \
                unsigned bias = mode == RM_DARKEN ? 2 : 0; \
                if(depth + bias < *depth_pixel){ \
                    RASTER_WORK(depth_rejected_pixels); \
                    depth_pass = false; \
                } \
            } \
            if(depth_pass){ \
                uint8_t source = 0; \
                if(textured){ \
                    /* Assets and the GBA TEXEL macro store V rows, U columns. */ \
                    source = texture[raster_texture_offset(uw, vw, texture_row_mask)]; \
                    RASTER_WORK(texture_samples); \
                    RASTER_ZERO_TEXEL(source); \
                } \
                switch(mode){ \
                case RM_FLAT: \
                case RM_TEX: \
                    *pixel = mode == RM_FLAT ? colour : source; \
                    RASTER_WORK(colour_writes); \
                    if(depth_pixel){ *depth_pixel = (uint16_t)depth; RASTER_WORK(depth_writes); } \
                    break; \
                case RM_ALPHA: \
                    if(source){ \
                        *pixel = source; RASTER_WORK(colour_writes); \
                        if(depth_pixel){ *depth_pixel = (uint16_t)depth; RASTER_WORK(depth_writes); } \
                    } \
                    break; \
                case RM_ADD: \
                    if(source && g_additive_rows && g_additive_table){ \
                        uint32_t index = (uint32_t)*pixel + ((uint32_t)g_additive_rows[source] << 8); \
                        *pixel = g_additive_table[index]; RASTER_WORK(colour_writes); \
                    } \
                    break; \
                case RM_DARKEN: \
                    if(source){ *pixel = shade_darken[*pixel]; RASTER_WORK(colour_writes); } \
                    break; \
                case RM_TINT: \
                    *pixel = g_shade_water[*pixel]; RASTER_WORK(colour_writes); \
                    break; \
                case RM_BLEND_QUARTER: \
                    if(source){ *pixel = shade_quarter[((uint32_t)*pixel << 8) | source]; RASTER_WORK(colour_writes); } \
                    break; \
                default: break; \
                } \
            } \
        } while(0)
        if(visible && perspective){
            int px = first_x + left;
            const int end_x = first_x + right;
            while(px <= end_x){
                unsigned remaining = (unsigned)(end_x - px + 1);
                if(!reuse_endpoints && !precheck)
                    span = raster_perspective_span_default(q, uq, vq, dq, duq, dvq, remaining);
                else
                    span = raster_perspective_span(q, uq, vq, dq, duq, dvq, remaining,
                        reuse_endpoints ? &endpoint : NULL, precheck ? &curvature : NULL);
#if defined(SMG_RASTER_WORK_STATS) && SMG_RASTER_WORK_STATS
                work.perspective_spans++;
                work.perspective_pixels += span.count;
                work.perspective_divisions += span.divisions;
#endif
                q += dq * span.count; uq += duq * span.count; vq += dvq * span.count;
                uw = span.u; vw = span.v;
                for(unsigned i = 0; i < span.count; i++, px++){
                    RASTER_PIXEL_BODY();
                    uw += span.du; vw += span.dv;
                    depth_value += (uint32_t)depth_dx;
                }
            }
        } else if(visible){
            for(int px = first_x + left; px <= first_x + right; px++){
                RASTER_PIXEL_BODY();
                uw += (uint32_t)du_dx; vw += (uint32_t)dv_dx;
                depth_value += (uint32_t)depth_dx;
            }
        }
#undef RASTER_PIXEL_BODY
#undef RASTER_ZERO_TEXEL
        if(narrow_coverage){
            nw1 += (uint32_t)edge1_y;
            nw2 += (uint32_t)edge2_y;
            nw0 -= (uint32_t)(edge1_y + edge2_y);
        } else {
            w1_row += edge1_y;
            w2_row += edge2_y;
        }
        u_row += (uint32_t)du_dy;
        v_row += (uint32_t)dv_dy;
        depth_row += (uint32_t)depth_dy;
        if(perspective) for(unsigned i = 0; i < 3; i++) perspective_row[i] += perspective_dy[i];
    }
#if defined(SMG_RASTER_WORK_STATS) && SMG_RASTER_WORK_STATS
    if(mode < 7){
        SmgRasterModeWork* totals = &smg_raster_work_stats.mode[mode];
        totals->triangles += work.triangles;
        totals->covered_pixels += work.covered_pixels;
        totals->depth_rejected_pixels += work.depth_rejected_pixels;
        totals->texture_samples += work.texture_samples;
        totals->zero_texels += work.zero_texels;
        totals->colour_writes += work.colour_writes;
        totals->depth_writes += work.depth_writes;
        totals->perspective_spans += work.perspective_spans;
        totals->perspective_pixels += work.perspective_pixels;
        totals->perspective_divisions += work.perspective_divisions;
    }
#endif
}


__attribute__((noinline)) static void engine_raster_inner_affine(const DrawEntry* entry){
    engine_raster_inner_impl(entry, false, false, -1);
}

__attribute__((noinline)) static void engine_raster_inner_perspective(const DrawEntry* entry){
    engine_raster_inner_impl(entry, true, false, -1);
}

__attribute__((noinline)) static void engine_raster_inner_affine_opaque(const DrawEntry* entry){
    engine_raster_inner_impl(entry, false, true, -1);
}

__attribute__((noinline)) static void engine_raster_inner_perspective_opaque(const DrawEntry* entry){
    engine_raster_inner_impl(entry, true, true, 0);
}

__attribute__((noinline)) static void engine_raster_inner_perspective_opaque_depth(const DrawEntry* entry){
    engine_raster_inner_impl(entry, true, true, 1);
}

/* Retain a same-build general-path control for exact A/B measurements. */
static volatile bool g_opaque_raster_enabled = true;

__attribute__((noinline)) static void engine_raster_inner(const DrawEntry* entry){
    /* Depth collection is independent of the optional depth test. Flat and
     * blending passes retain their original sampling; correct world/cutouts. */
    bool perspective = g_perspective_active && g_raster_depth &&
        (entry->mode == RM_TEX || entry->mode == RM_ALPHA || entry->mode == RM_BLEND_QUARTER) &&
        g_raster_depth[0] && g_raster_depth[1] && g_raster_depth[2];
    if(entry->mode == RM_TEX && g_opaque_raster_enabled){
        /* Perspective eligibility already proves the depth pointer and all
         * three values valid. Snapshot the depth mode once per triangle. */
        if(perspective && g_depth_active) engine_raster_inner_perspective_opaque_depth(entry);
        else if(perspective) engine_raster_inner_perspective_opaque(entry);
        else engine_raster_inner_affine_opaque(entry);
    }
    else if(perspective) engine_raster_inner_perspective(entry);
    else engine_raster_inner_affine(entry);
}

void engine_raster(const DrawEntry* entry){
    /* Cutouts write depth only for solid texels. Blended overlays must wait
     * until opaque world/actors have established the final visible surface. */
    if(g_depth_active && !g_depth_flushing && entry->mode >= RM_ADD){
        if(g_depth_effect_count >= DEPTH_EFFECT_MAX){
            SMG_COUNT(depth_effect_dropped);
            return;
        }
        DepthEffect* effect = &g_depth_effects[g_depth_effect_count++];
        effect->entry = *entry;
        memcpy(effect->depth, g_raster_depth, sizeof effect->depth);
        if(entry->texture){
            effect->texture = *entry->texture;
            effect->entry.texture = &effect->texture;
        }
        effect->additive_rows = g_additive_rows;
        effect->additive_table = g_additive_table;
        SMG_PEAK(depth_effect_peak, g_depth_effect_count);
        return;
    }
    uint32_t started = smg_perf_now();
    engine_raster_inner(entry);
    smg_perf_add(SMG_PERF_RASTER, started);
}

void engine_draw_chain(unsigned head){
    while(head != BIN_END){
        const DrawEntry* entry = &g_arena[head];
        g_raster_depth = g_arena_depth[head];
        engine_raster(entry);
        head = entry->next;
    }
}

static void engine_depth_end(void){
    const uint16_t* rows = g_additive_rows;
    const uint8_t* table = g_additive_table;
    g_depth_flushing = true;
    for(unsigned i = 0; i < g_depth_effect_count; i++){
        DepthEffect* effect = &g_depth_effects[i];
        g_raster_depth = effect->depth;
        g_additive_rows = effect->additive_rows;
        g_additive_table = effect->additive_table;
        engine_raster(&effect->entry);
    }
    g_additive_rows = rows;
    g_additive_table = table;
    g_depth_active = false;
    g_perspective_active = false;
    g_raster_depth = NULL;
}
#endif

static const FigureTriangle* g_figure_draw_triangles;
static const TextureReference* g_figure_draw_textures;
EWRAM_BSS const uint16_t* g_additive_rows;
EWRAM_BSS const uint8_t* g_additive_table;

static void engine_figure_clipped(const FramePoint* points, int face){
    const FigureTriangle* tri = &g_figure_draw_triangles[face];
    int ia = tri->v[0].point, ib = tri->v[2].point, ic = tri->v[1].point;
    bool alpha = tri->flags & TRI_ALPHA;
    ClipVertex t[3] = {{points[ia].position, tri->v[0].u, tri->v[0].v},
                       {points[ib].position, tri->v[2].u, tri->v[2].v},
                       {points[ic].position, tri->v[1].u, tri->v[1].v}};
    engine_emit_clipped(-1, t,
                        alpha             ? RM_ALPHA
                        : g_additive_rows ? RM_ADD
                                          : RM_TEX,
                        0, &g_figure_draw_textures[tri->texture]);
}

#if defined(__GNW__)
void engine_draw_figure_triangles(const FigureRasterJob* job){
    for(int i = 0; i < job->count; i++){
        int face = job->order[i * (job->step / 2)];
        const FigureTriangle* tri = &job->tris[face];
        int ia = tri->v[0].point, ib = tri->v[2].point, ic = tri->v[1].point;
        if(!((job->points[ia].clip | job->points[ib].clip | job->points[ic].clip) & CLIP_GUARD)){
            /* No clipping is necessary: these exact projections were already
             * computed once per vertex by engine_transform_points. Match the
             * clipped path's winding, row rejection, UV order and alpha mode. */
            uint32_t a = job->screen[ia], b = job->screen[ib], c = job->screen[ic];
            if(screen_area(a, b, c) <= 0 || screen_rowless(a, b, c)) continue;
            unsigned mode = tri->flags & TRI_ALPHA ? RM_ALPHA : job->textured;
            engine_set_depth(job->points[ia].position.z, job->points[ib].position.z, job->points[ic].position.z);
            engine_out(-1, a, b, c,
                       pack_uv(tri->v[0].u, tri->v[0].v),
                       pack_uv(tri->v[2].u, tri->v[2].v),
                       pack_uv(tri->v[1].u, tri->v[1].v),
                       mode, 0, &job->textures[tri->texture]);
            continue;
        }
        job->clipped(face);
    }
}
#endif

static const TextureReference g_shadow_texture = {
    {(uint32_t)shadow_texture.mip[0], (uint32_t)shadow_texture.mip[0]}};
__attribute__((noinline)) static void draw_shadow(const Figure* figure, const CameraView* view){
    const fx radius = FXC(0.2f);
    v3 up = figure == &g_game.player.figure ? g_game.player.up : V3(0, 0, N14_ONE);
    v3 t1 = v3_tangent14(view->view.r[0].x || view->view.r[0].y || view->view.r[0].z
                             ? V3(view->view.r[0].x << 6, view->view.r[0].y << 6, view->view.r[0].z << 6)
                             : V3(N14_ONE, 0, 0),
                         up, V3(N14_ONE, 0, 0));
    v3 t2 = v3_cross14(up, t1);
    v3 g = v3_add(figure->shadow, v3_scale14(up, 2));
    v3 e1 = v3_scale14(t1, radius), e2 = v3_scale14(t2, radius);
    v3 corner[4] = {v3_sub(v3_sub(g, e1), e2), v3_sub(v3_add(g, e1), e2), v3_add(v3_add(g, e1), e2),
                    v3_add(v3_sub(g, e1), e2)};
    static const int16_t uvs[4][2] = {{0, 0}, {512, 0}, {512, 512}, {0, 512}};
    ClipVertex q[4];
    uint32_t clip_and = CLIP_SIDES | CLIP_GUARD, clip_or = 0;
    for(int i = 0; i < 4; i++){
        q[i].p = xform_point(&view->to_screen, corner[i]);
        q[i].u = uvs[i][0];
        q[i].v = uvs[i][1];
        uint32_t c = view_clip_mask(q[i].p);
        clip_and &= c;
        clip_or |= c;
    }
    if(clip_and & CLIP_SIDES) return;
    for(int half = 0; half < 2; half++){
        ClipVertex t[3] = {q[0], q[1 + half], q[2 + half]};

        if(!(clip_or & CLIP_GUARD)){
            uint32_t s0 = screen_pack_position(project_to_screen(t[0].p));
            uint32_t s1 = screen_pack_position(project_to_screen(t[1].p));
            uint32_t s2 = screen_pack_position(project_to_screen(t[2].p));
            if(screen_area(s0, s1, s2) < 0){
                ClipVertex x = t[1];
                t[1] = t[2];
                t[2] = x;
            }
        } else {
            v3 n = v3_cross14(v3_sub(t[1].p, t[0].p), v3_sub(t[2].p, t[0].p));
            if(v3_dot14(n, t[0].p) > 0){
                ClipVertex x = t[1];
                t[1] = t[2];
                t[2] = x;
            }
        }
        engine_emit_clipped(-1, t, RM_DARKEN, 0, &g_shadow_texture);
    }
}
