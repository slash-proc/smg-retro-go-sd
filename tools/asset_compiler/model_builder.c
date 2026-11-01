#include "model_builder.h"
#include "obj_reader.h"
#include "png_reader.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

#define BVH_LEAF_TRIS 16
#define MAX_LEAVES    16384

typedef struct {
    float x, y, z;
} V3;

static V3 v3_sub(V3 a, V3 b) { return (V3){ a.x - b.x, a.y - b.y, a.z - b.z }; }
static V3 v3_neg(V3 a) { return (V3){ -a.x, -a.y, -a.z }; }
static float v3_dot(V3 a, V3 b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
static V3 v3_cross(V3 a, V3 b) {
    return (V3){ a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x };
}
static V3 v3_normalize(V3 a) {
    float m = sqrtf(v3_dot(a, a));
    return (V3){ a.x / m, a.y / m, a.z / m };
}

static V3 obj_to_game(const float *p) { return (V3){ -p[0], p[2], p[1] }; }

static V3 model_point_as_float(const Vector *points, int i, int shift) {
    const int16_t *p = &VEC_AT(*points, Point3, i)[0];
    float s = (float)(1 << shift) / 256.0f;
    return (V3){ p[0] * s, p[1] * s, p[2] * s };
}

static void convert_positions(const Obj *obj, Vector *out, int shift) {
    *out = VEC(int16_t[3]);
    float scale = 1000.0f / (float)(1 << shift);
    for (size_t i = 0; i < obj->positions.len; i++) {
        const float *p = OBJ_POS(obj, i);
        int16_t q[3] = { float_to_i16_saturated(p[0] * scale), float_to_i16_saturated(p[1] * scale),
            float_to_i16_saturated(p[2] * scale) };
        int16_t *d = vec_push(out);
        d[0] = (int16_t)-q[0];
        d[1] = q[2];
        d[2] = q[1];
    }
}

Vector model_load_positions(const char *path) {
    Obj obj = obj_load(path);
    Vector points;
    convert_positions(&obj, &points, 0);
    return points;
}

int model_point_shift(const char *path) {
    Obj obj = obj_load(path);
    float m = 0;
    for (size_t i = 0; i < obj.positions.len; i++)
        for (int k = 0; k < 3; k++) m = fmaxf(m, fabsf(OBJ_POS(&obj, i)[k] * 1000.0f));
    int shift = 0;
    while (m / (float)(1 << shift) > 32760.0f) shift++;
    return shift;
}

static float wrap_texture_coord(float x) {
    float r = fmodf(x, 1.0f);
    return r < 0.0f ? r + 1.0f : r;
}

/* Translate an entire face by whole texture periods before narrowing. This
 * preserves every quantized gradient and sample modulo512; wrapping/clamping
 * vertices independently changes interpolation. Leave one period of headroom
 * for the runtime's signed animated-UV offset. */
static void model_triangle_uvs(ModelVertex out[3], const float uv[3][2]) {
    for (int axis = 0; axis < 2; axis++) {
        int32_t raw[3];
        for (int i = 0; i < 3; i++) {
            if (!isfinite(uv[i][axis]) || fabsf(uv[i][axis]) > 1000000.0f)
                die("nonfinite or excessive model UV");
            raw[i] = float_to_i32_saturated(uv[i][axis] * 512.0f);
        }
        int32_t lo = raw[0], hi = raw[0];
        for (int i = 1; i < 3; i++) {
            if (raw[i] < lo) lo = raw[i];
            if (raw[i] > hi) hi = raw[i];
        }
        const int32_t minimum = INT16_MIN + 512, maximum = INT16_MAX - 512;
        int64_t shift = 0;
        if (lo < minimum || hi > maximum) {
            int64_t low_shift = (int64_t)hi - maximum;
            int64_t high_shift = (int64_t)lo - minimum;
            int64_t first = low_shift / 512 + (low_shift % 512 > 0);
            int64_t last = high_shift / 512 - (high_shift % 512 < 0);
            if (first > last) die("triangle UV span exceeds signed16 storage");
            int64_t periods = ((int64_t)lo + hi) / 1024;
            if (periods < first) periods = first;
            if (periods > last) periods = last;
            shift = periods * 512;
        }
        for (int i = 0; i < 3; i++) {
            int16_t value = (int16_t)(raw[i] - shift);
            if (axis == 0) out[i].u = value;
            else out[i].v = value;
        }
    }
}

static const uint8_t *texture_pixel_at(const Image *img, int x, int y) {
    uint32_t u = float_to_u32_saturated(wrap_texture_coord((float)x / 32.0f) * (float)img->w);
    uint32_t v = float_to_u32_saturated(wrap_texture_coord((float)y / 32.0f) * (float)img->h);
    if (u >= (uint32_t)img->w || v >= (uint32_t)img->h) die("texture sample out of range");
    return img->rgba + ((size_t)v * img->w + u) * 4;
}

static void texture_build_mip(uint8_t *texels, const Image *img, int radius, float intensity, int underwater,
                              int *transparent, const Rgba *pal) {
    for (int j = 0; j < 32; j++) {
        for (int i = 0; i < 32; i++) {
            int counts[256] = { 0 };
            for (int l = -radius; l <= radius; l++) {
                for (int k = -radius; k <= radius; k++) {
                    const uint8_t *px = texture_pixel_at(img, i + k, j + l);
                    if (px[3] < 128) continue;
                    float col[3] = { (float)px[0] * intensity, (float)px[1] * intensity,
                                     (float)px[2] * intensity };
                    if (underwater) {
                        const float tint[3] = { 0.08f * 255.0f, 0.42f * 255.0f, 0.65f * 255.0f };
                        for (int c = 0; c < 3; c++) col[c] = fmaf(0.18f, tint[c] - col[c], col[c]);
                    }
                    counts[palette_nearest_opaque(pal, col)]++;
                }
            }
            uint8_t out = 0;
            if (texture_pixel_at(img, i, j)[3] < 128) {
                *transparent = 1;
            } else {
                int best = 0;
                for (int c = 1; c < 256; c++)
                    if (counts[c] >= 1 && (best == 0 || counts[c] >= counts[best])) best = c;
                out = (uint8_t)best;
            }
            texels[(31 - j) * 32 + i] = out;
        }
    }
}

static void texture_build_mip1(uint8_t *texels, const Image *img, float intensity, int underwater,
                               int *transparent, const Rgba *pal) {
    memset(texels, 0, TEX_TEXELS);
    for (int r = 0; r < TEX_MIP1_ROWS; r++) {
        int j0 = 31 - 2 * r, j1 = j0 - 1;
        for (int i = 0; i < 32; i++) {
            int counts[256] = { 0 };
            for (int l = j1 - 2; l <= j0 + 2; l++) {
                for (int k = -2; k <= 2; k++) {
                    const uint8_t *px = texture_pixel_at(img, i + k, l);
                    if (px[3] < 128) continue;
                    float col[3] = { (float)px[0] * intensity, (float)px[1] * intensity,
                                     (float)px[2] * intensity };
                    if (underwater) {
                        const float tint[3] = { 0.08f * 255.0f, 0.42f * 255.0f, 0.65f * 255.0f };
                        for (int c = 0; c < 3; c++) col[c] = fmaf(0.18f, tint[c] - col[c], col[c]);
                    }
                    counts[palette_nearest_opaque(pal, col)]++;
                }
            }
            uint8_t out = 0;
            if (texture_pixel_at(img, i, j0)[3] < 128 && texture_pixel_at(img, i, j1)[3] < 128) {
                *transparent = 1;
            } else {
                int best = 0;
                for (int c = 1; c < 256; c++)
                    if (counts[c] >= 1 && (best == 0 || counts[c] >= counts[best])) best = c;
                out = (uint8_t)best;
            }
            texels[r * TEX_MIP1_STRIDE + i] = out;
        }
    }
}

Texture texture_from_image(const char *path, int mip1_radius, const Rgba *palette) {
    Image img = png_load(path);
    Texture t;
    int transparent = 0;
    texture_build_mip(t.mip[0], &img, 0, 1.0f, 0, &transparent, palette);
    texture_build_mip(t.mip[1], &img, mip1_radius, 1.0f, 0, &transparent, palette);
    image_free(&img);
    return t;
}

typedef struct {
    char *file;
    uint32_t level;
    int texture, water_texture, alpha, blend_quarter;
} TexEntry;

typedef struct {
    char *file;
    Image img;
} ImgEntry;

typedef struct {
    const ModelParams *p;
    const Rgba *pal;
    const Mtl *mtl;
    Model *m;
    Vector entries;
    Vector images;
} TexCache;

static int texture_cache_append(TexCache *tc, const Texture *texture) {
    if (!tc->p->is_figure) {
        for (size_t i = 0; i < tc->m->textures.len; i++)
            if (!memcmp(&VEC_AT(tc->m->textures, Texture, i), texture, sizeof *texture))
                return (int)i;
    }
    int index = (int)tc->m->textures.len;
    *(Texture *)vec_push(&tc->m->textures) = *texture;
    return index;
}

/* Some simplified surfaces replace the backing mesh with an alpha decal.
 * Bake an explicitly configured opaque backing at the same UVs before mip
 * generation; this costs no extra triangles or runtime blending. */
static void texture_apply_underlay(Image *foreground, const Image *background) {
    if (foreground->w != background->w || foreground->h != background->h)
        die("texture underlay dimensions must match the foreground");
    for (size_t i = 0; i < (size_t)foreground->w * foreground->h; i++) {
        uint8_t *f = foreground->rgba + 4 * i;
        const uint8_t *b = background->rgba + 4 * i;
        if (b[3] != 255) die("texture underlay must be opaque");
        unsigned alpha = f[3];
        for (int c = 0; c < 3; c++)
            f[c] = (f[c] * alpha + b[c] * (255 - alpha) + 127) / 255;
        f[3] = 255;
    }
}

/* Resolve named material semantics at the compiler's final sampling grid.
 * Keep reconstructed PNGs/palette inputs unchanged: recipes have residuals
 * against those exact pixels. Native and WASM use this same compiler. */
static void texture_apply_alpha_policy(Image *image, int mode) {
    static const uint8_t bayer[16] = { 0,8,2,10, 12,4,14,6, 3,11,1,9, 15,7,13,5 };
    for (int y = 0; y < image->h; y++) for (int x = 0; x < image->w; x++) {
        uint8_t *alpha = image->rgba + ((size_t)y * image->w + x) * 4 + 3;
        if (mode == TEXTURE_ALPHA_OPAQUE || mode == TEXTURE_ALPHA_BLEND_QUARTER) *alpha = 255;
        else if (mode == TEXTURE_ALPHA_COVERAGE) {
            unsigned rank = bayer[((y & 3) << 2) | (x & 3)];
            *alpha = (unsigned)*alpha * 32 >= (2 * rank + 1) * 255 ? 255 : 0;
        } else die("invalid texture alpha policy");
    }
}

static const TexEntry *texture_cache_get_lit(TexCache *tc, const char *material, float intensity) {
    const char *name = mtl_texture(tc->mtl, material);
    if (!name) return NULL;
    char *file = format_string("%s/%s", tc->p->dir, name);

    uint32_t level = float_to_u32_saturated(roundf(intensity * 6.0f / fmaxf(tc->p->shading, 0.7f)));

    for (size_t i = 0; i < tc->entries.len; i++) {
        TexEntry *e = &VEC_AT(tc->entries, TexEntry, i);
        if (e->level == level && !strcmp(e->file, file)) {
            free(file);
            return e;
        }
    }

    const Image *img = NULL;
    for (size_t i = 0; i < tc->images.len; i++)
        if (!strcmp(VEC_AT(tc->images, ImgEntry, i).file, file)) img = &VEC_AT(tc->images, ImgEntry, i).img;
    if (!img) {
        ImgEntry *ie = vec_push(&tc->images);
        ie->file = xstrdup(file);
        ie->img = png_load(file);
        for (int i = 0; i < tc->p->texture_alpha_policy_count; i++) {
            const TextureAlphaPolicy *policy = &tc->p->texture_alpha_policies[i];
            if (!strcmp(name, policy->texture)) texture_apply_alpha_policy(&ie->img, policy->mode);
        }
        for (int i = 0; i < tc->p->texture_underlay_count; i++) {
            const TextureUnderlay *layer = &tc->p->texture_underlays[i];
            if (strcmp(name, layer->foreground)) continue;
            char *base_file = format_string("%s/%s", tc->p->dir, layer->background);
            Image base = png_load(base_file);
            texture_apply_underlay(&ie->img, &base);
            image_free(&base);
            free(base_file);
        }
        img = &ie->img;
    }

    TexEntry *e = vec_push(&tc->entries);
    e->file = file;
    e->level = level;
    for (int i = 0; i < tc->p->texture_alpha_policy_count; i++) {
        const TextureAlphaPolicy *policy = &tc->p->texture_alpha_policies[i];
        if (!strcmp(name, policy->texture) && policy->mode == TEXTURE_ALPHA_BLEND_QUARTER)
            e->blend_quarter = 1;
    }
    Texture texture;
    texture_build_mip(texture.mip[0], img, 0, intensity, 0, &e->alpha, tc->pal);
    texture_build_mip1(texture.mip[1], img, intensity, 0, &e->alpha, tc->pal);
    e->texture = texture_cache_append(tc, &texture);
    e->water_texture = e->texture;
    if (tc->p->underwater) {
        texture_build_mip(texture.mip[0], img, 0, intensity, 1, &e->alpha, tc->pal);
        texture_build_mip1(texture.mip[1], img, intensity, 1, &e->alpha, tc->pal);
        e->water_texture = texture_cache_append(tc, &texture);
    }
    return e;
}

typedef struct {
    uint64_t key;
    V3 a, b;
    int count;
    V3 c[2], n[2];
} EdgeEntry;

typedef struct {
    EdgeEntry *slots;
    size_t cap;
} EdgeMap;

static uint64_t edge_key(int a, int b) {
    if (a > b) {
        int t = a;
        a = b;
        b = t;
    }
    return ((uint64_t)(uint32_t)a << 32 | (uint32_t)b) + 1;
}

static EdgeEntry *edge_slot(EdgeMap *m, uint64_t key) {
    size_t i = (size_t)(key * 0x9E3779B97F4A7C15ull >> 20) & (m->cap - 1);
    while (m->slots[i].key && m->slots[i].key != key) i = (i + 1) & (m->cap - 1);
    return &m->slots[i];
}

static EdgeMap edge_map_build(const Obj *obj) {
    EdgeMap m;
    m.cap = 16;
    while (m.cap < obj->verts.len * 4) m.cap *= 2;
    m.slots = xcalloc(m.cap, sizeof(EdgeEntry));
    for (size_t o = 0; o < obj->objects.len; o++) {
        const ObjObject *ob = &VEC_AT(obj->objects, ObjObject, o);
        for (size_t g = 0; g < ob->groups.len; g++) {
            const ObjGroup *gr = &VEC_AT(ob->groups, ObjGroup, g);
            for (size_t f = 0; f < gr->faces.len; f++) {
                const ObjFace *face = &VEC_AT(gr->faces, ObjFace, f);
                for (int t = 0; t + 2 < face->count; t++) {
                    int idx[3] = { VEC_AT(obj->verts, ObjVertex, face->first).pos,
                                   VEC_AT(obj->verts, ObjVertex, face->first + t + 1).pos,
                                   VEC_AT(obj->verts, ObjVertex, face->first + t + 2).pos };
                    V3 p[3] = { obj_to_game(OBJ_POS(obj, idx[0])), obj_to_game(OBJ_POS(obj, idx[1])),
                                obj_to_game(OBJ_POS(obj, idx[2])) };
                    V3 n = v3_normalize(v3_cross(v3_sub(p[0], p[1]), v3_sub(p[2], p[0])));
                    for (int i = 0; i < 3; i++) {
                        EdgeEntry *e = edge_slot(&m, edge_key(idx[i], idx[(i + 1) % 3]));
                        if (!e->key) {
                            e->key = edge_key(idx[i], idx[(i + 1) % 3]);
                            e->a = p[i];
                            e->b = p[(i + 1) % 3];
                        }
                        if (e->count < 2) {
                            e->c[e->count] = p[(i + 2) % 3];
                            e->n[e->count] = n;
                        }
                        e->count++;
                    }
                }
            }
        }
    }
    return m;
}

static int is_convex(V3 a, V3 n0, V3 c1, V3 n1) {
    if (v3_dot(v3_sub(c1, a), n0) > 0.0f) return 0;
    return v3_dot(n0, n1) < 0.35f;
}

static int is_ledge(EdgeMap *m, int a, int b) {
    EdgeEntry *e = edge_slot(m, edge_key(a, b));
    if (!e->key || e->count != 2) return 0;
    return fabsf(v3_normalize(v3_sub(e->a, e->b)).z) < 0.5f && is_convex(e->a, e->n[0], e->c[1], e->n[1]) &&
           is_convex(e->a, e->n[1], e->c[0], e->n[0]);
}

typedef struct {
    uint16_t surface;
    float uv_scroll_speed[2];
    int double_sided, invisible;
    int has_light, source_ice;
    unsigned floor_code, wall_code;
    float light;
} MatAttr;

static MatAttr parse_material(const char *mat, const char *file) {
    MatAttr ma = { 0 };
    const char *colon = strrchr(mat, ':');
    if (!colon) return ma;
    char *attrs = xstrdup(colon + 1);
    for (char *tok = strtok(attrs, ","); tok; tok = strtok(NULL, ",")) {
        while (*tok == ' ') tok++;
        char *val = strchr(tok, '=');
        if (val) *val++ = 0;
        for (char *e = tok + strlen(tok); e > tok && e[-1] == ' '; e--) e[-1] = 0;
        if (!strcmp(tok, "floor") && !val) ma.surface |= SURF_FLOOR;
        else if (!strcmp(tok, "unsolid") && !val) ma.surface |= SURF_UNSOLID;
        else if (!strcmp(tok, "double_sided") && !val) ma.double_sided = 1;
        else if (!strcmp(tok, "invisible") && !val) ma.invisible = 1;
        else if (!strcmp(tok, "death") && !val) ma.surface |= SURF_DEATH;
        else if (!strcmp(tok, "slip") && !val) ma.surface |= SURF_SLIPPERY;
        else if (!strcmp(tok, "honey") && !val) ma.surface |= SURF_HONEY;
        else if (!strcmp(tok, "ice") && !val) ma.source_ice = 1;
        else if ((!strcmp(tok, "floor_code") || !strcmp(tok, "wall_code")) && val) {
            char *end;
            unsigned long code = strtoul(val, &end, 10);
            if (*end || code > 255) die("%s: collision code must fit in a byte", file);
            if (!strcmp(tok, "floor_code")) ma.floor_code = (unsigned)code;
            else ma.wall_code = (unsigned)code;
        } else if (!strcmp(tok, "scen") && val) {
            char *end;
            unsigned long mask = strtoul(val, &end, 16);
            if (*end || mask > 0x3f) die("%s: scenario mask must fit in six bits", file);
            ma.surface |= (uint16_t)(mask << 10);
        } else if (!strcmp(tok, "nograb") && !val) {
            ma.surface |= SURF_NOT_GRAB;
        } else if (!strcmp(tok, "nocamera") && !val) {
            ma.surface |= SURF_NOCAMERA;
        } else if (!strcmp(tok, "water") && !val) {
            ma.surface |= SURF_WATER;
        } else if (!strcmp(tok, "lit") && val) {
            ma.light = strtof(val, NULL);
            ma.has_light = 1;
        } else if (!strcmp(tok, "unlit") && !val) {
            ma.light = 1.0f;
            ma.has_light = 1;
        } else if (!strcmp(tok, "animate_u") && val) {
            ma.uv_scroll_speed[0] = strtof(val, NULL);
            ma.surface |= SURF_ANIM_UV;
        } else if (!strcmp(tok, "animate_v") && val) {
            ma.uv_scroll_speed[1] = strtof(val, NULL);
            ma.surface |= SURF_ANIM_UV;
        } else {
            die("%s: unsupported material attribute '%s' in '%s'", file, tok, mat);
        }
    }
    free(attrs);
    return ma;
}

typedef struct {
    Model *m;
    int base;
    uint16_t *tri_leaf;
    int axis;
} Bvh;

static void triangle_point(const Model *m, int t, int i, int64_t out[3]) {
    const BuildTriangle *tri = &VEC_AT(m->triangles, BuildTriangle, t);
    const int16_t *p = VEC_AT(m->points, Point3, tri->v[i].point);
    for (int c = 0; c < 3; c++) out[c] = p[c];
}

static int64_t triangle_centroid_sum(const Model *m, int t, int axis) {
    int64_t a[3], b[3], c[3];
    triangle_point(m, t, 0, a);
    triangle_point(m, t, 1, b);
    triangle_point(m, t, 2, c);
    return a[axis] + b[axis] + c[axis];
}

static int compare_triangle_centroids(const void *x, const void *y, void *ctx) {
    const Bvh *b = ctx;
    int ta = *(const int *)x, tb = *(const int *)y;
    int64_t ca = triangle_centroid_sum(b->m, ta, b->axis), cb = triangle_centroid_sum(b->m, tb, b->axis);
    if (ca != cb) return ca < cb ? -1 : 1;
    return (ta > tb) - (ta < tb);
}

static int compare_ints(const void *x, const void *y, void *ctx) {
    (void)ctx;
    return (*(const int *)x > *(const int *)y) - (*(const int *)x < *(const int *)y);
}

static int vector_fits_i16(const int64_t v[3]) {
    for (int i = 0; i < 3; i++)
        if (v[i] < INT16_MIN || v[i] > INT16_MAX) return 0;
    return 1;
}

static void build_bvh_node(Bvh *b, int *group, int n) {
    Model *m = b->m;
    int64_t lo[3] = { INT64_MAX, INT64_MAX, INT64_MAX }, hi[3] = { INT64_MIN, INT64_MIN, INT64_MIN };
    int64_t sum[3] = { 0, 0, 0 };
    for (int g = 0; g < n; g++) {
        for (int i = 0; i < 3; i++) {
            int64_t p[3];
            triangle_point(m, group[g], i, p);
            for (int c = 0; c < 3; c++) {
                if (p[c] < lo[c]) lo[c] = p[c];
                if (p[c] > hi[c]) hi[c] = p[c];
            }
        }
        const BuildTriangle *tri = &VEC_AT(m->triangles, BuildTriangle, group[g]);
        for (int c = 0; c < 3; c++) sum[c] += tri->normal[c];
    }

    int64_t cone_normal[3], cone_normal_extent[3] = { 0, 0, 0 }, cone_center[3];
    for (int c = 0; c < 3; c++) {
        cone_normal[c] = (int64_t)round((double)sum[c] / (double)n);
        cone_center[c] = (lo[c] + hi[c]) >> 1;
    }
    int64_t cone_plane_limit = INT64_MIN;
    for (int g = 0; g < n; g++) {
        const BuildTriangle *tri = &VEC_AT(m->triangles, BuildTriangle, group[g]);
        int64_t p0[3];
        triangle_point(m, group[g], 0, p0);
        int64_t d = 0;
        for (int c = 0; c < 3; c++) {
            int64_t e = llabs(tri->normal[c] - cone_normal[c]);
            if (e > cone_normal_extent[c]) cone_normal_extent[c] = e;
            d += (int64_t)tri->normal[c] * cone_center[c] - (int64_t)tri->normal[c] * p0[c];
        }
        if (d > cone_plane_limit) cone_plane_limit = d;
    }

    int64_t n2[3], e2[3];
    for (int c = 0; c < 3; c++) {
        n2[c] = (int64_t)llround((double)cone_normal[c] / 2.0);
        if (n2[c] > 127) n2[c] = 127;
        if (n2[c] < -127) n2[c] = -127;
        int64_t err = llabs(2 * n2[c] - cone_normal[c]);
        e2[c] = (cone_normal_extent[c] + err + 1) / 2;
    }
    int can_cull = 0;
    for (int c = 0; c < 3; c++)
        if (e2[c] < llabs(n2[c])) can_cull = 1;
    int cone_ok = can_cull && e2[0] <= 255 && e2[1] <= 255 && e2[2] <= 255 &&
        vector_fits_i16(cone_center) && llabs(cone_plane_limit) < (1 << 28);

    size_t idx = m->chunks.len;
    StageChunk *ch = vec_push(&m->chunks);
    for (int c = 0; c < 3; c++) {
        ch->min[c] = (int16_t)lo[c];
        ch->max[c] = (int16_t)hi[c];
        ch->cone_normal[c] = cone_ok ? (int8_t)n2[c] : 0;
        ch->cone_normal_extent[c] = cone_ok ? (uint8_t)e2[c] : 0;
        ch->cone_center[c] = (int16_t)cone_center[c];
    }
    ch->leaf = CHUNK_INNER;
    ch->cone_plane_limit = cone_ok ? (int32_t)cone_plane_limit : CONE_NONE;

    uint8_t cls0 = VEC_AT(m->triangles, BuildTriangle, group[0]).occl, cls_min = cls0;
    int mixed = 0;
    for (int g = 1; g < n; g++) {
        uint8_t c = VEC_AT(m->triangles, BuildTriangle, group[g]).occl;
        if (c != cls0) mixed = 1;
        if (c < cls_min) cls_min = c;
    }
    if (mixed) {
        int *tmp = xmalloc(sizeof(int) * (size_t)n), k = 0;
        for (int g = 0; g < n; g++)
            if (VEC_AT(m->triangles, BuildTriangle, group[g]).occl == cls_min) tmp[k++] = group[g];
        int first = k;
        for (int g = 0; g < n; g++)
            if (VEC_AT(m->triangles, BuildTriangle, group[g]).occl != cls_min) tmp[k++] = group[g];
        memcpy(group, tmp, sizeof(int) * (size_t)n);
        free(tmp);
        build_bvh_node(b, group, first);
        build_bvh_node(b, group + first, n - first);
    } else if (n > BVH_LEAF_TRIS) {
        int64_t clo[3] = { INT64_MAX, INT64_MAX, INT64_MAX }, chi[3] = { INT64_MIN, INT64_MIN, INT64_MIN };
        for (int g = 0; g < n; g++)
            for (int c = 0; c < 3; c++) {
                int64_t v = triangle_centroid_sum(m, group[g], c);
                if (v < clo[c]) clo[c] = v;
                if (v > chi[c]) chi[c] = v;
            }
        int64_t ext[3] = { chi[0] - clo[0], chi[1] - clo[1], chi[2] - clo[2] };
        b->axis = ext[0] >= ext[1] && ext[0] >= ext[2] ? 0 : ext[1] >= ext[2] ? 1 : 2;
        stable_sort(group, (size_t)n, sizeof(int), compare_triangle_centroids, b);
        int half = n / 2;
        build_bvh_node(b, group, half);
        build_bvh_node(b, group + half, n - half);
    } else {
        uint16_t leaf = (uint16_t)(m->leaf_triangle_offsets.len);
        VEC_AT(m->chunks, StageChunk, idx).leaf = leaf;
        *(uint8_t *)vec_push(&m->leaf_occl) = cls0;
        stable_sort(group, (size_t)n, sizeof(int), compare_ints, NULL);
        *(uint16_t *)vec_push(&m->leaf_triangle_offsets) = (uint16_t)m->leaf_triangles.len;
        for (int g = 0; g < n; g++) {
            b->tri_leaf[group[g] - b->base] = leaf;
            *(uint16_t *)vec_push(&m->leaf_triangles) = (uint16_t)group[g];
        }
    }
    VEC_AT(m->chunks, StageChunk, idx).skip = (uint16_t)m->chunks.len;
}

static int compare_axis_offsets(const void *x, const void *y, void *ctx) {
    (void)ctx;
    int16_t a = ((const AxisEntry *)x)->offset, b = ((const AxisEntry *)y)->offset;
    return (a > b) - (a < b);
}

static void append_axis_draw_order(Model *m, Vector *list, int first, int end, int axis, int sign) {
    size_t start = list->len;
    for (int t = first; t < end; t++) {
        const BuildTriangle *tri = &VEC_AT(m->triangles, BuildTriangle, t);
        int32_t best = INT32_MAX;
        for (int i = 0; i < 3; i++) {
            int32_t v = VEC_AT(m->points, Point3, tri->v[i].point)[axis] * sign;
            if (v < best) best = v;
        }
        AxisEntry *e = vec_push(list);
        e->offset = (int16_t)best;
        e->tri = (uint16_t)t;
    }
    stable_sort((AxisEntry *)list->data + start, list->len - start, sizeof(AxisEntry), compare_axis_offsets,
                NULL);
}

typedef struct {
    const Obj *obj;
    const Model *m;
    int axis, sign;
} FigOrder;

static int32_t figure_triangle_far_extent(const FigOrder *o, uint16_t t) {
    const BuildTriangle *tri = &VEC_AT(o->m->triangles, BuildTriangle, t);
    float best = -INFINITY;
    for (int i = 0; i < 3; i++) {
        const float *p = OBJ_POS(o->obj, tri->v[i].point);
        V3 q = obj_to_game((float[3]){ p[0] * 1000.0f, p[1] * 1000.0f, p[2] * 1000.0f });
        float axis[3] = { 0, 0, 0 };
        axis[o->axis] = (float)o->sign;
        float d = v3_dot(q, (V3){ axis[0], axis[1], axis[2] });
        if (i == 0 || d > best) best = d;
    }
    return float_to_i32_saturated(best);
}

static int compare_figure_depths(const void *x, const void *y, void *ctx) {
    int32_t a = figure_triangle_far_extent(ctx, *(const uint16_t *)x),
        b = figure_triangle_far_extent(ctx, *(const uint16_t *)y);
    return (a > b) - (a < b);
}

static void append_triangle(Vector *triangles, Vector *source_codes, const ModelVertex v[3],
                            const int16_t normal[3], const TexEntry *texture, const MatAttr *ma,
                            const int16_t uv_scroll_speed[2], int ledges) {
    BuildTriangle *t = vec_push(triangles);
    memcpy(t->v, v, sizeof t->v);
    memcpy(t->normal, normal, sizeof t->normal);
    t->texture = texture ? (uint16_t)texture->texture : 0;
    t->water_texture = texture ? (uint16_t)texture->water_texture : 0;
    t->flags = ma->surface | (texture && (texture->alpha || texture->blend_quarter) ? TRI_ALPHA : 0);
    t->uv_scroll_speed[0] = uv_scroll_speed[0];
    t->uv_scroll_speed[1] = uv_scroll_speed[1];
    t->ledges = (uint8_t)ledges | (ma->source_ice ? TRI_SOURCE_ICE : 0);
    if (texture && texture->blend_quarter) t->ledges |= TRI_SOURCE_BLEND_QUARTER;
    if (ma->floor_code == 10) t->ledges |= TRI_SOURCE_BURN;
    *(uint16_t *)vec_push(source_codes) = (uint16_t)(ma->floor_code | ma->wall_code << 8);
}

#define OCCL_STEP (1.0f / 16.0f)
#define OCCL_MARGIN 0.05f
typedef struct {
    float cx, cy, r, zp, zq;
} OcclShell;

static int occl_inside(const OcclShell *o, float x, float y) {
    return (x - o->cx) * (x - o->cx) + (y - o->cy) * (y - o->cy) <= o->r * o->r;
}

static void occl_envelope(Model *m, const Obj *obj, const ModelParams *p, OcclShell *o) {
    o->cx = p->occluder[0];
    o->cy = p->occluder[1];
    o->r = p->occluder[2];
    int n = (int)ceilf(2.0f * o->r / OCCL_STEP) + 1;
    float x0 = o->cx - o->r, y0 = o->cy - o->r;
    float *up = xmalloc(sizeof(float) * (size_t)n * (size_t)n);
    float *lo = xmalloc(sizeof(float) * (size_t)n * (size_t)n);
    uint8_t *upf = xcalloc((size_t)n * (size_t)n, 1), *lof = xcalloc((size_t)n * (size_t)n, 1);
    for (int i = 0; i < n * n; i++) {
        up[i] = -INFINITY;
        lo[i] = INFINITY;
    }
    for (size_t ob = 0; ob < obj->objects.len; ob++) {
        const ObjObject *oo = &VEC_AT(obj->objects, ObjObject, ob);
        for (size_t g = 0; g < oo->groups.len; g++) {
            const ObjGroup *gr = &VEC_AT(oo->groups, ObjGroup, g);
            for (size_t f = 0; f < gr->faces.len; f++) {
                const ObjFace *face = &VEC_AT(gr->faces, ObjFace, f);
                const char *mat = face->mat >= 0 ? VEC_AT(obj->materials, char *, face->mat) : "";
                MatAttr ma = parse_material(mat, p->obj);
                if (ma.invisible) continue;

                int conditional = (ma.surface & SURF_SCENARIO_MASK) != 0;
                for (int k = 0; k < p->part_count; k++)
                    if (oo->name && !strcmp(oo->name, p->part_names[k])) conditional = 1;
                const ObjVertex *fv = &VEC_AT(obj->verts, ObjVertex, face->first);
                for (int t = 0; t + 2 < face->count; t++) {
                    V3 a = model_point_as_float(&m->points, fv[0].pos, p->point_shift),
                       b = model_point_as_float(&m->points, fv[t + 1].pos, p->point_shift),
                       c = model_point_as_float(&m->points, fv[t + 2].pos, p->point_shift);
                    V3 nf = v3_cross(v3_sub(a, b), v3_sub(c, b));
                    float area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
                    if (fabsf(area) < 1e-9f) continue;
                    float lx = fminf(a.x, fminf(b.x, c.x)), hx = fmaxf(a.x, fmaxf(b.x, c.x));
                    float ly = fminf(a.y, fminf(b.y, c.y)), hy = fmaxf(a.y, fmaxf(b.y, c.y));
                    int i0 = (int)ceilf((lx - x0) / OCCL_STEP), i1 = (int)floorf((hx - x0) / OCCL_STEP);
                    int j0 = (int)ceilf((ly - y0) / OCCL_STEP), j1 = (int)floorf((hy - y0) / OCCL_STEP);
                    if (i0 < 0) i0 = 0;
                    if (j0 < 0) j0 = 0;
                    if (i1 > n - 1) i1 = n - 1;
                    if (j1 > n - 1) j1 = n - 1;
                    for (int j = j0; j <= j1; j++)
                        for (int i = i0; i <= i1; i++) {
                            float x = x0 + i * OCCL_STEP, y = y0 + j * OCCL_STEP;
                            float wa = ((b.x - x) * (c.y - y) - (b.y - y) * (c.x - x)) / area;
                            float wb = ((c.x - x) * (a.y - y) - (c.y - y) * (a.x - x)) / area;
                            float wc = 1.0f - wa - wb;
                            if (wa < -1e-5f || wb < -1e-5f || wc < -1e-5f) continue;
                            float z = wa * a.z + wb * b.z + wc * c.z;
                            int k = j * n + i;

                            int fu = (nf.z > 0 || ma.double_sided) && !conditional;
                            int fd = (nf.z < 0 || ma.double_sided) && !conditional;
                            if (z > up[k] + 1e-4f) {
                                up[k] = z;
                                upf[k] = (uint8_t)fu;
                            } else if (z >= up[k] - 1e-4f) {
                                upf[k] |= (uint8_t)fu;
                            }
                            if (z < lo[k] - 1e-4f) {
                                lo[k] = z;
                                lof[k] = (uint8_t)fd;
                            } else if (z <= lo[k] + 1e-4f) {
                                lof[k] |= (uint8_t)fd;
                            }
                        }
                }
            }
        }
    }
    float up_min = INFINITY, lo_max = -INFINITY;
    for (int j = 0; j < n; j++)
        for (int i = 0; i < n; i++) {
            float x = x0 + i * OCCL_STEP, y = y0 + j * OCCL_STEP;
            if (!occl_inside(o, x, y)) continue;
            int k = j * n + i;
            if (!isfinite(up[k]) || !upf[k] || !isfinite(lo[k]) || !lof[k]) {
                const char *missing = !isfinite(up[k]) ? "above or below"
                                      : !upf[k]        ? "looking up on top"
                                                       : "looking down underneath";
                die("%s: occluder shell open at %.3f %.3f (game units): no face %s", p->obj, x, y, missing);
            }
            up_min = fminf(up_min, up[k]);
            lo_max = fmaxf(lo_max, lo[k]);
        }
    o->zp = up_min - OCCL_MARGIN;
    o->zq = lo_max + OCCL_MARGIN;

    m->occl_x0 = floorf(x0);
    m->occl_y0 = floorf(y0);
    m->occl_n = (int)ceilf(o->cx + o->r - m->occl_x0);
    if ((int)ceilf(o->cy + o->r - m->occl_y0) > m->occl_n) m->occl_n = (int)ceilf(o->cy + o->r - m->occl_y0);
    int cn = m->occl_n;
    m->occl_above = xmalloc(sizeof(float) * (size_t)cn * (size_t)cn);
    m->occl_below = xmalloc(sizeof(float) * (size_t)cn * (size_t)cn);
    for (int cj = 0; cj < cn; cj++)
        for (int ci = 0; ci < cn; ci++) {
            float ax = m->occl_x0 + ci, ay = m->occl_y0 + cj;
            int whole = occl_inside(o, ax, ay) && occl_inside(o, ax + 1, ay) && occl_inside(o, ax, ay + 1) &&
                        occl_inside(o, ax + 1, ay + 1);
            float hi = -INFINITY, lw = INFINITY;
            for (int j = (int)floorf((ay - y0) / OCCL_STEP) - 1;
                 whole && j <= (int)ceilf((ay + 1 - y0) / OCCL_STEP) + 1; j++)
                for (int i = (int)floorf((ax - x0) / OCCL_STEP) - 1;
                     i <= (int)ceilf((ax + 1 - x0) / OCCL_STEP) + 1; i++) {
                    if (i < 0 || j < 0 || i >= n || j >= n) continue;
                    if (!occl_inside(o, x0 + i * OCCL_STEP, y0 + j * OCCL_STEP)) continue;
                    hi = fmaxf(hi, up[j * n + i]);
                    lw = fminf(lw, lo[j * n + i]);
                }
            m->occl_above[cj * cn + ci] = whole ? hi + OCCL_MARGIN : NAN;
            m->occl_below[cj * cn + ci] = whole ? lw - OCCL_MARGIN : NAN;
        }
    printf("  occluder: shell over r %.2f about %.2f %.2f: upper envelope from %.2f, lower up to %.2f\n",
           o->r, o->cx, o->cy, up_min, lo_max);
    free(up);
    free(lo);
    free(upf);
    free(lof);
}

static uint8_t occl_class(const OcclShell *o, V3 a, V3 b, V3 c) {
    if (!occl_inside(o, a.x, a.y) || !occl_inside(o, b.x, b.y) || !occl_inside(o, c.x, c.y)) return 0;
    if (a.z < o->zp && b.z < o->zp && c.z < o->zp) return 1;
    if (a.z > o->zq && b.z > o->zq && c.z > o->zq) return 2;
    return 0;
}

Model model_build(const ModelParams *p, const Rgba *palette) {
#ifdef SMG_ASSETC_PROGRESS
    extern void smg_assetc_progress(uint32_t phase, uint32_t index, uint32_t total);
#define MODEL_PROGRESS(index, total) do { if (!p->is_figure) smg_assetc_progress(32, (index), (total)); } while (0)
#else
#define MODEL_PROGRESS(index, total) ((void)0)
#endif
    MODEL_PROGRESS(0, 0);
    char *obj_path = format_string("%s/%s", p->dir, p->obj);
    char *mtl_path = xstrdup(obj_path);
    strcpy(mtl_path + strlen(mtl_path) - 4, ".mtl");
    Obj obj = obj_load(obj_path);
    Mtl mtl = mtl_load(mtl_path);
    MODEL_PROGRESS(1, 0);

    Model m = { 0 };
    m.triangles = VEC(BuildTriangle);
    m.source_codes = VEC(uint16_t);
    m.textures = VEC(Texture);
    m.chunks = VEC(StageChunk);
    m.leaf_triangles = VEC(uint16_t);
    m.leaf_triangle_offsets = VEC(uint16_t);
    m.tri_axes = VEC(AxisEntry);
    m.tri_rank = VEC(uint16_t[6]);
    m.rank_segments = VEC(uint16_t);
    for (int k = 0; k < 6; k++) m.draw_axes[k] = VEC(uint16_t);
    Vector axis_lists[6];
    for (int k = 0; k < 6; k++) axis_lists[k] = VEC(AxisEntry);
    convert_positions(&obj, &m.points, p->point_shift);
    int source_zero_faces = 0;

    TexCache tc = { p, palette, &mtl, &m, VEC(TexEntry), VEC(ImgEntry) };
    EdgeMap edges = edge_map_build(&obj);
    MODEL_PROGRESS(2, 0);
    V3 sun = { p->sun[0], p->sun[1], p->sun[2] };

    m.leaf_occl = VEC(uint8_t);
    OcclShell occl = { 0 };
    if (p->occluder) occl_envelope(&m, &obj, p, &occl);
    int occl_counts[3] = { 0, 0, 0 };
    m.part_leaves = VEC(IntPair);
    for (int k = 0; k < p->part_count; k++) {
        int *pl = vec_push(&m.part_leaves);
        pl[0] = pl[1] = 0;
    }
    for (size_t o = 0; o < obj.objects.len; o++) {
        if ((o & 15) == 0 || o + 1 == obj.objects.len)
            MODEL_PROGRESS((uint32_t)o + 3, (uint32_t)obj.objects.len);
        const ObjObject *ob = &VEC_AT(obj.objects, ObjObject, o);
        int first = (int)m.triangles.len;
        int part = 0;
        for (int k = 0; k < p->part_count; k++)
            if (ob->name && !strcmp(ob->name, p->part_names[k])) part = k + 1;
        int leaf_first = (int)m.leaf_triangle_offsets.len;
        Vector invisible = VEC(BuildTriangle);
        Vector invisible_codes = VEC(uint16_t);

        for (size_t g = 0; g < ob->groups.len; g++) {
            const ObjGroup *gr = &VEC_AT(ob->groups, ObjGroup, g);
            for (size_t f = 0; f < gr->faces.len; f++) {
                const ObjFace *face = &VEC_AT(gr->faces, ObjFace, f);
                const char *mat = face->mat >= 0 ? VEC_AT(obj.materials, char *, face->mat) : "";
                MatAttr ma = parse_material(mat, obj_path);
                int16_t uv_scroll_speed[2] = { float_to_i16_saturated(ma.uv_scroll_speed[0] * 32.0f * 256.0f),
                    float_to_i16_saturated(ma.uv_scroll_speed[1] * 32.0f * 256.0f) };
                const ObjVertex *fv = &VEC_AT(obj.verts, ObjVertex, face->first);

                V3 a = model_point_as_float(&m.points, fv[0].pos, p->point_shift),
                   b = model_point_as_float(&m.points, fv[1].pos, p->point_shift),
                   c = model_point_as_float(&m.points, fv[2].pos, p->point_shift);
                V3 cross = v3_cross(v3_sub(a, b), v3_sub(c, b));
                V3 nf;
                if (p->source_zero_faces && !cross.x && !cross.y && !cross.z) {

                    V3 oa = obj_to_game(OBJ_POS(&obj, fv[0].pos));
                    V3 ob = obj_to_game(OBJ_POS(&obj, fv[1].pos));
                    V3 oc = obj_to_game(OBJ_POS(&obj, fv[2].pos));
                    V3 original = v3_cross(v3_sub(oa, ob), v3_sub(oc, ob));
                    if (!p->is_figure || p->point_shift || original.x || original.y || original.z)
                        die("%s: a face became zero-area only after rounding", obj_path);
                    nf = (V3){ 0, 0, 0 };
                    source_zero_faces++;
                } else {
                    nf = v3_normalize(cross);
                }
                float lit = 0.5f + v3_dot(nf, v3_neg(sun)) * 0.5f + p->shading_base;
                if (p->light_up) {
                    float centre[3] = { (a.x + b.x + c.x) / 3, (a.y + b.y + c.y) / 3, (a.z + b.z + c.z) / 3 };
                    float up[3];
                    p->light_up(p->light_ctx, centre, up);
                    float facing = nf.x * up[0] + nf.y * up[1] + nf.z * up[2];
                    lit = 0.62f + 0.38f * facing + 0.12f * v3_dot(nf, v3_neg(sun)) + p->shading_base;
                }
                if (ma.has_light) lit = ma.light;
                float factor = fminf(fmaxf(p->shading, 0.0f), 1.0f);
                float intensity = fmaf(factor, lit - 1.0f, 1.0f);
                int16_t normal[3] = { float_to_i16_saturated(nf.x * 256.0f),
                    float_to_i16_saturated(nf.y * 256.0f), float_to_i16_saturated(nf.z * 256.0f) };
                if (!normal[0] && !normal[1] && !normal[2]) {

                    if (p->point_shift > 0) continue;
                    if (!p->source_zero_faces)
                        die("%s: degenerate triangle with material '%s'", obj_path, mat);
                }
                int16_t flipped[3] = { (int16_t)-normal[0], (int16_t)-normal[1], (int16_t)-normal[2] };

                for (int t = 0; t + 2 < face->count; t++) {
                    const TexEntry *texture = NULL;
                    if (!ma.invisible) texture = texture_cache_get_lit(&tc, mat, intensity);
                    ModelVertex v[3];
                    const ObjVertex *tv[3] = { &fv[0], &fv[t + 1], &fv[t + 2] };
                    float face_uv[3][2];
                    for (int i = 0; i < 3; i++) {
                        static const float no_uv[3] = { 0, 0, 0 };
                        const float *uv = tv[i]->uv >= 0 ? OBJ_UV(&obj, tv[i]->uv) : no_uv;
                        v[i].point = (uint16_t)tv[i]->pos;
                        face_uv[i][0] = uv[0]; face_uv[i][1] = uv[1];
                    }
                    model_triangle_uvs(v, face_uv);
                    int ledges = 0;
                    if (normal[2] > 128) {
                        for (int i = 0; i < 3; i++)
                            if (is_ledge(&edges, v[i].point, v[(i + 1) % 3].point)) ledges |= 1 << i;
                    }
                    Vector *dst = ma.invisible ? &invisible : &m.triangles;
                    Vector *codes = ma.invisible ? &invisible_codes : &m.source_codes;
                    uint8_t cls = p->occluder && !ma.invisible ?
                        occl_class(&occl, model_point_as_float(&m.points, tv[0]->pos, p->point_shift),
                                   model_point_as_float(&m.points, tv[1]->pos, p->point_shift),
                                   model_point_as_float(&m.points, tv[2]->pos, p->point_shift)) : 0;
                    append_triangle(dst, codes, v, normal, texture, &ma, uv_scroll_speed, ledges);
                    VEC_AT(*dst, BuildTriangle, dst->len - 1).occl = cls;
                    occl_counts[cls]++;
                    if ((p->is_figure && texture && texture->alpha) || ma.double_sided) {
                        ModelVertex w[3] = { v[0], v[2], v[1] };
                        int back = (ledges >> 2 & 1) | (ledges & 2) | (ledges & 1) << 2;
                        append_triangle(dst, codes, w, flipped, texture, &ma, uv_scroll_speed, back);
                        VEC_AT(*dst, BuildTriangle, dst->len - 1).occl = cls;
                    }
                }
            }
        }

        int end = (int)m.triangles.len;
        if (part) {
            for (int t = first; t < end; t++) VEC_AT(m.triangles, BuildTriangle, t).pad = (uint8_t)part;
            for (size_t i = 0; i < invisible.len; i++)
                VEC_AT(invisible, BuildTriangle, i).pad = (uint8_t)part;
        }
        if (end > first) {
            Bvh b = { &m, first, xcalloc((size_t)(end - first), sizeof(uint16_t)), 0 };
            int *group = xmalloc(sizeof(int) * (end - first));
            for (int t = first; t < end; t++) group[t - first] = t;
            build_bvh_node(&b, group, end - first);
            free(group);
            if (m.leaf_triangle_offsets.len > MAX_LEAVES || m.chunks.len >= 0xFFFF)
                die("%s: too many BVH nodes", obj_path);

            size_t list_start[6];
            *(uint16_t *)vec_push(&m.rank_segments) = (uint16_t)axis_lists[0].len;
            for (int k = 0; k < 6; k++) {
                list_start[k] = axis_lists[k].len;
                append_axis_draw_order(&m, &axis_lists[k], first, end, k / 2, k & 1 ? -1 : 1);
            }
            vec_reserve(&m.tri_rank, (size_t)end);
            while (m.tri_rank.len < (size_t)end) vec_push(&m.tri_rank);
            for (int k = 0; k < 6; k++)
                for (int i = 0; i < end - first; i++) {
                    size_t pos = list_start[k] + i;
                    if (pos > 0xFFFF) die("%s: too many triangles", obj_path);
                    VEC_AT(m.tri_rank, Rank6, VEC_AT(axis_lists[k], AxisEntry, pos).tri)[k] = (uint16_t)pos;
                }
            free(b.tri_leaf);
        }
        if (part) {
            int *pl = &VEC_AT(m.part_leaves, IntPair, part - 1)[0];
            pl[1] = (int)m.leaf_triangle_offsets.len - leaf_first;
            pl[0] = pl[1] ? leaf_first : -1;
        }
        for (size_t i = 0; i < invisible.len; i++) {
            *(BuildTriangle *)vec_push(&m.triangles) = VEC_AT(invisible, BuildTriangle, i);
            *(uint16_t *)vec_push(&m.source_codes) = VEC_AT(invisible_codes, uint16_t, i);
        }
        vec_free(&invisible);
        vec_free(&invisible_codes);
    }
    if (p->occluder)
        printf("  occluder: %d faces hidden from above the shell, %d from below\n", occl_counts[1],
               occl_counts[2]);
    while (m.tri_rank.len < m.triangles.len) vec_push(&m.tri_rank);
    *(uint16_t *)vec_push(&m.leaf_triangle_offsets) = (uint16_t)m.leaf_triangles.len;
    for (int k = 0; k < 6; k++) {
        for (size_t i = 0; i < axis_lists[k].len; i++)
            *(AxisEntry *)vec_push(&m.tri_axes) = VEC_AT(axis_lists[k], AxisEntry, i);
        vec_free(&axis_lists[k]);
    }

    if (p->is_figure) {
        for (int k = 0; k < 6; k++) {
            FigOrder order = { &obj, &m, k / 2, k & 1 ? -1 : 1 };
            for (size_t t = 0; t < m.triangles.len; t++) *(uint16_t *)vec_push(&m.draw_axes[k]) = (uint16_t)t;
            stable_sort(m.draw_axes[k].data, m.triangles.len, sizeof(uint16_t), compare_figure_depths,
                        &order);
        }
    }

    for (size_t i = 0; i < tc.images.len; i++) image_free(&VEC_AT(tc.images, ImgEntry, i).img);
    free(edges.slots);
    if (source_zero_faces != p->source_zero_faces)
        die("%s: expected %d zero-area source faces, found %d", obj_path, p->source_zero_faces,
            source_zero_faces);
    free(obj_path);
    free(mtl_path);
    if (m.source_codes.len != m.triangles.len) die("triangle and collision code counts differ");
    return m;
}
