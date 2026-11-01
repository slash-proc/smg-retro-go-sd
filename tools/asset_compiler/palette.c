#include "palette.h"
#include "common.h"

#include <float.h>
#include <math.h>

int palette_nearest_opaque(const Rgba *pal, const float col[3]) {
    int32_t c[3] = { float_to_i32_saturated(col[0]), float_to_i32_saturated(col[1]),
                     float_to_i32_saturated(col[2]) };
    int best = 1;
    int32_t best_d = INT32_MAX;
    for (int i = 1; i < 256; i++) {
        int32_t dr = pal[i].r - c[0], dg = pal[i].g - c[1], db = pal[i].b - c[2];
        int32_t d = dr * dr + dg * dg + db * db;
        if (d < best_d) {
            best_d = d;
            best = i;
        }
    }
    return best;
}

/* Indexed as background*256 + foreground. Transparent foreground remains a
 * no-op; solid texels use nearest-palette25% source/75% destination colours. */
void palette_build_quarter_blend(uint8_t out[65536], const Rgba pal[256]) {
    for (int background = 0; background < 256; background++) {
        out[background * 256] = (uint8_t)background;
        for (int foreground = 1; foreground < 256; foreground++) {
            int r = 3 * pal[background].r + pal[foreground].r;
            int g = 3 * pal[background].g + pal[foreground].g;
            int b = 3 * pal[background].b + pal[foreground].b;
            int best = 1, best_distance = INT32_MAX;
            for (int candidate = 1; candidate < 256; candidate++) {
                int dr = 4 * pal[candidate].r - r;
                int dg = 4 * pal[candidate].g - g;
                int db = 4 * pal[candidate].b - b;
                int distance = dr * dr + dg * dg + db * db;
                if (distance < best_distance) { best_distance = distance; best = candidate; }
            }
            out[background * 256 + foreground] = (uint8_t)best;
        }
    }
}

typedef struct {
    double r, g, b, a;
} Colorf;

static const double GAMMA = 1.145, TRANSPARENCY_SCALE = 0.01;
static const Colorf SCALE = { 1.0, 1.2, 0.8, 0.75 };

static double color_dot(Colorf x, Colorf y) { return x.r * y.r + x.g * y.g + x.b * y.b + x.a * y.a; }
static Colorf color_add(Colorf x, Colorf y) { return (Colorf){ x.r + y.r, x.g + y.g, x.b + y.b, x.a + y.a }; }
static Colorf color_sub(Colorf x, Colorf y) { return (Colorf){ x.r - y.r, x.g - y.g, x.b - y.b, x.a - y.a }; }
static Colorf color_mul(Colorf x, Colorf y) { return (Colorf){ x.r * y.r, x.g * y.g, x.b * y.b, x.a * y.a }; }
static Colorf color_scale(Colorf x, double s) { return (Colorf){ x.r * s, x.g * s, x.b * s, x.a * s }; }

static Colorf color_pow(Colorf x, double e) {
    return (Colorf){ pow(fmax(x.r, 0.0), e), pow(fmax(x.g, 0.0), e), pow(fmax(x.b, 0.0), e),
                     pow(fmax(x.a, 0.0), e) };
}

static Colorf color_to_perceptual(Rgba c) {
    Colorf f = { c.r / 255.0, c.g / 255.0, c.b / 255.0, c.a / 255.0 };
    f = color_mul(color_pow(f, GAMMA), SCALE);
    double k = f.a * (1.0 - TRANSPARENCY_SCALE) + TRANSPARENCY_SCALE;
    f.r *= k;
    f.g *= k;
    f.b *= k;
    return f;
}

typedef struct KdNode {
    Colorf mid;
    int index;
    Colorf normal;
    struct KdNode *left, *right;
} KdNode;

struct ColorMap {
    KdNode *root;
};

typedef struct {
    const Colorf *colors;
    Colorf normal;
} SortCtx;

static int compare_color_axis(const void *a, const void *b, void *ctx) {
    const SortCtx *s = ctx;
    double x = color_dot(s->colors[*(const int *)a], s->normal);
    double y = color_dot(s->colors[*(const int *)b], s->normal);
    return x < y ? -1 : x > y;
}

static KdNode *color_tree_build(int *idx, int n, const Colorf *colors) {
    Colorf sum = { 0 }, sum2 = { 0 };
    for (int i = 0; i < n; i++) {
        Colorf c = colors[idx[i]];
        sum = color_add(sum, c);
        sum2 = color_add(sum2, color_mul(c, c));
    }
    Colorf var = color_sub(sum2, color_scale(color_mul(sum, sum), 1.0 / n));
    Colorf normal = { 0 };
    if (var.r > var.g && var.r > var.b && var.r > var.a) normal.r = 1.0;
    else if (var.g > var.b && var.g > var.a) normal.g = 1.0;
    else if (var.b > var.a) normal.b = 1.0;
    else normal.a = 1.0;

    SortCtx ctx = { colors, normal };
    stable_sort(idx, (size_t)n, sizeof(int), compare_color_axis, &ctx);

    int mid = n / 2;
    KdNode *node = xcalloc(1, sizeof *node);
    node->mid = colors[idx[mid]];
    node->index = idx[mid];
    node->normal = normal;
    if (mid > 0) node->left = color_tree_build(idx, mid, colors);
    if (mid + 1 < n) node->right = color_tree_build(idx + mid + 1, n - mid - 1, colors);
    return node;
}

static int color_tree_nearest(const KdNode *node, Colorf needle, double limit, int *index, double *dist) {
    int found = 0;
    Colorf diff = color_sub(needle, node->mid);
    double d = sqrt(color_dot(diff, diff));
    if (d < limit) {
        limit = d;
        *index = node->index;
        *dist = d;
        found = 1;
    }
    double dot = color_dot(diff, node->normal);
    const KdNode *near = dot <= 0.0 ? node->left : node->right;
    const KdNode *far = dot <= 0.0 ? node->right : node->left;
    double reach = dot <= 0.0 ? -dot : dot;
    int i;
    double di;
    if (near && color_tree_nearest(near, needle, limit, &i, &di)) {
        limit = di;
        *index = i;
        *dist = di;
        found = 1;
    }
    if (reach < limit && far && color_tree_nearest(far, needle, limit, &i, &di)) {
        *index = i;
        *dist = di;
        found = 1;
    }
    return found;
}

ColorMap *colormap_create(const Rgba *pal, int n) {
    Colorf *colors = xmalloc(sizeof(Colorf) * n);
    int *idx = xmalloc(sizeof(int) * n);
    for (int i = 0; i < n; i++) {
        colors[i] = color_to_perceptual(pal[i]);
        idx[i] = i;
    }
    ColorMap *map = xcalloc(1, sizeof *map);
    map->root = color_tree_build(idx, n, colors);
    free(idx);
    free(colors);
    return map;
}

int colormap_nearest_rgb(const ColorMap *map, uint8_t r, uint8_t g, uint8_t b) {
    int index = 0;
    double dist;
    color_tree_nearest(map->root, color_to_perceptual((Rgba){ r, g, b, 255 }), DBL_MAX, &index, &dist);
    return index;
}
