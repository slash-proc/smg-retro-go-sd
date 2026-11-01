#include "../../src/assets/asset_format.h"
#include "../../src/graphics/visibility_indexed.h"
#include "common.h"
#include "lookup_tables.h"
#include "model_builder.h"
#include "palette.h"
#include "png_reader.h"
#include "pose_packing.h"
#include "sound_codec.h"
#include "stage_extras.h"

#include <ctype.h>
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

#ifdef SMG_ASSETC_PROGRESS
extern void smg_assetc_progress(uint32_t phase, uint32_t index, uint32_t total);
#define ASSETC_PROGRESS(phase, index, total) smg_assetc_progress(phase, index, total)
#else
#define ASSETC_PROGRESS(phase, index, total) ((void)0)
#endif

static const char *g_in, *g_out;
static Rgba g_pal[256];
static uint16_t g_pal15[256];
static ByteBuffer g_asm, g_hdr;
static const char *g_asm_section = ".rodata.assets";

static void write_asset_blob(const char *symbol, const char *decl, const void *data, size_t len) {
    write_file(format_string("%s/%s.bin", g_out, symbol), data, len);
    char *s = format_string("\n    .section %s, \"a\"\n    .global %s\n    .balign 4\n%s:\n    .incbin \"%s.bin\"\n",
                            g_asm_section, symbol, symbol, symbol);
    buffer_append(&g_asm, s, strlen(s));
    s = format_string("extern const %s;\n", decl);
    buffer_append(&g_hdr, s, strlen(s));
}

