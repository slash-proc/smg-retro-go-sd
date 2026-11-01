static const int16_t (*model_rest_pose(const FigureHeader* f))[3] {
    return (const int16_t (*)[3])((const uint8_t*)f + f->points);
}

static const void* model_keyframe(const FigureHeader* f, int k){
    if(f->keyframe_count & FIGURE_PACKED_KEYS){
        const uint32_t* offsets = (const uint32_t*)((const uint8_t*)f + f->keyframes);

        return (const uint8_t*)f + offsets[k];
    }
    return (const int16_t (*)[3])((const uint8_t*)f + f->keyframes) + k * f->point_count;
}

#define POSE_INPUT_BYTES (160 * 3 * 2)
#if defined(__GNW__)
#define GNW_POSE_SLOTS 6
static EWRAM_BSS int16_t g_pose_scratch[GNW_POSE_SLOTS][FIGURE_PACKED_MAX_POINTS * 3];
static EWRAM_BSS uintptr_t g_pose_cache[GNW_POSE_SLOTS];
static void model_invalidate_pose_cache(void){
    for(unsigned i = 0; i < GNW_POSE_SLOTS; i++) g_pose_cache[i] = 0;
}
static int16_t* model_pose_scratch(unsigned slot){
    return g_pose_scratch[slot < GNW_POSE_SLOTS ? slot : GNW_POSE_SLOTS - 1];
}

static const int16_t* model_decode_pose(const uint8_t* descriptor, unsigned slot){
    return pose_decode(descriptor, model_pose_scratch(slot));
}

static const int16_t (*model_resolve_packed(uintptr_t address, unsigned slot))[3]{
    const uint8_t* descriptor = (const uint8_t*)(address & ~(uintptr_t)FIGURE_POSE_TAG);
    const PackedPoseHeader* h = (const PackedPoseHeader*)(const void*)descriptor;
    if(h->mode == FIGURE_POSE_RAW)
        return (const int16_t (*)[3])(descriptor + sizeof(*h));
    if(slot >= GNW_POSE_SLOTS) slot = GNW_POSE_SLOTS - 1;
    /* Slots are live interpolation inputs, not interchangeable cache lines.
     * Returning old B's slot for new A lets decoding new B overwrite A. */
    if(g_pose_cache[slot] == address)
        return (const int16_t (*)[3])g_pose_scratch[slot];
    const int16_t* decoded = model_decode_pose(descriptor, slot);
    g_pose_cache[slot] = address;
    return (const int16_t (*)[3])decoded;
}
#else
static int16_t* model_pose_scratch(unsigned slot){
    if(slot == 4) return (int16_t*)(0x06009300u + 2 * POSE_INPUT_BYTES);
    if(slot == 5) return (int16_t*)0x06013d00u;
    uintptr_t base = slot < 2 ? 0x06009300u : 0x06013300u;
    return (int16_t*)(base + (slot & 1u) * POSE_INPUT_BYTES);
}
_Static_assert(2 * POSE_INPUT_BYTES + FIGURE_PACKED_MAX_POINTS * 6 + 64 <= POSE_TAIL_BYTES,
               "pose buffers and cache tags must fit the first VRAM tail");
#define POSE_KEY_CACHE ((volatile uint32_t*)0x06009fc0u)
static void model_invalidate_pose_cache(void){
    for(unsigned i = 0; i < 4; i++) POSE_KEY_CACHE[i] = 0;
}

#define POSE_CODE_VRAM 0x06013a80u
static bool g_pose_code_ready;
typedef const int16_t* (*PoseDecoderFn)(const uint8_t*, int16_t*);

static const int16_t* model_decode_pose(const uint8_t* descriptor, unsigned slot){
    if(!g_pose_code_ready){
        unsigned words = (unsigned)(__pose_code_end - __pose_code_start);

        for(unsigned i = 0; i < words; i += 64){
            unsigned n = words - i;
            if(n > 64) n = 64;
            dma3_copy32((uint32_t*)POSE_CODE_VRAM + i, __pose_code_start + i, n);
        }
        model_invalidate_pose_cache();
        for(unsigned i = 4; i < 8; i++) POSE_KEY_CACHE[i] = 0;
        g_pose_code_ready = true;
    }
    return ((PoseDecoderFn)POSE_CODE_VRAM)(descriptor, model_pose_scratch(slot));
}

__attribute__((noinline)) static const int16_t (*model_resolve_packed(uintptr_t address, unsigned slot))[3] {
    const uint8_t* descriptor = (const uint8_t*)(address & ~(uintptr_t)FIGURE_POSE_TAG);
    const PackedPoseHeader* h = (const PackedPoseHeader*)(const void*)descriptor;
    if(h->mode == FIGURE_POSE_RAW) return (const int16_t (*)[3])(descriptor + sizeof(*h));
    if(g_pose_code_ready && POSE_KEY_CACHE[slot] == address)
        return (const int16_t (*)[3])model_pose_scratch(slot);
    if(slot == 4 && g_pose_code_ready){
        if(POSE_KEY_CACHE[5] == address) return (const int16_t (*)[3])model_pose_scratch(5);
        if(h->point_count <= 128){
            if(POSE_KEY_CACHE[6] > 128 || !POSE_KEY_CACHE[5]) slot = 5;
            else if(POSE_KEY_CACHE[4]){
                POSE_KEY_CACHE[7] ^= 1;
                slot = 4 + POSE_KEY_CACHE[7];
            }
        }
    }
    if(slot < 4 && g_pose_code_ready){
        for(unsigned other = 0; other < 4; other++)
            if(POSE_KEY_CACHE[other] == address){
                int16_t* dst = model_pose_scratch(slot);
                const int16_t* src = model_pose_scratch(other);
                for(unsigned i = 0; i < h->point_count * 3u; i++) dst[i] = src[i];
                POSE_KEY_CACHE[slot] = (uint32_t)address;
                return (const int16_t (*)[3])dst;
            }
    }
    const int16_t* decoded = model_decode_pose(descriptor, slot);
    POSE_KEY_CACHE[slot] = (uint32_t)address;
    if(slot == 4) POSE_KEY_CACHE[6] = h->point_count;
    return (const int16_t (*)[3])decoded;
}
#endif

