#ifndef ASSETC_MODEL_BUILDER_H
#define ASSETC_MODEL_BUILDER_H

#include "../../src/assets/asset_format.h"
#include "palette.h"
#include "common.h"

typedef int16_t Point3[3];
typedef uint16_t Rank6[6];
typedef int IntPair[2];

typedef struct {
    const char *foreground, *background;
} TextureUnderlay;

enum { TEXTURE_ALPHA_OPAQUE, TEXTURE_ALPHA_COVERAGE, TEXTURE_ALPHA_BLEND_QUARTER };
typedef struct {
    const char *texture;
    int mode;
} TextureAlphaPolicy;

typedef struct {
    int16_t offset;
    uint16_t tri;
} AxisEntry;

typedef struct {
    ModelVertex v[3];
    int16_t normal[3];
    uint16_t texture;
    uint16_t water_texture;
    uint16_t flags;
    int16_t uv_scroll_speed[2];
    uint8_t ledges;
    uint8_t pad;
    uint8_t occl;
} BuildTriangle;

typedef struct {
    Vector points;
    Vector triangles;
    Vector source_codes;
    Vector textures;
    Vector chunks;
    Vector leaf_triangles;
    Vector leaf_triangle_offsets;
    Vector tri_axes;
    Vector tri_rank;
    Vector rank_segments;
    Vector draw_axes[6];
    Vector part_leaves;
    Vector leaf_occl;

    int occl_n;
    float occl_x0, occl_y0, *occl_above, *occl_below;
} Model;

typedef struct {
    const char *dir;
    const char *obj;
    const TextureUnderlay *texture_underlays;
    int texture_underlay_count;
    const TextureAlphaPolicy *texture_alpha_policies;
    int texture_alpha_policy_count;
    float shading_base, shading;
    float sun[3];
    int underwater;
    int is_figure;
    void (*light_up)(void *ctx, const float position[3], float up[3]);
    void *light_ctx;
    int point_shift;
    const char *const *part_names;
    int part_count;
    int source_zero_faces;

    const float *occluder;
} ModelParams;

Model model_build(const ModelParams *p, const Rgba *palette);

Vector model_load_positions(const char *path);

int model_point_shift(const char *path);

Texture texture_from_image(const char *path, int mip1_radius, const Rgba *palette);

#endif