static void append_header(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
static void append_header(const char *fmt, ...) {
    char line[512];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(line, sizeof line, fmt, ap);
    va_end(ap);
    buffer_append(&g_hdr, line, strlen(line));
}

static uint16_t pack_rgb15(int r, int g, int b) { return (uint16_t)(r | g << 5 | b << 10); }

typedef struct {
    char *name;
    char *far;
    int initial, underwater;
    float water_plane;
    int has_water, has_region;
    float water, region[4];
    float death_height;
    int music;
    float shading;
    int has_occluder;
    float occluder[3];
    TextureUnderlay texture_underlays[16];
    int texture_underlay_count;
    TextureAlphaPolicy texture_alpha_policies[16];
    int texture_alpha_policy_count;
} StageCfg;

typedef struct {
    char *name, **frames;
    int n_frames;
    int ticks_per_key, loop;
} AnimCfg;

#define MAX_ANIM_CFGS 256

typedef struct {
    char *name, *obj;
    float shading_base, shading;
    AnimCfg anims[MAX_ANIM_CFGS];
    int n_anims;
    int static_texture_dedup;
    int source_zero_faces;
    TextureAlphaPolicy texture_alpha_policies[16];
    int texture_alpha_policy_count;
} FigureCfg;

typedef struct {
    char *name, *file;
    long loop_start;
    int is_music;
} SoundCfg;

#define MAX_STAGE_CFGS 32
static StageCfg g_stages[MAX_STAGE_CFGS];
#define MAX_FIGURE_CFGS 512
static FigureCfg g_figures[MAX_FIGURE_CFGS];

#define MAX_MUSIC_CFGS 127
#define MAX_EFFECT_CFGS 1023
#define MAX_SOUND_CFGS (MAX_MUSIC_CFGS + MAX_EFFECT_CFGS)
static SoundCfg g_sounds[MAX_SOUND_CFGS];
static int g_n_music, g_n_effects;
#define MAX_BLOB_CFGS 256
static char *g_blob_names[MAX_BLOB_CFGS], *g_blob_files[MAX_BLOB_CFGS];
static int g_n_blobs;
static int g_n_stages, g_n_figures, g_n_sounds;

static void read_floats(char **t, int n, float *out, int line) {
    for (int i = 0; i < n; i++) {
        char *end;
        if (!t[i]) die("assets.cfg:%d: expected %d numbers", line, n);
        out[i] = strtof(t[i], &end);
        if (*end) die("assets.cfg:%d: bad number '%s'", line, t[i]);
    }
}

static void parse_manifest(void) {
    char *path = format_string("%s/assets.cfg", g_in);
    char *text = (char *)read_file(path, NULL);
    if (!text) die("cannot open %s", path);
    StageCfg *st = NULL;
    FigureCfg *fig = NULL;
    int line_num = 0;
    for (char *line = text; line && *line;) {
        char *next = strchr(line, '\n');
        if (next) *next++ = 0;
        line_num++;
        char *hash = strchr(line, '#');
        if (hash) *hash = 0;
        char *t[160] = { 0 };
        int n = 0;
        for (char *tok = strtok(line, " \t\r"); tok && n < 159; tok = strtok(NULL, " \t\r")) t[n++] = tok;
        line = next;
        if (!n) continue;

        if (!strcmp(t[0], "stage") && n == 2) {
            if (g_n_stages >= MAX_STAGE_CFGS) die("assets.cfg:%d: too many stages", line_num);
            st = &g_stages[g_n_stages++];
            st->name = xstrdup(t[1]);
            st->shading = 0.7f;
        } else if (!strcmp(t[0], "figure") && n == 5) {
            if (g_n_figures == MAX_FIGURE_CFGS) die("assets.cfg:%d: too many figures", line_num);
            fig = &g_figures[g_n_figures++];
            fig->name = xstrdup(t[1]);
            fig->obj = xstrdup(t[2]);
            read_floats(t + 3, 1, &fig->shading_base, line_num);
            read_floats(t + 4, 1, &fig->shading, line_num);
        } else if ((!strcmp(t[0], "music") && n == 4) || (!strcmp(t[0], "sound") && n == 3)) {
            int is_music = t[0][0] == 'm';
            if (g_n_sounds >= MAX_SOUND_CFGS ||
               (is_music ? g_n_music >= MAX_MUSIC_CFGS : g_n_effects >= MAX_EFFECT_CFGS))
                die("assets.cfg:%d: too many %s", line_num, is_music ? "music tracks" : "sound effects");
            if (is_music) g_n_music++;
            else g_n_effects++;
            SoundCfg *snd = &g_sounds[g_n_sounds++];
            snd->name = xstrdup(t[1]);
            snd->file = xstrdup(t[2]);
            snd->is_music = is_music;
            snd->loop_start = snd->is_music ? strtol(t[3], NULL, 10) : 0;
        } else if (!strcmp(t[0], "blob") && n == 3) {
            if (g_n_blobs >= MAX_BLOB_CFGS) die("assets.cfg:%d: too many blobs", line_num);
            g_blob_names[g_n_blobs] = xstrdup(t[1]);
            g_blob_files[g_n_blobs++] = xstrdup(t[2]);
        } else if (!strcmp(t[0], "end")) {
            st = NULL;
            fig = NULL;
        } else if (st && !strcmp(t[0], "far") && n == 2) {
            st->far = xstrdup(t[1]);
        } else if (st && !strcmp(t[0], "texture_underlay") && n == 3) {
            if (st->texture_underlay_count == 16)
                die("assets.cfg:%d: too many texture underlays", line_num);
            for (int i = 0; i < st->texture_underlay_count; i++)
                if (!strcmp(st->texture_underlays[i].foreground, t[1]))
                    die("assets.cfg:%d: duplicate texture underlay", line_num);
            st->texture_underlays[st->texture_underlay_count++] =
                (TextureUnderlay){ xstrdup(t[1]), xstrdup(t[2]) };
        } else if ((st || fig) && !strcmp(t[0], "texture_alpha") && n == 3) {
            TextureAlphaPolicy *policies = st ? st->texture_alpha_policies : fig->texture_alpha_policies;
            int *count = st ? &st->texture_alpha_policy_count : &fig->texture_alpha_policy_count;
            if (*count == 16)
                die("assets.cfg:%d: too many texture alpha policies", line_num);
            for (int i = 0; i < *count; i++)
                if (!strcmp(policies[i].texture, t[1]))
                    die("assets.cfg:%d: duplicate texture alpha policy", line_num);
            int mode;
            if (!strcmp(t[2], "opaque")) mode = TEXTURE_ALPHA_OPAQUE;
            else if (!strcmp(t[2], "coverage")) mode = TEXTURE_ALPHA_COVERAGE;
            else if (!strcmp(t[2], "blend25") && st) mode = TEXTURE_ALPHA_BLEND_QUARTER;
            else die("assets.cfg:%d: unknown texture alpha policy '%s'", line_num, t[2]);
            policies[(*count)++] =
                (TextureAlphaPolicy){ xstrdup(t[1]), mode };
        } else if (st && !strcmp(t[0], "initial")) {
            st->initial = 1;
        } else if (st && !strcmp(t[0], "underwater")) {
            st->underwater = 1;
        } else if (st && !strcmp(t[0], "water_plane")) {
            read_floats(t + 1, 1, &st->water_plane, line_num);
        } else if (st && !strcmp(t[0], "water")) {
            st->has_water = 1;
            read_floats(t + 1, 1, &st->water, line_num);
        } else if (st && !strcmp(t[0], "water_region")) {
            st->has_region = 1;
            read_floats(t + 1, 4, st->region, line_num);
        } else if (st && !strcmp(t[0], "death_height")) {
            read_floats(t + 1, 1, &st->death_height, line_num);
        } else if (st && !strcmp(t[0], "occluder")) {
            st->has_occluder = 1;
            read_floats(t + 1, 3, st->occluder, line_num);
        } else if (st && !strcmp(t[0], "shading")) {
            read_floats(t + 1, 1, &st->shading, line_num);
        } else if (st && !strcmp(t[0], "stage_music") && n == 2) {
            st->music = -1;
            for (int i = 0; i < g_n_sounds; i++)
                if (g_sounds[i].is_music && !strcmp(g_sounds[i].name, t[1])) {
                    int k = 0;
                    for (int j = 0; j < i; j++) k += g_sounds[j].is_music;
                    st->music = k;
                }
            if (st->music < 0 && strcmp(t[1], "none"))
                die("assets.cfg:%d: unknown music '%s' (declare music first)", line_num, t[1]);
        } else if (!st && !fig && !strcmp(t[0], "object_types")) {
            for (int i = 1; i < n; i++) object_types_add(t[i]);
        } else if (fig && !strcmp(t[0], "static_texture_dedup") && n == 1) {
            fig->static_texture_dedup = 1;
        } else if (fig && !strcmp(t[0], "source_zero_faces") && n == 2) {
            char *end;
            long count = strtol(t[1], &end, 10);
            if (*end || count < 1 || count > 65535 || fig->source_zero_faces)
                die("assets.cfg:%d: invalid source_zero_faces", line_num);
            fig->source_zero_faces = (int)count;
        } else if (fig && !strcmp(t[0], "anim") && n >= 3) {

            if (fig->n_anims >= MAX_ANIM_CFGS) die("assets.cfg:%d: too many animations", line_num);
            AnimCfg *a = &fig->anims[fig->n_anims++];
            a->name = xstrdup(t[1]);
            int first = 2;
            a->ticks_per_key = 6;
            a->loop = 1;
            int timed = n >= 5 && isdigit((unsigned char)t[2][0]);
            if (timed && (!strcmp(t[3], "loop") || !strcmp(t[3], "once"))) {
                a->ticks_per_key = atoi(t[2]);
                a->loop = !strcmp(t[3], "loop");
                first = 4;
            }
            a->n_frames = n - first;
            a->frames = xmalloc(sizeof(char *) * a->n_frames);
            for (int i = 0; i < a->n_frames; i++) a->frames[i] = xstrdup(t[i + first]);
        } else {
            die("assets.cfg:%d: unexpected '%s'", line_num, t[0]);
        }
    }
    free(text);
    free(path);
}

static void build_palette(void) {
    size_t len;
    char *path = format_string("%s/palette.rgba", g_in);
    uint8_t *raw = read_file(path, &len);
    if (!raw || len != 1024) die("%s: expected 256 RGBA colours", path);
    memcpy(g_pal, raw, 1024);

    uint16_t pal[256];
    for (int i = 0; i < 256; i++) {
        int c[3];
        const uint8_t *rgb = &g_pal[i].r;
        for (int k = 0; k < 3; k++) {
            float v = (float)rgb[k] / 256.0f * 32.0f + 0.5f;
            c[k] = (int)roundf(fminf(fmaxf(v, 0.0f), 31.0f));
        }
        pal[i] = pack_rgb15(c[0], c[1], c[2]);
    }
    memcpy(g_pal15, pal, sizeof pal);
    write_asset_blob("game_palette", "uint16_t game_palette[256]", pal, sizeof pal);

    uint8_t darken[256];
    for (int i = 0; i < 256; i++) {
        float col[3] = { (float)g_pal[i].r * 0.5f, (float)g_pal[i].g * 0.5f, (float)g_pal[i].b * 0.5f };
        darken[i] = (uint8_t)palette_nearest_opaque(g_pal, col);
    }
    write_asset_blob("shade_darken", "uint8_t shade_darken[256]", darken, sizeof darken);
    uint8_t *quarter = xmalloc(65536);
    palette_build_quarter_blend(quarter, g_pal);
    write_asset_blob("shade_quarter", "uint8_t shade_quarter[65536]", quarter, 65536);
    free(quarter);

    char *shadow_path = format_string("%s/shadow.png", g_in);
    Texture shadow = texture_from_image(shadow_path, 1, g_pal);
    write_asset_blob("shadow_texture", "Texture shadow_texture", &shadow, sizeof shadow);
    free(raw);
}

static int32_t square_to_fixed(float x) {
    int32_t r = float_to_fixed(x, 8);
    return (int32_t)((uint32_t)r * (uint32_t)r) >> 8;
}

typedef struct {
    int32_t centre[3];
    int32_t bound;
} GridItem;

static uint32_t append_bound_grid(ByteBuffer *b, const GridItem *g, int n, const char *what) {
    int64_t lo[3] = { INT64_MAX, INT64_MAX, INT64_MAX }, hi[3] = { INT64_MIN, INT64_MIN, INT64_MIN };
    for (int i = 0; i < n; i++) {
        if (g[i].bound < 0) continue;
        for (int k = 0; k < 3; k++) {
            int64_t a = (int64_t)g[i].centre[k] - g[i].bound, c = (int64_t)g[i].centre[k] + g[i].bound;
            if (a < lo[k]) lo[k] = a;
            if (c > hi[k]) hi[k] = c;
        }
    }
    if (lo[0] > hi[0]) return 0;
    int shift = 8, dims[3];
    for (;; shift++) {
        int64_t cells = 1;
        for (int k = 0; k < 3; k++) {
            dims[k] = (int)(((hi[k] - lo[k]) >> shift) + 1);
            cells *= dims[k];
        }
        if (cells <= 131072) break;
    }
    Vector lists = VEC(uint16_t), cells = VEC(uint16_t);

    uint16_t outside = 0;
    {
        size_t at = lists.len;
        *(uint16_t *)vec_push(&lists) = 0;
        for (int i = 0; i < n; i++)
            if (g[i].bound < 0) {
                *(uint16_t *)vec_push(&lists) = (uint16_t)i;
                VEC_AT(lists, uint16_t, at)++;
            }
        outside = (uint16_t)at;
    }
    uint16_t *tmp = xmalloc(sizeof(uint16_t) * (size_t)(n + 1));
    long total = 0, longest = 0, count_cells = (long)dims[0] * dims[1] * dims[2];
    for (int z = 0; z < dims[2]; z++)
        for (int y = 0; y < dims[1]; y++)
            for (int x = 0; x < dims[0]; x++) {
                int c[3] = { x, y, z }, m = 0;
                for (int i = 0; i < n; i++) {
                    int in = 1;
                    if (g[i].bound >= 0)
                        for (int k = 0; k < 3 && in; k++) {
                            int64_t cl = lo[k] + ((int64_t)c[k] << shift);
                            int64_t ch = cl + ((int64_t)1 << shift) - 1;
                            int64_t vl = (int64_t)g[i].centre[k] - g[i].bound;
                            int64_t vh = (int64_t)g[i].centre[k] + g[i].bound;
                            if (vh < cl || vl > ch) in = 0;
                        }
                    if (in) tmp[m++] = (uint16_t)i;
                }
                total += m;
                if (m > longest) longest = m;

                size_t found = (size_t)-1;
                for (size_t at = 0; at < lists.len; at += 1 + VEC_AT(lists, uint16_t, at)) {
                    if (VEC_AT(lists, uint16_t, at) != m) continue;
                    if (!memcmp(&VEC_AT(lists, uint16_t, at + 1), tmp, sizeof(uint16_t) * (size_t)m)) {
                        found = at;
                        break;
                    }
                }
                if (found == (size_t)-1) {
                    found = lists.len;
                    *(uint16_t *)vec_push(&lists) = (uint16_t)m;
                    for (int i = 0; i < m; i++) *(uint16_t *)vec_push(&lists) = tmp[i];
                }
                if (found > 0xFFFF) die("%s grid lists too long", what);
                *(uint16_t *)vec_push(&cells) = (uint16_t)found;
            }
    free(tmp);

    int best_bs = 1;
    size_t best_words = (size_t)-1;
    Vector best_top = VEC(uint16_t), best_blocks = VEC(uint16_t);
    for (int bs = 1; bs <= 3; bs++) {
        int bd[3], side = 1 << bs, bsize = side * side * side;
        for (int k = 0; k < 3; k++) bd[k] = (dims[k] + side - 1) >> bs;
        size_t top_count = (size_t)bd[0] * bd[1] * bd[2];
        Vector top = VEC(uint16_t), blocks = VEC(uint16_t);
        size_t hash_size = 1;
        while (hash_size < top_count * 2) hash_size <<= 1;
        int32_t *hash = xmalloc(sizeof(int32_t) * hash_size);
        for (size_t i = 0; i < hash_size; i++) hash[i] = -1;
        uint16_t *blk = xmalloc(sizeof(uint16_t) * (size_t)bsize);
        for (int bz = 0; bz < bd[2]; bz++)
            for (int by = 0; by < bd[1]; by++)
                for (int bx = 0; bx < bd[0]; bx++) {
                    for (int i = 0; i < bsize; i++) {
                        int x = bx * side + (i & (side - 1)), y = by * side + ((i >> bs) & (side - 1)),
                            z = bz * side + (i >> (2 * bs));
                        size_t cell = ((size_t)z * dims[1] + y) * dims[0] + x;
                        int inside = x < dims[0] && y < dims[1] && z < dims[2];
                        blk[i] = inside ? VEC_AT(cells, uint16_t, cell) : outside;
                    }
                    uint32_t hv = 2166136261u;
                    for (int i = 0; i < bsize; i++) hv = (hv ^ blk[i]) * 16777619u;
                    size_t slot = hv & (hash_size - 1);
                    int found = -1;
                    for (; hash[slot] >= 0; slot = (slot + 1) & (hash_size - 1)) {
                        const uint16_t *block = &VEC_AT(blocks, uint16_t, (size_t)hash[slot] * bsize);
                        if (!memcmp(block, blk, sizeof(uint16_t) * bsize)) {
                            found = hash[slot];
                            break;
                        }
                    }
                    if (found < 0) {
                        found = (int)(blocks.len / (size_t)bsize);
                        hash[slot] = found;
                        for (int i = 0; i < bsize; i++) *(uint16_t *)vec_push(&blocks) = blk[i];
                    }
                    if (found > 0xFFFF) die("%s grid: too many blocks", what);
                    *(uint16_t *)vec_push(&top) = (uint16_t)found;
                }
        free(hash);
        free(blk);
        size_t words = top.len + blocks.len;
        if (words < best_words && top.len <= 0xFFFF) {
            vec_free(&best_top);
            vec_free(&best_blocks);
            best_words = words;
            best_bs = bs;
            best_top = top;
            best_blocks = blocks;
        } else {
            vec_free(&top);
            vec_free(&blocks);
        }
    }
    if (best_words == (size_t)-1) die("%s grid: top level too large", what);
    GravityGrid gg = { 0 };
    for (int k = 0; k < 3; k++) {
        gg.origin[k] = (int32_t)lo[k];
        gg.dims[k] = (uint16_t)dims[k];
    }
    gg.shift = (uint8_t)shift;
    gg.block_shift = (uint8_t)best_bs;
    gg.top_count = (uint16_t)best_top.len;
    gg.outside = outside;
    Vector packed = VEC(uint16_t);
    for (size_t i = 0; i < best_top.len; i++) *(uint16_t *)vec_push(&packed) = VEC_AT(best_top, uint16_t, i);
    for (size_t i = 0; i < best_blocks.len; i++)
        *(uint16_t *)vec_push(&packed) = VEC_AT(best_blocks, uint16_t, i);
    uint32_t at = buffer_append_aligned(b, &gg, sizeof gg);
    uint32_t cells_offset = buffer_append_aligned(b, packed.data, packed.len * 2);
    uint32_t entries = buffer_append_aligned(b, lists.data, lists.len * 2);
    GravityGrid *grid = (GravityGrid *)(b->data + at);
    grid->cells = cells_offset;
    grid->lists = entries;
    printf("  %s grid: %dx%dx%d cells of %d, %.1f per cell (longest %ld of %d), %zu list entries, "
           "blocks of %d^3: %zu -> %zu bytes\n", what, dims[0], dims[1], dims[2], 1 << shift,
           (double)total / (double)count_cells, longest, n, lists.len, 1 << best_bs, cells.len * 2,
           packed.len * 2);
    vec_free(&packed);
    vec_free(&best_top);
    vec_free(&best_blocks);
    vec_free(&cells);
    vec_free(&lists);
    return at;
}

static size_t g_max_points, g_max_far_points, g_max_objects, g_max_textures, g_max_leaves, g_max_home_leaves;
static size_t g_max_figure_points = FIGURE_PROCEDURAL_MAX_POINTS;
static void figure_screen_bound_include(size_t count, const char *name) {
    if (count > FIGURE_TRANSFORM_MAX_POINTS)
        die("%s: %zu points, at most %d fit the figure transform", name, count, FIGURE_TRANSFORM_MAX_POINTS);
    if (count > g_max_figure_points) g_max_figure_points = count;
}

typedef struct {
    Vector home;
    Vector first;
    int slot_max;
} HomeTables;

static const BuildTriangle *leaf_triangle(const Model *m, size_t j) {
    return &VEC_AT(m->triangles, BuildTriangle, VEC_AT(m->leaf_triangles, uint16_t, j));
}

static HomeTables number_points_by_leaf(Model *m) {
    HomeTables ht = { VEC(uint16_t), VEC(uint16_t), 0 };
    size_t n = m->points.len, leaves = m->leaf_triangle_offsets.len - 1;
    int *new_index = xmalloc(sizeof(int) * n);
    for (size_t i = 0; i < n; i++) new_index[i] = -1;
    Vector order = VEC(int);
    for (size_t l = 0; l < leaves; l++) {
        *(uint16_t *)vec_push(&ht.first) = (uint16_t)order.len;
        uint16_t a = VEC_AT(m->leaf_triangle_offsets, uint16_t, l);
        uint16_t b = VEC_AT(m->leaf_triangle_offsets, uint16_t, l + 1);
        for (uint16_t j = a; j < b; j++) {
            const BuildTriangle *tri = leaf_triangle(m, j);
            for (int v = 0; v < 3; v++) {
                int p = tri->v[v].point;
                if (new_index[p] >= 0) continue;
                new_index[p] = (int)order.len;
                *(int *)vec_push(&order) = p;
                *(uint16_t *)vec_push(&ht.home) = (uint16_t)l;
            }
        }
    }
    *(uint16_t *)vec_push(&ht.first) = (uint16_t)order.len;
    for (size_t i = 0; i < n; i++)
        if (new_index[i] < 0) {
            new_index[i] = (int)order.len;
            *(int *)vec_push(&order) = (int)i;
            *(uint16_t *)vec_push(&ht.home) = 0xFFFF;
        }
    Vector points = VEC(Point3);
    for (size_t i = 0; i < n; i++)
        memcpy(vec_push(&points), VEC_AT(m->points, Point3, VEC_AT(order, int, i)), sizeof(Point3));
    vec_free(&m->points);
    m->points = points;
    for (size_t t = 0; t < m->triangles.len; t++)
        for (int v = 0; v < 3; v++) {
            ModelVertex *mv = &VEC_AT(m->triangles, BuildTriangle, t).v[v];
            mv->point = (uint16_t)new_index[mv->point];
        }

    uint8_t *seen = xcalloc(leaves + 1, 1);
    int *list = xmalloc(sizeof(int) * 64 * 3);
    for (size_t l = 0; l < leaves; l++) {
        int n_list = 0, slots = 0;
        uint16_t a = VEC_AT(m->leaf_triangle_offsets, uint16_t, l);
        uint16_t b = VEC_AT(m->leaf_triangle_offsets, uint16_t, l + 1);
        for (uint16_t j = a; j < b; j++) {
            const BuildTriangle *tri = leaf_triangle(m, j);
            for (int v = 0; v < 3; v++) {
                uint16_t h = VEC_AT(ht.home, uint16_t, tri->v[v].point);
                if (seen[h]) continue;
                seen[h] = 1;
                list[n_list++] = h;
                slots += VEC_AT(ht.first, uint16_t, h + 1) - VEC_AT(ht.first, uint16_t, h);
            }
        }
        for (int i = 0; i < n_list; i++) seen[list[i]] = 0;
        if (slots > ht.slot_max) ht.slot_max = slots;
    }
    free(seen);
    free(list);
    free(new_index);
    vec_free(&order);
    return ht;
}

static int leaves_fit_indexed_visibility(const Model *m) {
    for (size_t leaf = 0; leaf + 1 < m->leaf_triangle_offsets.len; leaf++) {
        const uint16_t *offsets = &VEC_AT(m->leaf_triangle_offsets, uint16_t, leaf);
        if ((unsigned)(offsets[1] - offsets[0]) > VISIBILITY_LEAF_MAX) return 0;
    }
    return 1;
}

static uint8_t far_palette_nearest(int r, int g, int b) {
    int best = 1, best_d = 1 << 30;
    for (int i = 1; i < 256; i++) {
        uint16_t c = g_pal15[i];
        int dr = (c & 31) - r, dg = (c >> 5 & 31) - g, db = (c >> 10 & 31) - b;
        int d = dr * dr * 3 + dg * dg * 4 + db * db * 2;
        if (d < best_d) {
            best_d = d;
            best = i;
        }
    }
    return (uint8_t)best;
}

static uint8_t far_texture_average(const Texture *t) {
    int r = 0, g = 0, b = 0, count = 0;
    for (int i = 0; i < TEX_TEXELS; i += 3) {
        uint8_t idx = t->mip[0][i];
        if (!idx) continue;
        uint16_t c = g_pal15[idx];
        r += c & 31;
        g += c >> 5 & 31;
        b += c >> 10 & 31;
        count++;
    }
    return count ? far_palette_nearest(r / count, g / count, b / count) : 0;
}

static void append_far_textures(ByteBuffer *b, StageHeader *h, const Model *m) {
    size_t n = m->textures.len;
    uint8_t *raw = xcalloc(n, 1);
    for (size_t i = 0; i < m->triangles.len; i++) {
        const BuildTriangle *t = &VEC_AT(m->triangles, BuildTriangle, i);
        if (t->flags & TRI_ALPHA) {
            raw[t->texture] = 1;
            raw[t->water_texture] = 1;
        }
    }
    h->textures = buffer_append_aligned(b, NULL, n * 4);
    size_t stored = n * 4, packed = 0;
    for (size_t t = 0; t < n; t++) {
        const Texture *texture = &VEC_AT(m->textures, Texture, t);
        uint8_t average = raw[t] ? 0 : far_texture_average(texture);
        uint32_t record;
        if (average) {
            uint32_t detail = buffer_append_aligned(b, texture, sizeof *texture);
            if ((detail & 3) || detail >= (1u << 24)) die("far texture detail offset exceeds packed record");
            record = FAR_TEXTURE_AVERAGE | FAR_TEXTURE_DETAIL | (detail << 6) | average;
            stored += sizeof *texture;
            packed++;
        } else {
            record = buffer_append_aligned(b, texture, sizeof *texture);
            stored += sizeof *texture;
        }
        ((uint32_t *)(void *)(b->data + h->textures))[t] = record;
    }
    free(raw);
    h->surface_features |= STAGE_PACKED_FAR_TEXTURES;
    printf("  far textures: %zu of %zu retain average plus detail, %zu -> %zu bytes\n", packed, n, n * sizeof(Texture),
           stored);
}

typedef struct {
    int base, water, scroll;
    int16_t speed[2];
} TextureSlot;

static void split_texture_slots(Model *m, const char *name) {
    Vector slots = VEC(TextureSlot);
    size_t original = m->textures.len;
    for (size_t t = 0; t < original; t++)
        *(TextureSlot *)vec_push(&slots) = (TextureSlot){ (int)t, -1, 0, { 0, 0 } };

    uint8_t *drawn = xcalloc(m->triangles.len + 1, 1);
    for (size_t j = 0; j < m->leaf_triangles.len; j++) drawn[VEC_AT(m->leaf_triangles, uint16_t, j)] = 1;
    int added = 0;
    for (size_t i = 0; i < m->triangles.len; i++) {
        BuildTriangle *t = &VEC_AT(m->triangles, BuildTriangle, i);
        if (!drawn[i]) continue;
        int anim = (t->flags & SURF_ANIM_UV) != 0;
        size_t k;
        for (k = t->texture; k < slots.len; k = k == t->texture ? original : k + 1) {
            TextureSlot *s = &VEC_AT(slots, TextureSlot, k);
            if (s->base != t->texture) continue;
            if (s->water >= 0 && s->water != t->water_texture) continue;
            int other_speed = s->speed[0] != t->uv_scroll_speed[0] || s->speed[1] != t->uv_scroll_speed[1];
            if (anim && s->scroll && other_speed) continue;
            break;
        }
        if (k >= slots.len) {
            k = m->textures.len;
            Texture copy = VEC_AT(m->textures, Texture, t->texture);
            *(Texture *)vec_push(&m->textures) = copy;
            *(TextureSlot *)vec_push(&slots) = (TextureSlot){ t->texture, -1, 0, { 0, 0 } };
            added++;
        }
        TextureSlot *s = &VEC_AT(slots, TextureSlot, k);
        s->water = t->water_texture;
        if (anim && !s->scroll) {
            s->scroll = 1;
            s->speed[0] = t->uv_scroll_speed[0];
            s->speed[1] = t->uv_scroll_speed[1];
        }
        t->texture = (uint16_t)k;
    }
    if (added)
        printf("  %s: %d texture numbers added (shared texels, other water texture or UV scroll)\n", name,
               added);
    free(drawn);
    vec_free(&slots);
}

static void append_stage_triangles(ByteBuffer *b, StageHeader *h, const Model *m) {
    size_t n = m->triangles.len, nt = m->textures.len;
    StageTriangle *rom = xcalloc(n + 1, sizeof *rom);
    uint16_t *water = xcalloc(nt + 1, sizeof *water);
    int16_t (*scroll)[2] = xcalloc(nt + 1, sizeof *scroll);
    uint8_t *drawn = xcalloc(n + 1, 1);
    for (size_t j = 0; j < m->leaf_triangles.len; j++) drawn[VEC_AT(m->leaf_triangles, uint16_t, j)] = 1;
    int any_water = 0, any_scroll = 0;
    for (size_t t = 0; t < nt; t++) water[t] = (uint16_t)t;
    for (size_t i = 0; i < n; i++) {
        const BuildTriangle *t = &VEC_AT(m->triangles, BuildTriangle, i);
        memcpy(rom[i].v, t->v, sizeof rom[i].v);

        for (int k = 0; k < 3; k++) {
            int q = (int)lround(t->normal[k] * 127.0 / 256.0);
            rom[i].normal[k] = (int8_t)(q > 127 ? 127 : q < -127 ? -127 : q);
        }
        if (t->texture >= STAGE_MAX_TEXTURE_NUMBERS)
            die("stage face texture number %u: at most %d per stage", t->texture, STAGE_MAX_TEXTURE_NUMBERS);
        rom[i].texture = (uint16_t)(t->texture | (t->flags & SURF_SCENARIO_MASK));
        rom[i].flags = (uint8_t)((t->flags & (SURF_ANIM_UV | TRI_ALPHA)) |
                                 (t->ledges & TRI_SOURCE_BLEND_QUARTER ? TRI_BLEND_QUARTER : 0) |
                                 (t->ledges & TRI_LEDGE_WATER_TINT ? TRI_WATER_TINT : 0));
        if (!drawn[i]) continue;
        if (t->water_texture != t->texture) any_water = 1;
        water[t->texture] = t->water_texture;
        if (t->flags & SURF_ANIM_UV) {
            any_scroll = 1;
            scroll[t->texture][0] = t->uv_scroll_speed[0];
            scroll[t->texture][1] = t->uv_scroll_speed[1];
        }
    }
    h->triangles = buffer_append_aligned(b, rom, n * sizeof(StageTriangle));
    if (any_water) h->texture_water = buffer_append_aligned(b, water, nt * sizeof *water);
    if (any_scroll) h->texture_scroll = buffer_append_aligned(b, scroll, nt * sizeof *scroll);
    free(rom);
    free(water);
    free(scroll);
    free(drawn);
}

static void append_render_tables(ByteBuffer *b, StageHeader *h, Model *mp, size_t *max_tris,
                                 size_t *max_ranks, int indexed_allowed, int far) {
    Model m = *mp;
    int indexed = indexed_allowed && leaves_fit_indexed_visibility(&m);
    size_t n_tris = m.triangles.len;
    uint16_t *old_of = xmalloc(sizeof(uint16_t) * (n_tris + 1));
    uint16_t *new_of = xmalloc(sizeof(uint16_t) * (n_tris + 1));
    for (size_t t = 0; t < n_tris; t++) old_of[t] = new_of[t] = (uint16_t)t;
    if (indexed) {
        uint8_t *placed = xcalloc(n_tris + 1, 1);
        size_t next = 0;
        for (size_t j = 0; j < m.leaf_triangles.len; j++) {
            uint16_t t = VEC_AT(m.leaf_triangles, uint16_t, j);
            if (placed[t]) die("a face in two leaves");
            placed[t] = 1;
            old_of[next++] = t;
        }
        for (size_t t = 0; t < n_tris; t++) if (!placed[t]) old_of[next++] = (uint16_t)t;
        for (size_t t = 0; t < n_tris; t++) new_of[old_of[t]] = (uint16_t)t;
        Vector tris = VEC(BuildTriangle), leaf = VEC(uint16_t);
        for (size_t t = 0; t < n_tris; t++)
            *(BuildTriangle *)vec_push(&tris) = VEC_AT(m.triangles, BuildTriangle, old_of[t]);
        for (size_t j = 0; j < m.leaf_triangles.len; j++) *(uint16_t *)vec_push(&leaf) = (uint16_t)j;
        m.triangles = tris;
        m.leaf_triangles = leaf;
        free(placed);
    }
    h->point_count = (uint32_t)m.points.len;
    h->triangle_count = (uint32_t)m.triangles.len;
    h->texture_count = (uint32_t)m.textures.len;
    h->chunk_count = (uint32_t)m.chunks.len;
    h->leaf_count = (uint32_t)m.leaf_triangle_offsets.len - 1;
    h->leaf_triangle_count = (uint32_t)m.leaf_triangles.len;

    size_t per_axis = m.tri_axes.len / 6;
    h->rank_count = (uint32_t)(per_axis * 3);
    if (m.triangles.len > *max_tris) *max_tris = m.triangles.len;
    if (per_axis * 3 > *max_ranks) *max_ranks = per_axis * 3;

    h->points = buffer_append_aligned(b, m.points.data, m.points.len * 6);
    append_stage_triangles(b, h, &m);
    if (far) append_far_textures(b, h, &m);
    else h->textures = buffer_append_aligned(b, m.textures.data, m.textures.len * sizeof(Texture));
    h->chunks = buffer_append_aligned(b, m.chunks.data, m.chunks.len * sizeof(StageChunk));
    h->leaf_triangle_offsets =
        buffer_append_aligned(b, m.leaf_triangle_offsets.data, m.leaf_triangle_offsets.len * 2);

    LeafTriangle *leaves = xcalloc(m.leaf_triangles.len, sizeof(LeafTriangle));

    uint16_t *ranks = xmalloc(sizeof(uint16_t) * 3 * m.leaf_triangles.len + 2);
    for (size_t j = 0; j < m.leaf_triangles.len; j++) {
        uint16_t t = VEC_AT(m.leaf_triangles, uint16_t, j);
        const BuildTriangle *tri = &VEC_AT(m.triangles, BuildTriangle, t);
        for (int i = 0; i < 3; i++) {
            leaves[j].points[i] = tri->v[i].point;
            leaves[j].a[i] = VEC_AT(m.points, Point3, tri->v[0].point)[i];
            leaves[j].normal[i] = tri->normal[i];
        }
        for (int k = 0; k < 3; k++)
            ranks[k * m.leaf_triangles.len + j] = VEC_AT(m.tri_rank, Rank6, old_of[t])[2 * k];
    }

    size_t segments = m.rank_segments.len, rank_triangle_words = per_axis * 3 + 1 + segments;
    uint16_t *rank_triangles = xmalloc(2 * rank_triangle_words + 2);
    for (int k = 0; k < 3; k++) {
        for (size_t i = 0; i < per_axis; i++) {
            const AxisEntry *entry = &VEC_AT(m.tri_axes, AxisEntry, 2 * k * per_axis + i);
            rank_triangles[k * per_axis + i] = new_of[entry->tri];
        }
    }
    if (segments > 0xFFFF) die("too many draw order segments");
    rank_triangles[per_axis * 3] = (uint16_t)segments;
    for (size_t i = 0; i < segments; i++)
        rank_triangles[per_axis * 3 + 1 + i] = VEC_AT(m.rank_segments, uint16_t, i);
    if (indexed) {
        h->leaf_triangles = 0;
        h->surface_features |= STAGE_INDEXED_LEAVES;
    } else {
        h->leaf_triangles = buffer_append_aligned(b, leaves, m.leaf_triangles.len * sizeof(LeafTriangle));
    }
    h->leaf_draw_ranks = buffer_append_aligned(b, ranks, m.leaf_triangles.len * 3 * 2);
    h->rank_triangles = buffer_append_aligned(b, rank_triangles, rank_triangle_words * 2);
    free(leaves);
    free(ranks);
    free(rank_triangles);
    free(old_of);
    free(new_of);
    if (indexed) {
        vec_free(&m.triangles);
        vec_free(&m.leaf_triangles);
    }
}

typedef struct {
    const char *stage;
    int kind;
    int32_t origin[3];
    int16_t axes[3][3];
    int32_t size[3];
} CompileWater;
#include "water_volumes_gen.h"

static int point_in_water(const char *stage, const double point[3]) {
    for (unsigned i = 0; i < sizeof compile_water / sizeof compile_water[0]; i++) {
        const CompileWater *w = &compile_water[i];
        if (strcmp(stage, w->stage)) continue;
        double d[3], local[3] = { 0 };
        for (int j = 0; j < 3; j++) d[j] = point[j] - w->origin[j];
        for (int j = 0; j < 3; j++)
            for (int k = 0; k < 3; k++) local[j] += d[k] * w->axes[j][k] / 16384.0;
        double radius_squared = (double)w->size[0] * w->size[0];
        int in_height = local[1] >= 0 && local[1] < w->size[1];
        if (w->kind == 0) {
            if (fabs(local[0]) < w->size[0] && in_height && fabs(local[2]) < w->size[2]) return 1;
        } else if (w->kind == 1) {
            if (in_height && local[0] * local[0] + local[2] * local[2] < radius_squared) return 1;
        } else if (d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < radius_squared) {
            if (w->kind != 3 || local[1] < 0) return 1;
        }
    }
    return 0;
}

static void face_centre(const Model *m, const BuildTriangle *t, int shift, double centre[3]) {
    centre[0] = centre[1] = centre[2] = 0;
    for (int j = 0; j < 3; j++)
        for (int k = 0; k < 3; k++)
            centre[k] += (double)VEC_AT(m->points, Point3, t->v[j].point)[k] * (1 << shift) / 3.0;
}

static void tint_faces_below_water_plane(Model *m, double radius, int shift) {
    int count = 0;
    for (size_t i = 0; i < m->triangles.len; i++) {
        BuildTriangle *t = &VEC_AT(m->triangles, BuildTriangle, i);
        double centre[3];
        face_centre(m, t, shift, centre);
        if (centre[2] < 0 && centre[0] * centre[0] + centre[1] * centre[1] < radius * radius) {
            t->ledges |= TRI_LEDGE_WATER_TINT;
            count++;
        }
    }
    printf("  below the water plane: %d faces\n", count);
}

static void tint_faces_in_water(Model *m, const char *stage, int shift) {
    int count = 0;
    for (size_t i = 0; i < m->triangles.len; i++) {
        BuildTriangle *t = &VEC_AT(m->triangles, BuildTriangle, i);
        double centre[3];
        face_centre(m, t, shift, centre);
        if (point_in_water(stage, centre)) {
            t->ledges |= TRI_LEDGE_WATER_TINT;
            count++;
        }
    }
    printf("  in water: %d faces\n", count);
}

static int has_water_volumes(const char *stage) {
    for (unsigned i = 0; i < sizeof compile_water / sizeof compile_water[0]; i++)
        if (!strcmp(stage, compile_water[i].stage)) return 1;
    return 0;
}

static const char *const g_minus_order_figures[] = { "mario", "mariobee", "ghostmario" };
static char *g_minus_order_models;

static char *g_stage_rock_drops[256];
static Vector g_rock_drop_data[256];

static void build_stage_occluder(const StageCfg *cfg, const Model *m) {
    int n = m->occl_n;
    if (n < 1 || n > 64) die("%s: occluder grid of %d cells", cfg->name, n);
    float lo = INFINITY, hi = -INFINITY;
    for (int k = 0; k < n * n; k++)
        if (!isnan(m->occl_above[k])) {
            lo = fminf(lo, fminf(m->occl_above[k], m->occl_below[k]));
            hi = fmaxf(hi, fmaxf(m->occl_above[k], m->occl_below[k]));
        }
    if (!isfinite(lo)) die("%s: occluder: no grid cell lies inside the cylinder", cfg->name);
    StageOccluder o = { (int32_t)lroundf(m->occl_x0 * 256.0f), (int32_t)lroundf(m->occl_y0 * 256.0f),
                        (int32_t)lroundf((lo + hi) * 128.0f), (uint8_t)n, 0, { 0, 0 } };
    ByteBuffer b = { 0 };
    buffer_append(&b, &o, sizeof o);
    int8_t *grid = xmalloc((size_t)(2 * n * n));
    int cells = 0;
    for (int k = 0; k < n * n; k++) {
        if (isnan(m->occl_above[k])) {
            grid[k] = 127;
            grid[n * n + k] = -128;
            continue;
        }

        double a = ceil((m->occl_above[k] * 256.0 - o.base) / 32.0);
        double w = floor((m->occl_below[k] * 256.0 - o.base) / 32.0);
        if (a > 126 || w < -127)
            die("%s: occluder heights %.2f..%.2f do not fit the grid's range", cfg->name, lo, hi);
        grid[k] = (int8_t)a;
        grid[n * n + k] = (int8_t)w;
        cells++;
    }
    buffer_append(&b, grid, (size_t)(2 * n * n));
    free(grid);
    while (b.len & 3) buffer_append(&b, "", 1);
    int ranges = 0, hidden[3] = { 0, 0, 0 };
    for (size_t l = 0; l < m->leaf_occl.len;) {
        uint8_t c = VEC_AT(m->leaf_occl, uint8_t, l);
        size_t e = l + 1;
        while (e < m->leaf_occl.len && VEC_AT(m->leaf_occl, uint8_t, e) == c) e++;
        if (c) {
            StageOccluderRange r = { (uint16_t)l, (uint16_t)(e - l), c, { 0, 0, 0 } };
            buffer_append(&b, &r, sizeof r);
            ranges++;
            hidden[c] += (int)(e - l);
        }
        l = e;
    }
    if (ranges > 255) die("%s: occluder: %d leaf ranges", cfg->name, ranges);
    ((StageOccluder *)b.data)->range_count = (uint8_t)ranges;
    char *sym = format_string("occl_%s", cfg->name);
    write_asset_blob(sym, format_string("uint8_t %s[]", sym), b.data, b.len);
    printf("  occluder: %d camera cells, %d leaf ranges (%d leaves hidden from above, %d from below), "
           "%zu bytes\n", cells, ranges, hidden[1], hidden[2], b.len);
}

static StagePart stage_part(const int leaves[2], const PartDef *part) {
    return (StagePart){ (uint16_t)(leaves[0] < 0 ? 0 : leaves[0]), (uint16_t)leaves[1], (int16_t)part->sw,
                        (uint8_t)part->mode, 0 };
}

static uint16_t scenario_surface_flag(const Model *m) {
    for (size_t i = 0; i < m->triangles.len; i++)
        if (VEC_AT(m->triangles, BuildTriangle, i).flags & SURF_SCENARIO_MASK) return STAGE_SCENARIO_SURFACES;
    return 0;
}

static void build_stage(int index, const StageCfg *cfg, size_t *max_tris, size_t *max_ranks) {
    ASSETC_PROGRESS(30, 10, (uint32_t)index);
    char *dir = format_string("%s/stages/%s", g_in, cfg->name);
    char *obj = format_string("%s.obj", cfg->name);
    StageExtras extras = stage_extras_load(format_string("%s/%s.stage", dir, cfg->name),
                                           format_string("%s/%s.galaxy", dir, cfg->name));
    ASSETC_PROGRESS(30, 11, (uint32_t)index);

    int shift = model_point_shift(format_string("%s/%s", dir, obj));
    ASSETC_PROGRESS(30, 12, (uint32_t)index);
    if (cfg->far) {
        int fs = model_point_shift(format_string("%s/%s", dir, cfg->far));
        if (fs > shift) shift = fs;
    }
    ASSETC_PROGRESS(30, 13, (uint32_t)index);
    float sun_len = sqrtf(0.2f * 0.2f + -0.4f * -0.4f + -1.0f * -1.0f);

    int tinted = cfg->has_water || (cfg->underwater && has_water_volumes(cfg->name));
    if (cfg->underwater && !tinted) printf("  underwater without water volumes or level\n");
    ModelParams p = {
        .dir = dir,
        .obj = obj,
        .texture_underlays = cfg->texture_underlays,
        .texture_underlay_count = cfg->texture_underlay_count,
        .texture_alpha_policies = cfg->texture_alpha_policies,
        .texture_alpha_policy_count = cfg->texture_alpha_policy_count,
        .shading = cfg->shading,
        .sun = { 0.2f / sun_len, -0.4f / sun_len, -1.0f / sun_len },
        .underwater = cfg->underwater && tinted,
        .light_up = extras.gravity_count ? stage_extras_up_at : NULL,
        .light_ctx = &extras,
        .point_shift = shift,
        .part_names = extras.part_names,
        .part_count = extras.part_count,
        .occluder = cfg->has_occluder ? cfg->occluder : NULL,
    };
    printf("stage %s\n", cfg->name);
    fflush(stdout);
    ASSETC_PROGRESS(30, 0, (uint32_t)index);
    Model m = model_build(&p, g_pal);
    ASSETC_PROGRESS(30, 1, (uint32_t)index);
    if (cfg->underwater) tint_faces_in_water(&m, cfg->name, shift);
    if (cfg->water_plane > 0) tint_faces_below_water_plane(&m, cfg->water_plane, shift);

    Model fm = { 0 };
    if (cfg->far) {
        ModelParams fp = p;
        fp.obj = cfg->far;
        fp.occluder = NULL;
        printf("  far LOD %s\n", cfg->far);
        fm = model_build(&fp, g_pal);
        ASSETC_PROGRESS(30, 2, (uint32_t)index);
        if (cfg->underwater) tint_faces_in_water(&fm, cfg->name, shift);
        if (cfg->water_plane > 0) tint_faces_below_water_plane(&fm, cfg->water_plane, shift);
        if (fm.points.len > STAGE_FAR_POINT_LIMIT)
            die("%s: far LOD has %zu points, at most %d fit", cfg->far, fm.points.len, STAGE_FAR_POINT_LIMIT);
    }

    StageHeader h = { 0 };
    h.surface_features = scenario_surface_flag(&m);
    ByteBuffer b = { 0 };
    buffer_append(&b, NULL, sizeof h);

    HomeTables ht = { VEC(uint16_t), VEC(uint16_t), 0 };

    if (m.points.len > STAGE_DIRECT_POINTS || (cfg->far && fm.points.len > STAGE_FAR_DIRECT_POINTS))
        ht = number_points_by_leaf(&m);
    if (m.points.len > 32767) die("%s: %zu points, at most 32767", cfg->name, m.points.len);
    split_texture_slots(&m, cfg->name);

    append_render_tables(&b, &h, &m, max_tris, max_ranks, ht.home.len != 0, 0);
    if ((size_t)h.leaf_count > g_max_leaves) g_max_leaves = h.leaf_count;
    if (ht.home.len) {
        h.point_home = buffer_append_aligned(&b, ht.home.data, ht.home.len * 2);
        h.leaf_point_first = buffer_append_aligned(&b, ht.first.data, ht.first.len * 2);
        h.leaf_slot_max = (uint16_t)ht.slot_max;
        if ((size_t)h.leaf_count > g_max_home_leaves) g_max_home_leaves = h.leaf_count;
        printf("  points numbered by leaf: %u leaves, at most %d pool slots per leaf\n", h.leaf_count,
               ht.slot_max);
    }
    if (m.points.len > g_max_points) g_max_points = m.points.len;
    if (m.textures.len > g_max_textures) g_max_textures = m.textures.len;
    h.point_shift = (uint8_t)shift;
    h.galaxy = (uint32_t)extras.galaxy;
    printf("  %zu points, %zu triangles, %zu textures, %d gravity, %d objects, point shift %d\n",
           m.points.len, m.triangles.len, m.textures.len, extras.gravity_count, extras.object_count, shift);

    Vector col_tris, col_nodes;
    collision_build(&m, shift, &col_tris, &col_nodes);
    ASSETC_PROGRESS(31, 1, (uint32_t)index);

    if (!strcmp(cfg->name, "freezeflame") && col_tris.len > 0x7200)
        die("%s: %zu collision triangles, at most 0x7200 (higher IDs belong to ice steps)", cfg->name,
            col_tris.len);
    h.collision_triangle_count = (uint32_t)col_tris.len;
    h.collision_node_count = (uint32_t)col_nodes.len;
    h.collision_triangles = buffer_append_aligned(&b, col_tris.data, col_tris.len * sizeof(CollisionFace));
    h.collision_nodes = buffer_append_aligned(&b, col_nodes.data, col_nodes.len * sizeof(CollisionNode));
    printf("  collision: %zu triangles, %zu nodes\n", col_tris.len, col_nodes.len);

    char *sky_path = format_string("%s/sky.png", dir);
    Image sky = png_load(sky_path);
    if (sky.w != SKY_DIM || sky.h != SKY_DIM) die("%s: must be %dx%d", sky_path, SKY_DIM, SKY_DIM);
    ColorMap *cmap = colormap_create(g_pal, 256);

    uint8_t *sky_idx = xmalloc(SKY_W * SKY_DIM);
    for (int y = 0; y < SKY_DIM; y++)
        for (int x = 0; x < SKY_W; x++) {
            const uint8_t *a = &sky.rgba[(y * SKY_DIM + 2 * x) * 4], *c = a + 4;
            int la = a[0] * 299 + a[1] * 587 + a[2] * 114, lc = c[0] * 299 + c[1] * 587 + c[2] * 114;
            const uint8_t *p = lc > la ? c : a;
            sky_idx[y * SKY_W + x] = (uint8_t)colormap_nearest_rgb(cmap, p[0], p[1], p[2]);
        }
    h.sky = buffer_append_aligned(&b, sky_idx, SKY_W * SKY_DIM);
    ASSETC_PROGRESS(31, 2, (uint32_t)index);

    Gravity *grav = xcalloc((size_t)extras.gravity_count + 1, sizeof(Gravity));
    for (int i = 0; i < extras.gravity_count; i++) {
        grav[i] = gravity_to_asset(&extras.gravities[i]);
        const GravityDef *gd = &extras.gravities[i];
        if (gd->kind == GRAV_WIRE) {

            int32_t (*pts)[3] = xmalloc(sizeof(int32_t[3]) * (size_t)gd->wire_count);
            for (int j = 0; j < gd->wire_count; j++)
                for (int k = 0; k < 3; k++) pts[j][k] = float_to_fixed(gd->wire[j][k], 8);
            size_t size = sizeof(int32_t[3]) * (size_t)gd->wire_count;
            grav[i].extent[0] = (int32_t)buffer_append_aligned(&b, pts, size);
            grav[i].extent[1] = gd->wire_count;
            grav[i].extent[2] = 0;
            free(pts);
        }
    }
    h.gravity_count = (uint32_t)extras.gravity_count;
    h.gravities = buffer_append_aligned(&b, grav, sizeof(Gravity) * (size_t)extras.gravity_count);
    if (extras.gravity_count > 8) {
        GridItem *items = xcalloc((size_t)extras.gravity_count, sizeof(GridItem));
        for (int i = 0; i < extras.gravity_count; i++) {
            memcpy(items[i].centre, grav[i].centre, sizeof items[i].centre);
            items[i].bound = grav[i].bound;
        }
        h.gravity_grid = append_bound_grid(&b, items, extras.gravity_count, "gravity");
        free(items);
    }

    StageObject *objs = xcalloc((size_t)extras.object_count + 1, sizeof(StageObject));
    for (int i = 0; i < extras.object_count; i++) objs[i] = object_to_asset(&extras.objects[i]);
    h.object_count = (uint32_t)extras.object_count;
    if ((size_t)extras.object_count > g_max_objects) g_max_objects = (size_t)extras.object_count;
    h.objects = buffer_append_aligned(&b, objs, sizeof(StageObject) * (size_t)extras.object_count);

    if (!extras.start_count) die("%s: no start point", cfg->name);
    StageStart *starts = xcalloc((size_t)extras.start_count, sizeof(StageStart));
    for (int i = 0; i < extras.start_count; i++) starts[i] = start_to_asset(&extras.starts[i]);
    h.start_count = (uint32_t)extras.start_count;
    h.starts = buffer_append_aligned(&b, starts, sizeof(StageStart) * (size_t)extras.start_count);

    Vector rails, rail_points, rail_parts;
    rails_to_asset(&extras, &rails, &rail_points, &rail_parts);
    h.rail_count = (uint32_t)rails.len;
    h.rails = buffer_append_aligned(&b, rails.data, rails.len * sizeof(StageRail));
    h.rail_points = buffer_append_aligned(&b, rail_points.data, rail_points.len * sizeof(StageRailPoint));
    h.rail_parts = buffer_append_aligned(&b, rail_parts.data, rail_parts.len * sizeof(StageRailPart));
    {
        Vector drops;
        if (rock_drops_build(&extras, &m, shift, &rails, &rail_parts, &drops)) {
            for (int i = 0; i < 256 && !g_stage_rock_drops[index]; i++)
                if (g_stage_rock_drops[i] && g_rock_drop_data[i].len == drops.len &&
                    !memcmp(g_rock_drop_data[i].data, drops.data, drops.len))
                    g_stage_rock_drops[index] = g_stage_rock_drops[i];
            if (!g_stage_rock_drops[index]) {
                char *drops_name = format_string("rock_drops_%s", cfg->name);
                g_stage_rock_drops[index] = drops_name;
                write_asset_blob(drops_name, format_string("uint8_t %s[]", drops_name), drops.data,
                                 drops.len);
            }
            g_rock_drop_data[index] = drops;
        }
    }

    StageArea *areas = xcalloc((size_t)extras.area_count + 1, sizeof(StageArea));
    for (int i = 0; i < extras.area_count; i++) areas[i] = area_to_asset(&extras.areas[i]);
    h.area_count = (uint32_t)extras.area_count;
    h.areas = buffer_append_aligned(&b, areas, sizeof(StageArea) * (size_t)extras.area_count);
    if (extras.area_count > 8) {
        GridItem *items = xcalloc((size_t)extras.area_count, sizeof(GridItem));
        for (int i = 0; i < extras.area_count; i++) {
            memcpy(items[i].centre, areas[i].centre, sizeof items[i].centre);
            items[i].bound = areas[i].bound;
        }
        h.area_grid = append_bound_grid(&b, items, extras.area_count, "area");
        free(items);
    }

    StageCamera *cams = xcalloc((size_t)extras.camera_count + 1, sizeof(StageCamera));
    for (int i = 0; i < extras.camera_count; i++) cams[i] = camera_to_asset(&extras.cameras[i]);
    h.camera_count = (uint32_t)extras.camera_count;
    h.cameras = buffer_append_aligned(&b, cams, sizeof(StageCamera) * (size_t)extras.camera_count);
    if (extras.galaxy)
        printf("  galaxy data: %d starts, %zu rails (%zu parts), %d areas, %d cameras\n", extras.start_count,
               rails.len, rail_parts.len, extras.area_count, extras.camera_count);
    if (extras.part_count) {
        StagePart parts[STAGE_MAX_PARTS];
        memset(parts, 0, sizeof parts);
        for (int i = 0; i < extras.part_count; i++) {
            const int *pl = VEC_AT(m.part_leaves, IntPair, i);
            if (!pl[1] && pl[0] != -1)
                die("%s: part %s: no such OBJ object", cfg->name, extras.parts[i].name);
            parts[i] = stage_part(pl, &extras.parts[i]);
            printf("  part %s: leaves %d..%d, switch %d\n", extras.parts[i].name, pl[0], pl[0] + pl[1] - 1,
                   extras.parts[i].sw);
        }
        h.part_count = (uint32_t)extras.part_count;
        h.parts = buffer_append_aligned(&b, parts, sizeof(StagePart) * (size_t)extras.part_count);
    }
    h.death_height = float_to_fixed(cfg->death_height, 8);
    h.music = (uint8_t)cfg->music;

    h.has_water = (uint8_t)cfg->has_water;
    if (cfg->underwater && has_water_volumes(cfg->name)) h.has_water = 2;
    h.has_water_region = (uint8_t)cfg->has_region;
    h.water_level = float_to_fixed(cfg->water, 8);
    h.water_centre[0] = float_to_fixed(cfg->region[0], 8);
    h.water_centre[1] = float_to_fixed(cfg->region[1], 8);
    h.water_radius_squared = square_to_fixed(cfg->region[2]);
    h.water_min_z = float_to_fixed(cfg->region[3], 8);

    memcpy(b.data, &h, sizeof h);
    char *sym = format_string("stage_%s", cfg->name);
    write_asset_blob(sym, format_string("uint8_t %s[]", sym), b.data, b.len);
    ASSETC_PROGRESS(31, 3, (uint32_t)index);
    if (cfg->has_occluder) build_stage_occluder(cfg, &m);
    char *upper = xstrdup(cfg->name);
    for (char *c = upper; *c; c++) *c = (char)toupper((unsigned char)*c);
    append_header("#define STAGE_%s %d\n", upper, index);

    if (cfg->far) {
        split_texture_slots(&fm, cfg->far);
        if (fm.points.len > g_max_far_points) g_max_far_points = fm.points.len;
        if (fm.textures.len > g_max_textures) g_max_textures = fm.textures.len;
        StageHeader fh = { 0 };
        fh.surface_features = scenario_surface_flag(&fm);
        fh.point_shift = (uint8_t)shift;
        ByteBuffer fb = { 0 };
        buffer_append(&fb, NULL, sizeof fh);
        append_render_tables(&fb, &fh, &fm, max_tris, max_ranks, 0, 1);
        printf("  far: %zu points, %zu triangles\n", fm.points.len, fm.triangles.len);
        if (extras.part_count) {

            StagePart parts[STAGE_MAX_PARTS];
            memset(parts, 0, sizeof parts);
            for (int i = 0; i < extras.part_count; i++) {
                const int *pl = VEC_AT(fm.part_leaves, IntPair, i);
                parts[i] = stage_part(pl, &extras.parts[i]);
            }
            fh.part_count = (uint32_t)extras.part_count;
            fh.parts = buffer_append_aligned(&fb, parts, sizeof(StagePart) * (size_t)extras.part_count);
        }
        memcpy(fb.data, &fh, sizeof fh);
        char *fsym = format_string("stage_%s_far", cfg->name);
        write_asset_blob(fsym, format_string("uint8_t %s[]", fsym), fb.data, fb.len);
    }
}

static uint8_t triangle_flat_colour(const Texture *tex, const ModelVertex v[3]) {
    float sum[3] = { 0, 0, 0 };
    int n = 0;
    for (int a = 0; a <= 8; a++) {
        for (int b = 0; b <= 8 - a; b++) {
            float wa = (float)a / 8.0f, wb = (float)b / 8.0f, wc = 1.0f - wa - wb;
            float u = wa * v[0].u + wb * v[1].u + wc * v[2].u;
            float w = wa * v[0].v + wb * v[1].v + wc * v[2].v;
            int tx = ((int)floorf(u / 16.0f)) & 31, ty = ((int)floorf(w / 16.0f)) & 31;
            uint8_t idx = tex->mip[0][ty * 32 + tx];
            if (!idx) continue;
            sum[0] += g_pal[idx].r;
            sum[1] += g_pal[idx].g;
            sum[2] += g_pal[idx].b;
            n++;
        }
    }
    if (!n) return 0;
    float col[3] = { sum[0] / n, sum[1] / n, sum[2] / n };
    return (uint8_t)palette_nearest_opaque(g_pal, col);
}

static void build_figure(const FigureCfg *cfg) {
    char *dir = format_string("%s/figures/%s", g_in, cfg->name);
    ModelParams p = {
        .dir = dir,
        .obj = cfg->obj,
        .shading_base = cfg->shading_base,
        .shading = cfg->shading,
        .sun = { 0.0f, 0.0f, 1.0f },
        .is_figure = 1,
        .source_zero_faces = cfg->source_zero_faces,
        .texture_alpha_policies = cfg->texture_alpha_policies,
        .texture_alpha_policy_count = cfg->texture_alpha_policy_count,
    };
    printf("figure %s\n", cfg->name);
    fflush(stdout);
    Model m = model_build(&p, g_pal);
    figure_screen_bound_include(m.points.len, cfg->name);
    if (cfg->static_texture_dedup) {

        if (cfg->n_anims) die("%s: static_texture_dedup does not permit animations", cfg->name);
        uint16_t *remap = xcalloc(m.textures.len, sizeof *remap);
        size_t unique = 0, original = m.textures.len;
        for (size_t i = 0; i < original; i++) {
            const Texture texture = VEC_AT(m.textures, Texture, i);
            size_t j = 0;
            while (j < unique && memcmp(&VEC_AT(m.textures, Texture, j), &texture, sizeof texture)) j++;
            if (j == unique) VEC_AT(m.textures, Texture, unique++) = texture;
            remap[i] = (uint16_t)j;
        }
        for (size_t i = 0; i < m.triangles.len; i++) {
            BuildTriangle *tri = &VEC_AT(m.triangles, BuildTriangle, i);
            if (tri->texture >= original) die("%s: invalid static texture index", cfg->name);
            tri->texture = remap[tri->texture];
        }
        m.textures.len = unique;
        printf("  static texture bank: %zu -> %zu records (%zu bytes saved)\n",
               original, unique, (original - unique) * sizeof(Texture));
        free(remap);
    }

    FigureHeader h = { 0 };
    ByteBuffer b = { 0 };
    buffer_append(&b, NULL, sizeof h);
    h.point_count = (uint16_t)m.points.len;
    h.triangle_count = (uint16_t)m.triangles.len;
    h.texture_count = (uint16_t)m.textures.len;
    quantize_pose_values((int16_t *)m.points.data, m.points.len * 3, (unsigned)m.points.len);
    h.points = buffer_append_aligned(&b, m.points.data, m.points.len * 6);
    for (size_t i = 0; i < m.points.len; i++)
        for (int k = 0; k < 3; k++) {
            int c = abs(((const int16_t *)m.points.data)[i * 3 + k]);
            if ((uint32_t)c > h.extent) h.extent = (uint32_t)c;
        }

    ByteBuffer kf = { 0 };
    char *upper = xstrdup(cfg->name);
    for (char *c = upper; *c; c++) *c = (char)toupper((unsigned char)*c);
    AnimClip *clips = xcalloc((size_t)cfg->n_anims + 1, sizeof(AnimClip));
    for (int a = 0; a < cfg->n_anims; a++) {
        const AnimCfg *an = &cfg->anims[a];
        char *aname = xstrdup(an->name);
        for (char *c = aname; *c; c++) *c = (char)toupper((unsigned char)*c);
        append_header("#define ANIM_%s_%s %d\n", upper, aname, a);
        clips[a].first_key = (uint16_t)h.keyframe_count;
        clips[a].key_count = (uint16_t)an->n_frames;
        clips[a].ticks_per_key = (uint8_t)an->ticks_per_key;
        clips[a].loop = (uint8_t)an->loop;
        for (int f = 0; f < an->n_frames; f++) {
            char *path = format_string("%s/%s.obj", dir, an->frames[f]);
            Vector pts = model_load_positions(path);
            if (pts.len != m.points.len) die("%s: has %zu points, expected %zu", path, pts.len, m.points.len);
            buffer_append(&kf, pts.data, pts.len * 6);
            if (h.keyframe_count == FIGURE_KEY_COUNT) die("%s: too many pose keys", cfg->name);
            h.keyframe_count++;
            vec_free(&pts);
        }
    }
    hold_one_tick_keys(&kf, h.point_count, clips, (unsigned)cfg->n_anims, cfg->name);
    h.keyframes = append_figure_keys(&b, &h, &kf, m.points.data, cfg->name);
    h.clip_count = (uint16_t)cfg->n_anims;
    h.clips = buffer_append_aligned(&b, clips, sizeof(AnimClip) * (size_t)cfg->n_anims);
    append_header("#define ANIM_%s_COUNT %d\n", upper, cfg->n_anims);

    FigureTriangle *triangles = xcalloc(m.triangles.len, sizeof(FigureTriangle));
    for (size_t i = 0; i < m.triangles.len; i++) {
        const BuildTriangle *s = &VEC_AT(m.triangles, BuildTriangle, i);
        memcpy(triangles[i].v, s->v, sizeof s->v);
        triangles[i].texture = s->texture;
        triangles[i].flags = (uint16_t)((s->flags & TRI_ALPHA) |
            triangle_flat_colour(&VEC_AT(m.textures, Texture, s->texture), s->v) << 8);
    }
    h.triangles = buffer_append_aligned(&b, triangles, m.triangles.len * sizeof(FigureTriangle));
    h.axes = buffer_append_aligned(&b, NULL, 0);

    for (int k = 0; k < 6; k += 2) buffer_append(&b, m.draw_axes[k].data, m.triangles.len * 2);
    char *lod_path = format_string("%s/lod.obj", dir);
    uint8_t *lod_source = read_file(lod_path, NULL);
    int has_lod = lod_source != NULL;
    Model ml = { 0 };
    uint16_t *lod_texture_map = NULL;
    if (lod_source) {
        free(lod_source);
        ModelParams lp = p;
        lp.obj = "lod.obj";
        ml = model_build(&lp, g_pal);
        if (ml.points.len != m.points.len)
            die("%s: LOD has %zu points, expected %zu", lod_path, ml.points.len, m.points.len);
        lod_texture_map = xcalloc(ml.textures.len, sizeof *lod_texture_map);

        for (size_t i = 0; i < ml.textures.len; i++) {
            const Texture *texture = &VEC_AT(ml.textures, Texture, i);
            size_t index = 0;
            while (index < m.textures.len &&
                   memcmp(&VEC_AT(m.textures, Texture, index), texture, sizeof *texture)) index++;
            if (index == m.textures.len) *(Texture *)vec_push(&m.textures) = *texture;
            if (index > UINT16_MAX) die("%s: too many merged textures", cfg->name);
            lod_texture_map[i] = (uint16_t)index;
        }
    }
    if (m.textures.len > UINT16_MAX) die("%s: too many merged textures", cfg->name);
    h.texture_count = (uint16_t)m.textures.len;
    h.textures = buffer_append_aligned(&b, m.textures.data, m.textures.len * sizeof(Texture));
    if (has_lod) {
        FigureTriangle *lt = xcalloc(ml.triangles.len, sizeof(FigureTriangle));
        for (size_t i = 0; i < ml.triangles.len; i++) {
            const BuildTriangle *s = &VEC_AT(ml.triangles, BuildTriangle, i);
            memcpy(lt[i].v, s->v, sizeof s->v);
            lt[i].texture = lod_texture_map[s->texture];
            lt[i].flags = (uint16_t)((s->flags & TRI_ALPHA) |
                triangle_flat_colour(&VEC_AT(ml.textures, Texture, s->texture), s->v) << 8);
        }
        h.lod_triangle_count = (uint16_t)ml.triangles.len;
        h.lod_texture_count = h.texture_count;
        h.lod_triangles = buffer_append_aligned(&b, lt, ml.triangles.len * sizeof(FigureTriangle));
        h.lod_axes = buffer_append_aligned(&b, NULL, 0);
        for (int k = 0; k < 6; k += 2) buffer_append(&b, ml.draw_axes[k].data, ml.triangles.len * 2);
        h.lod_textures = h.textures;
        uint8_t *used = xcalloc(m.points.len, 1);
        for (size_t i = 0; i < ml.triangles.len; i++)
            for (int k = 0; k < 3; k++) used[VEC_AT(ml.triangles, BuildTriangle, i).v[k].point] = 1;
        uint16_t *list = xcalloc(m.points.len, sizeof(uint16_t));
        uint32_t n_used = 0;
        for (size_t i = 0; i < m.points.len; i++)
            if (used[i]) list[n_used++] = (uint16_t)i;
        h.lod_point_count = n_used;
        h.lod_points = buffer_append_aligned(&b, list, n_used * sizeof(uint16_t));
        printf("  lod: %zu triangles\n", ml.triangles.len);
    }
    free(lod_texture_map);
    memcpy(b.data, &h, sizeof h);
    char *sym = format_string("figure_%s", cfg->name);

    write_asset_blob(sym, format_string("uint8_t %s[]", sym), b.data, b.len);

    int minus_orders = 0;
    for (size_t i = 0; i < sizeof g_minus_order_figures / sizeof *g_minus_order_figures; i++)
        if (!strcmp(cfg->name, g_minus_order_figures[i])) minus_orders = 1;
    if (minus_orders) {
        ByteBuffer mo = { 0 };
        for (int k = 1; k < 6; k += 2) buffer_append(&mo, m.draw_axes[k].data, m.triangles.len * 2);
        if (has_lod)
            for (int k = 1; k < 6; k += 2) buffer_append(&mo, ml.draw_axes[k].data, ml.triangles.len * 2);
        char *msym = format_string("draw_minus_orders_%s", cfg->name);
        write_asset_blob(msym, format_string("uint16_t %s[]", msym), mo.data, mo.len);
        const char *list = g_minus_order_models ? g_minus_order_models : "";
        char *entry = format_string("%s X(%s, %s)", list, sym, msym);
        free(g_minus_order_models);
        g_minus_order_models = entry;
        free(mo.data);
    }
}

typedef struct {
    uint8_t *data;
    size_t length;
    char *symbol;
} SoundBank;

#define SOUND_EFFECT_TAIL_LEVEL 4
static size_t g_effect_tail_samples;

static void build_sounds(int music) {
    SoundBank *banks = xmalloc((size_t)(g_n_sounds ? g_n_sounds : 1) * sizeof *banks);
    int bank_count = 0;
    int count = 0;
    const char *kind = music ? "MUSIC" : "SFX";
    for (int i = 0; i < g_n_sounds; i++) {
        const SoundCfg *cfg = &g_sounds[i];
        if (cfg->is_music != music) continue;
        if ((i & 15) == 0 || i + 1 == g_n_sounds)
            ASSETC_PROGRESS(music ? 3 : 4, (uint32_t)i, (uint32_t)g_n_sounds);
#ifdef ASSETC_NO_AUDIO
        /* The GWHB runtime currently provides the __GNW__ silent audio
         * adapter. Keep the stable sound IDs and counts in assets_gen.h, but
         * do not require ISO-derived PCM inputs or emit sound payloads. The
         * sound blob macros remain available for targets that use audio. */
        char *id_upper = xstrdup(cfg->name);
        for (char *c = id_upper; *c; c++) *c = (char)toupper((unsigned char)*c);
        append_header("#define %s_%s %d\n", kind, id_upper, count++);
        free(id_upper);
        continue;
#endif
        size_t len;
        char *path = format_string("%s/audio/%s", g_in, cfg->file);
        uint8_t *pcm = read_file(path, &len);
        if (!pcm || !len) die("cannot read %s", path);

        if (cfg->loop_start < 0 || (size_t)cfg->loop_start > len) die("%s: bad loop start", path);

        size_t n, loop;
        float *x;
        if (music) {
            x = sound_resample((const int8_t *)pcm, len, (size_t)cfg->loop_start,
                               SOUND_MUSIC_SAMPLES_PER_REFRESH, SOUND_SAMPLES_PER_REFRESH, &n, &loop);
        } else {

            size_t full = len;
            while (len > SOUND_BLOCK && abs(((const int8_t *)pcm)[len - 1]) <= SOUND_EFFECT_TAIL_LEVEL) len--;
            if (len < full) g_effect_tail_samples += full - len;
            x = sound_resample((const int8_t *)pcm, len, len, SOUND_EFFECT_SAMPLES_PER_REFRESH,
                               SOUND_SAMPLES_PER_REFRESH, &n, &loop);
        }
        ByteBuffer b = { 0 };
        double sse = sound_encode(&b, x, n, loop), sig = 0;
        for (size_t k = 0; k < n; k++) sig += (double)x[k] * x[k];
        printf("%s %s: %zu -> %zu bytes, SNR %.1f dB\n", music ? "music" : "sound", cfg->name, len, b.len,
               sse > 0 ? 10 * log10(sig / sse) : 99.0);
        free(x);
        char *upper = xstrdup(cfg->name);
        for (char *c = upper; *c; c++) *c = (char)toupper((unsigned char)*c);
        char *sym = format_string("%s_%s", music ? "music" : "sfx", cfg->name);
        int same = -1;
        for (int j = 0; j < bank_count; j++) {
            if (banks[j].length == b.len && !memcmp(banks[j].data, b.data, b.len)) {
                same = j;
                break;
            }
        }
        if (same >= 0) {
            write_file(format_string("%s/%s.bin", g_out, sym), b.data, b.len);
            char *alias = format_string("\n    .global %s\n    .set %s, %s\n", sym, sym, banks[same].symbol);
            buffer_append(&g_asm, alias, strlen(alias));
            append_header("extern const uint8_t %s[];\n", sym);
            free(alias);
            free(b.data);
            free(sym);
        } else {
            write_asset_blob(sym, format_string("uint8_t %s[]", sym), b.data, b.len);
            banks[bank_count++] = (SoundBank){ b.data, b.len, sym };
        }
        append_header("#define %s_%s %d\n", kind, upper, count++);
        free(upper);
        free(path);
        free(pcm);
    }
    for (int j = 0; j < bank_count; j++) { free(banks[j].data); free(banks[j].symbol); }
    free(banks);
    append_header("#define %s_COUNT %d\n#define %s_BLOBS {", kind, count, kind);
    int first = 1;
    for (int i = 0; i < g_n_sounds; i++) {
        if (g_sounds[i].is_music != music) continue;
        append_header("%s (const SoundBlob *)%s_%s", first ? "" : ",", music ? "music" : "sfx",
                      g_sounds[i].name);
        first = 0;
    }
    append_header(" }\n\n");
}

static void build_tables(void) {
    Tables t = tables_build();
    write_asset_blob("lut_sin_cos", "int16_t lut_sin_cos[1024][2]", t.sin_cos.data, t.sin_cos.len);
    write_asset_blob("lut_recip8", "int32_t lut_recip8[32768]", t.recip8.data, t.recip8.len);
    static const char assets_section[] = "\n    .section .rodata.assets, \"a\"\n";
    buffer_append(&g_asm, assets_section, sizeof assets_section - 1);
    write_asset_blob("lut_recip_clip", "int32_t lut_recip_clip[32768]", t.recip_clip.data, t.recip_clip.len);
    write_asset_blob("lut_recip_sprite", "int32_t lut_recip_sprite[2048]", t.recip_sprite.data,
                     t.recip_sprite.len);

    if (t.recip_area.len <= t.project.len &&
        !memcmp(t.recip_area.data, t.project.data, t.recip_area.len)) {
        write_file(format_string("%s/lut_recip_area.bin", g_out), t.recip_area.data, t.recip_area.len);
        static const char alias[] = "\n    .global lut_recip_area\n    .set lut_recip_area, lut_project\n";
        buffer_append(&g_asm, alias, sizeof alias - 1);
        append_header("extern const int32_t lut_recip_area[32768];\n");
    } else {
        write_asset_blob("lut_recip_area", "int32_t lut_recip_area[32768]", t.recip_area.data,
                         t.recip_area.len);
    }
    write_asset_blob("lut_project", "int32_t lut_project[65536]", t.project.data, t.project.len);
    write_asset_blob("lut_delta", "int32_t lut_delta[32768]", t.delta.data, t.delta.len);
    write_asset_blob("lut_sqrt", "int16_t lut_sqrt[4096]", t.sqrt.data, t.sqrt.len);
    write_asset_blob("lut_atan2", "int16_t lut_atan2[64][64]", t.atan2.data, t.atan2.len);
    write_asset_blob("lut_zbin", "uint8_t lut_zbin[2048]", t.zbin.data, t.zbin.len);
    write_asset_blob("lut_zdrop", "int32_t lut_zdrop[129]", t.zdrop.data, t.zdrop.len);
    write_asset_blob("lut_pow", "int32_t lut_pow[4][33]", t.pow.data, t.pow.len);
}

int main(int argc, char **argv) {
    ASSETC_PROGRESS(0, 0, 1);
    if (argc != 3) die("usage: assetc <assets dir> <output dir>");
    g_in = argv[1];
    g_out = argv[2];

    const char *head = "#ifndef ASSETS_GEN_H\n#define ASSETS_GEN_H\n\n#include \"assets/asset_format.h\"\n\n";
    buffer_append(&g_hdr, head, strlen(head));
    const char *ahead = "    .section .rodata.assets, \"a\"\n";
    buffer_append(&g_asm, ahead, strlen(ahead));

    parse_manifest();
    for (int i = 0; i < object_types_count(); i++) {
        char *upper = xstrdup(object_type_name(i));
        for (char *c = upper; *c; c++) *c = (char)toupper((unsigned char)*c);
        append_header("#define OBJ_%s %d\n", upper, i);
    }
    append_header("#define OBJ_TYPE_COUNT %d\n\n", object_types_count());
    build_tables();
    build_palette();

    size_t max_tris = 0, max_ranks = 0;
    int initial = 0;
    for (int i = 0; i < g_n_stages; i++)
        if (g_stages[i].initial) initial = i;
    for (int i = 0; i < g_n_stages; i++) {
        ASSETC_PROGRESS(1, (uint32_t)i, (uint32_t)g_n_stages);
        build_stage(i, &g_stages[i], &max_tris, &max_ranks);
    }
    append_header("\n#define STAGE_COUNT %d\n#define STAGE_INITIAL %d\n", g_n_stages, initial);
    append_header("#define STAGE_MAX_TRIS %zu\n#define STAGE_MAX_RANKS %zu\n", max_tris, max_ranks);
    append_header("#define STAGE_MAX_POINTS %zu\n", g_max_points);
    append_header("#define STAGE_FAR_MAX_POINTS %zu\n", g_max_far_points);
    append_header("#define STAGE_MAX_OBJECTS %zu\n", g_max_objects);
    append_header("#define STAGE_TEXTURE_LIMIT %zu\n", (g_max_textures + 7) & ~(size_t)7);
    append_header("#define STAGE_MAX_LEAVES %zu\n#define STAGE_MAX_HOME_LEAVES %zu\n", g_max_leaves,
                  g_max_home_leaves);
    append_header("#define STAGE_BLOBS {");
    for (int i = 0; i < g_n_stages; i++) append_header("%s stage_%s", i ? "," : "", g_stages[i].name);
    append_header(" }\n");
    append_header("#define STAGE_FAR_BLOBS {");
    for (int i = 0; i < g_n_stages; i++)
    {
        int far = g_stages[i].far != NULL;
        append_header("%s %s%s%s", i ? "," : "", far ? "stage_" : "NULL",
                      far ? g_stages[i].name : "", far ? "_far" : "");
    }
    append_header(" }\n");
    append_header("#define STAGE_ROCK_DROPS {");
    for (int i = 0; i < g_n_stages; i++)
        append_header("%s %s", i ? "," : "", g_stage_rock_drops[i] ? g_stage_rock_drops[i] : "NULL");
    append_header(" }\n\n");

    append_header("#define STAGE_OCCLUDERS {");
    for (int i = 0; i < g_n_stages; i++) {
        int has = g_stages[i].has_occluder;
        append_header("%s %s%s", i ? "," : "", has ? "occl_" : "NULL", has ? g_stages[i].name : "");
    }
    append_header(" }\n\n");

    for (int i = 0; i < g_n_figures; i++) {
        if ((i & 7) == 0 || i + 1 == g_n_figures)
            ASSETC_PROGRESS(2, (uint32_t)i, (uint32_t)g_n_figures);
        build_figure(&g_figures[i]);
    }
    append_header("#define FIGURE_SCREEN_POINT_LIMIT %zu\n", (g_max_figure_points + 7) & ~(size_t)7);
    append_header("#define FIGURE_MINUS_ORDER_MODELS%s\n", g_minus_order_models ? g_minus_order_models : "");
    /* GWHB runs without the GBA audio engine. Keep sound payloads in their
     * own input section so --gc-sections can omit them from the XIP sidecar
     * when the target supplies the __GNW__ no-op audio implementation. */
    g_asm_section = ".rodata.assets.sound";
    build_sounds(1);
    build_sounds(0);
    g_asm_section = ".rodata.assets";
    printf("effects: %zu near-silent tail samples (|s| <= %d) not stored\n", g_effect_tail_samples,
           SOUND_EFFECT_TAIL_LEVEL);
    for (int i = 0; i < g_n_blobs; i++) {
        ASSETC_PROGRESS(5, (uint32_t)i, (uint32_t)g_n_blobs);
        size_t len;
        char *path = format_string("%s/%s", g_in, g_blob_files[i]);
        uint8_t *data = read_file(path, &len);
        if (!data) die("cannot read %s", path);
        write_asset_blob(g_blob_names[i], format_string("uint8_t %s[]", g_blob_names[i]), data, len);
        append_header("#define %s_SIZE %zu\n", g_blob_names[i], len);
        free(data);
    }

    append_header("\n#endif\n");
    write_file(format_string("%s/assets_gen.h", g_out), g_hdr.data, g_hdr.len);
    write_file(format_string("%s/assets.s", g_out), g_asm.data, g_asm.len);
    ASSETC_PROGRESS(6, 1, 1);
    return 0;
}