static inline const int16_t (*model_resolve_pose(const void* pose, unsigned slot))[3] {
    uintptr_t address = (uintptr_t)pose;
    return address & FIGURE_POSE_TAG ? model_resolve_packed(address, slot) : (const int16_t (*)[3])pose;
}

static const FigureTriangle* model_triangles(const FigureHeader* f){
    return (const FigureTriangle*)((const uint8_t*)f + f->triangles);
}

static const uint16_t* model_draw_order(const FigureHeader* f, int k){
    return (const uint16_t*)((const uint8_t*)f + f->axes) + (k >> 1) * f->triangle_count;
}

static const TextureReference* model_textures(const FigureHeader* f){
    return (const TextureReference*)((const uint8_t*)f + f->textures);
}

static const FigureTriangle* model_lod_triangles(const FigureHeader* f){
    return (const FigureTriangle*)((const uint8_t*)f + f->lod_triangles);
}

static const uint16_t* model_lod_draw_order(const FigureHeader* f, int k){
    return (const uint16_t*)((const uint8_t*)f + f->lod_axes) + (k >> 1) * f->lod_triangle_count;
}

static const uint16_t* model_lod_points(const FigureHeader* f){
    return (const uint16_t*)((const uint8_t*)f + f->lod_points);
}

static const TextureReference* model_lod_textures(const FigureHeader* f){
    return (const TextureReference*)((const uint8_t*)f + f->lod_textures);
}

static void figure_init(Figure* figure, const uint8_t* model){
    *figure = (Figure){0};
    figure->model = (const FigureHeader*)model;
    figure->pose = model_rest_pose(figure->model);
    figure->scale = V3(FX_ONE, FX_ONE, FX_ONE);
    figure->up = V3(0, 0, FX_ONE);
    figure->transform_dirty = true;
}

static m3 m3_align_up(v3 up){
    fx c = up.z;
    if(c < FXC(-0.995f)) return (m3){{V3(FX_ONE, 0, 0), V3(0, -FX_ONE, 0), V3(0, 0, -FX_ONE)}};
    fx f = fx_recip(FX_ONE + c);
    return (m3){{V3(FX_ONE - fx_mul(fx_mul(f, up.x), up.x), fx_mul(fx_mul(-f, up.x), up.y), up.x),
                 V3(fx_mul(fx_mul(-f, up.x), up.y), FX_ONE - fx_mul(fx_mul(f, up.y), up.y), up.y),
                 V3(-up.x, -up.y, c)}};
}

static m3 m3_from_basis14(v3 front, v3 up){
    v3 right = v3_cross14(front, up);
    v3 r = v3_n14_to_fx(right), f = v3_n14_to_fx(front), u = v3_n14_to_fx(up);
    return (m3){{V3(r.x, f.x, u.x), V3(r.y, f.y, u.y), V3(r.z, f.z, u.z)}};
}

static void figure_update_transform(Figure* figure){
    if(!figure->transform_dirty) return;
    if(figure->use_basis){
        m3 m = m3_identity(), r;
        if(figure->local_pitch){
            r = m3_rotate_x(figure->local_pitch);
            m = m3_mul(&r, &m);
        }
        if(figure->local_roll){
            r = m3_rotate_y(figure->local_roll);
            m = m3_mul(&r, &m);
        }
        if(figure->local_yaw){
            r = m3_rotate_z(-figure->local_yaw);
            m = m3_mul(&r, &m);
        }
        m3 basis = m3_from_basis14(figure->basis_front, figure->basis_up);
        m = m3_mul(&basis, &m);
        v3 offset = V3(0, 0, 0);
        if(figure->rotation_pivot_height && (figure->local_pitch || figure->local_roll)){
            v3 pivot = V3(0, 0, figure->rotation_pivot_height);
            v3 rotated = m3_transform_vector(&m, pivot);
            v3 up_world = v3_n14_to_fx(figure->basis_up);
            offset = v3_sub(v3_scale(up_world, figure->rotation_pivot_height), rotated);
        }
        for(int i = 0; i < 3; i++) m.r[i] = v3_mul(m.r[i], figure->scale);
        figure->transform.m = m;
        figure->transform.o = v3_add(figure->position, offset);
        figure->transform_dirty = false;
        return;
    }
    v3 rot = v3_add(figure->orientation, figure->extra_rotation);
    m3 m = m3_identity(), r;
    r = m3_rotate_x(rot.y);
    m = m3_mul(&r, &m);
    r = m3_rotate_y(rot.z);
    m = m3_mul(&r, &m);
    r = m3_rotate_z(-rot.x);
    m = m3_mul(&r, &m);
    if(figure->up.x || figure->up.y || figure->up.z != FX_ONE){
        r = m3_align_up(figure->up);
        m = m3_mul(&r, &m);
    }

    v3 offset = V3(0, 0, 0);
    if(figure->rotation_pivot_height)
        offset = v3_scale(V3(-m.r[0].z, -m.r[1].z, FX_ONE - m.r[2].z), figure->rotation_pivot_height);
    for(int i = 0; i < 3; i++) m.r[i] = v3_scale(m.r[i], ((const fx*)&figure->scale)[i]);
    figure->transform.m = m;
    figure->transform.o = v3_add(figure->position, offset);
    figure->transform_dirty = false;
}
