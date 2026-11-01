#ifndef GALAXY_ASSET_FORMAT_H
#define GALAXY_ASSET_FORMAT_H

#include <stdint.h>
#include <stddef.h>

#define TEX_DIM      32
#define TEX_TEXELS   (TEX_DIM * TEX_DIM)
#define SKY_DIM      256
#define SKY_W        128

#define CHUNK_INNER  0xFFFF
#define CONE_NONE    0x7FFFFFFF

#define TEX_MIP1_ROWS   16
#define TEX_MIP1_STRIDE 64
typedef struct {
    uint8_t mip[2][TEX_TEXELS];
} Texture;

typedef struct {
    uint32_t mip[2];
} TextureReference;

/* Packed far records retain their legacy average in the low byte. An optional
 * detail payload uses bits8..29 for its aligned source offset divided by four.
 * Old readers still see the same average; canonical packers build the actual
 * runtime mip pointers, so runtime code never interprets the source offset. */
#define FAR_TEXTURE_AVERAGE UINT32_C(0x80000000)
#define FAR_TEXTURE_DETAIL  UINT32_C(0x40000000)
static inline int far_texture_has_pixels(uint32_t record){
    return !(record & FAR_TEXTURE_AVERAGE) || (record & FAR_TEXTURE_DETAIL);
}
static inline uint32_t far_texture_source_offset(uint32_t record){
    return record & FAR_TEXTURE_AVERAGE ? (record & UINT32_C(0x3fffff00)) >> 6 : record;
}

typedef struct {
    uint16_t point;
    int16_t u, v;
} ModelVertex;

enum {
    SURF_FLOOR   = 1 << 0,
    SURF_UNSOLID = 1 << 1,
    SURF_ANIM_UV = 1 << 2,
    TRI_ALPHA    = 1 << 3,
    SURF_DEATH   = 1 << 4,
    SURF_SLIPPERY = 1 << 5,
    SURF_HONEY   = 1 << 6,
    SURF_NOCAMERA = 1 << 7,
    SURF_WATER   = 1 << 8,
    SURF_NOT_GRAB = 1 << 9,
    SURF_SCENARIO_MASK = 0xfc00,
};

#define TRI_LEDGE_WATER_TINT 8
#define TRI_SOURCE_ICE 16
#define TRI_SOURCE_BURN 32
#define TRI_SOURCE_BLEND_QUARTER 64

typedef struct {
    ModelVertex v[3];
    uint16_t texture;
    uint8_t flags;
    int8_t normal[3];
} StageTriangle;
#define STAGE_TEXTURE_MASK 0x3ff
#define STAGE_MAX_TEXTURE_NUMBERS 1024
#define TRI_WATER_TINT (1 << 0)
/* Render-only flag; BuildTriangle uses its source marker in ledges instead
 * of this bit, which overlaps SURF_DEATH in the separate collision flags. */
#define TRI_BLEND_QUARTER (1 << 4)
static inline unsigned stage_triangle_texture(const StageTriangle *t) {
    return t->texture & STAGE_TEXTURE_MASK;
}

typedef struct {
    int16_t min[3], max[3];
    uint16_t skip;
    uint16_t leaf;
    int8_t cone_normal[3];
    uint8_t cone_normal_extent[3];
    int16_t cone_center[3];
    int32_t cone_plane_limit;
} StageChunk;

typedef struct {
    uint16_t points[3];
    int16_t a[3];
    int16_t normal[3];
    uint16_t pad;
} LeafTriangle;

enum {
    GRAV_POINT,
    GRAV_CUBE,
    GRAV_PLANE,
    GRAV_PLANE_BOX,
    GRAV_PLANE_CYLINDER,
    GRAV_DISK,
    GRAV_SEGMENT,
    GRAV_DISK_TORUS,
    GRAV_WIRE,
};

enum {
    GRAVF_INVERSE    = 1 << 0,
    GRAVF_BOTH_SIDES = 1 << 1,
    GRAVF_EDGE       = 1 << 2,
    GRAVF_EDGE0      = 1 << 3,
    GRAVF_EDGE1      = 1 << 4,
    GRAVF_LIGHT      = 1 << 5,
    GRAVF_LEGACY     = 1 << 6,
};

typedef struct {
    uint8_t kind, flags;
    uint8_t faces;
    uint8_t scenarios;
    int16_t priority;
    int16_t valid_cos;
    int32_t centre[3];
    int16_t axis[3][3];
    int16_t ref_side[3];
    int32_t extent[3];
    int32_t range;
    int32_t distant;
    int32_t base;
    int16_t switch_a, switch_b, switch_appear, switch_sleep;
    int32_t bound;
} Gravity;

typedef struct {
    int32_t origin[3];
    uint16_t dims[3];
    uint8_t shift, block_shift;
    uint32_t cells;
    uint32_t lists;
    uint16_t outside;
    uint16_t top_count;
} GravityGrid;

enum {
    COL_FLOOR_ONLY = 1 << 0,
    COL_DEATH      = 1 << 1,
    COL_SLIPPERY   = 1 << 2,
    COL_WATER      = 1 << 3,
    COL_NO_CAMERA  = 1 << 4,
    COL_HONEY      = 1 << 5,
    COL_NOT_GRAB   = 1 << 6,
    COL_ICE        = 1 << 7,
    COL_BURN       = 1 << 8,
    COL_NO_WALL_JUMP = 1 << 9,
    COL_SCENARIO_MASK = 0xfc00,
};

typedef struct {
    int16_t v[3][3];
    int16_t normal[3];
    uint16_t attr;
    uint8_t floor_code, wall_code;
    int32_t plane;
} CollisionTriangle;

typedef struct {
    uint16_t point[3];
    int16_t normal[3];
    uint16_t attr;
    uint8_t floor_code, wall_code;
} CollisionFace;

typedef struct {
    int16_t min[3], max[3];
    uint16_t next;
    uint8_t count;
    uint8_t part;
} CollisionNode;

typedef struct {
    uint16_t type;
    uint8_t scenarios;
    uint8_t flags;
    int32_t position[3];
    int16_t up[3];
    int16_t front[3];
    int32_t target[3];
    int16_t args[8];
    int16_t rail;
    int16_t camera;
    uint16_t id;
    uint8_t zone, pad;
    int16_t scale[3];
    int16_t sw[5];
    int16_t pad2;
} StageObject;
enum { SWITCH_A, SWITCH_B, SWITCH_APPEAR, SWITCH_DEAD, SWITCH_SLEEP };

#define STAGE_DIRECT_POINTS      2944
#define STAGE_FAR_DIRECT_POINTS  767
#define STAGE_FAR_POINT_LIMIT    1535

typedef struct {
    int32_t position[3];
    int16_t up[3];
    int16_t front[3];
    int16_t id;
    uint8_t scenarios, zone;
    int16_t camera;
    int16_t pad;
} StageStart;

typedef struct {
    int32_t position[3];
    int16_t args[8];
} StageRailPoint;

typedef struct {
    int32_t p[4][3];
    int32_t start, length;
    uint16_t lut[16];
} StageRailPart;

#define ROCK_DROP_STEP 128
#define ROCK_DROP_UNIT 4
#define ROCK_DROP_NONE 255

typedef struct {
    uint16_t first_point, point_count;
    uint16_t first_part, part_count;
    uint8_t closed, zone;
    int16_t args[8];
    uint16_t pad;
    int32_t length;
} StageRail;

enum { AREA_CAMERA, AREA_DEATH, AREA_RESTART, AREA_BLACKHOLE, AREA_SWITCH, AREA_TOWER };
enum { SHAPE_BOX, SHAPE_SPHERE, SHAPE_CYLINDER, SHAPE_BOWL };

typedef struct {
    uint8_t cls, shape, scenarios, zone;
    int16_t args[8];
    int16_t camera;
    int16_t priority;
    int32_t centre[3];
    int16_t axis[3][3];
    int16_t sw;
    int32_t extent[3];
    int32_t bound;
} StageArea;

enum {
    CAM_XZ_PARA, CAM_WONDER_PLANET, CAM_FOLLOW, CAM_POINT_FIX, CAM_EYEPOS_FIX, CAM_EYEPOS_FIX_THERE,
    CAM_CHARMED_FIX, CAM_TOWER, CAM_TOWER_POS, CAM_OBJ_PARALLEL, CAM_CUBE_PLANET, CAM_FRONT_AND_BACK,
    CAM_TRUNDLE, CAM_MEDIAN_PLANET, CAM_WATER_FOLLOW, CAM_SPIRAL, CAM_CHARMED_VECREG_TOWER, CAM_TYPE_COUNT
};

typedef struct {
    uint8_t type, vpan;
    int16_t camint, evfrm, num1, num2;
    int16_t angle_a, angle_b, roll;
    int16_t upper, lower;
    int16_t fovy;
    int16_t zone_up[3];
    int16_t extra[3];
    uint8_t flags;
    uint8_t pad;
    int32_t dist;
    int32_t offset[3];
    int32_t wpoint[3];
    int32_t axis[3];
    int32_t up[3];
    int32_t woffset[3];
    int32_t loffset, loffsetv;
    int32_t uplay, lplay;
    int32_t camendint;
} StageCamera;

enum {
    CAMF_ERP_FRAME     = 1 << 0,
    CAMF_END_ERP_FRAME = 1 << 1,
    CAMF_ANTIBLUR_OFF  = 1 << 2,
    CAMF_NO_RESET      = 1 << 3,
};

enum { STAGE_SCENARIO_SURFACES = 1 << 0, STAGE_INDEXED_LEAVES = 1 << 2, STAGE_PACKED_FAR_TEXTURES = 1 << 3 };

typedef struct {
    uint32_t point_count, triangle_count, texture_count, chunk_count;
    uint32_t leaf_count, leaf_triangle_count, rank_count;

    uint32_t points;
    uint32_t triangles;
    uint32_t textures;
    uint32_t chunks;
    uint32_t leaf_triangle_offsets;
    uint32_t leaf_triangles;
    uint32_t leaf_draw_ranks;
    uint32_t rank_triangles;
    uint32_t texture_water;
    uint32_t texture_scroll;
    uint32_t sky;

    uint32_t gravity_count, gravities;
    uint32_t collision_triangle_count, collision_triangles;
    uint32_t collision_node_count, collision_nodes;
    uint32_t object_count, objects;
    uint32_t start_count, starts;
    uint32_t rail_count, rails, rail_points, rail_parts;
    uint32_t area_count, areas;
    uint32_t camera_count, cameras;
    int32_t death_height;
    uint8_t has_water, has_water_region, music, point_shift;
    int32_t water_level;
    int32_t water_centre[2];
    int32_t water_radius_squared;
    int32_t water_min_z;
    uint32_t galaxy;

    uint32_t point_home, leaf_point_first;
    uint16_t leaf_slot_max, surface_features;
    uint32_t gravity_grid;
    uint32_t area_grid;
    uint32_t part_count, parts;
} StageHeader;

typedef struct {
    uint16_t leaf_first, leaf_count;
    int16_t sw;
    uint8_t mode;
    uint8_t pad;
} StagePart;
enum { PART_SHOW_WHEN_ON, PART_HIDE_WHEN_ON };

typedef struct {
    int32_t x0, y0, base;
    uint8_t n, range_count, pad[2];
} StageOccluder;
typedef struct {
    uint16_t first, count;
    uint8_t cls, pad[3];
} StageOccluderRange;
#define STAGE_MAX_PARTS 16

#define FIGURE_TRANSFORM_MAX_POINTS 384
#define FIGURE_TRAPEZE_DRAW_POINTS 12
#define FIGURE_PROCEDURAL_MAX_POINTS FIGURE_TRAPEZE_DRAW_POINTS

typedef struct {
    ModelVertex v[3];
    uint16_t texture;
    uint16_t flags;
} FigureTriangle;

typedef struct {
    uint16_t first_key, key_count;
    uint8_t ticks_per_key, loop;
    uint16_t pad;
} AnimClip;

#define FIGURE_PACKED_KEYS 0x8000u
#define FIGURE_KEY_COUNT 0x7fffu
#define FIGURE_POSE_TAG 1u
#define FIGURE_POSE_BLOCK 32
#define FIGURE_PACKED_MAX_POINTS 224
#define FIGURE_POSE_RAW 0
#define FIGURE_POSE_DELTA32_SCALE8 3
#define FIGURE_POSE_SCALE4 0x8000

typedef struct {
    uint16_t point_count, mode;
    int32_t bind_relative;
} PackedPoseHeader;

#define FIGURE_DRAW_AXES 3

typedef struct {
    uint16_t point_count, triangle_count, texture_count, keyframe_count;
    uint32_t points;
    uint32_t keyframes;
    uint32_t triangles;
    uint32_t axes;
    uint32_t textures;
    uint32_t clip_count, clips;

    uint16_t lod_triangle_count, lod_texture_count;
    uint32_t lod_triangles, lod_axes, lod_textures;
    uint32_t lod_point_count, lod_points;
    uint32_t extent;
} FigureHeader;

#define SOUND_SAMPLES_PER_REFRESH 304
#define SOUND_MUSIC_SAMPLES_PER_REFRESH 192
#define SOUND_EFFECT_SAMPLES_PER_REFRESH 228
#define SOUND_BLOCK 32
#define SOUND_BLOCK_BYTES 17
#define SOUND_FILTERS { {0, 0}, {420, -183}, {456, -217}, {259, -130}, {332, -190}, {303, -75}, \
    {423, -225}, {179, 22}, {482, -242}, {385, -151}, {102, 90}, {344, -117}, {175, -183}, {-89, -191}, \
    {247, -32}, {95, -80} }

typedef struct {
    uint32_t length;
    uint32_t loop_start;
    int16_t loop_s1, loop_s2;
    uint8_t blocks[];
} SoundBlob;

_Static_assert(sizeof(Texture) == 2048, "Texture size");
_Static_assert(sizeof(TextureReference) == 8, "TextureReference size");
_Static_assert(sizeof(ModelVertex) == 6, "ModelVertex size");
_Static_assert(sizeof(StageTriangle) == 24 && offsetof(StageTriangle, texture) == 18 &&
               offsetof(StageTriangle, flags) == 20 && offsetof(StageTriangle, normal) == 21,
               "StageTriangle layout");
_Static_assert(sizeof(StageChunk) == 32 && offsetof(StageChunk, cone_center) == 22 &&
               offsetof(StageChunk, cone_plane_limit) == 28, "StageChunk layout");
_Static_assert(sizeof(LeafTriangle) == 20, "LeafTriangle size");
_Static_assert(sizeof(FigureTriangle) == 22 && offsetof(FigureTriangle, texture) == 18,
               "FigureTriangle layout");
_Static_assert(sizeof(PackedPoseHeader) == 8, "PackedPoseHeader size");
_Static_assert(sizeof(Gravity) == 80 && offsetof(Gravity, extent) == 44, "Gravity layout");
_Static_assert(sizeof(GravityGrid) == 32 && offsetof(GravityGrid, block_shift) == 19 &&
               offsetof(GravityGrid, cells) == 20 && offsetof(GravityGrid, outside) == 28,
               "GravityGrid layout");
_Static_assert(sizeof(CollisionTriangle) == 32 && offsetof(CollisionTriangle, plane) == 28,
               "CollisionTriangle layout");
_Static_assert(sizeof(CollisionFace) == 16, "CollisionFace size");
_Static_assert(sizeof(CollisionNode) == 16, "CollisionNode size");
_Static_assert(sizeof(StageObject) == 84, "StageObject size");
_Static_assert(sizeof(StageStart) == 32, "StageStart size");
_Static_assert(sizeof(StageRail) == 32, "StageRail size");
_Static_assert(sizeof(StageRailPoint) == 28, "StageRailPoint size");
_Static_assert(sizeof(StageRailPart) == 88, "StageRailPart size");
_Static_assert(sizeof(StageArea) == 72, "StageArea size");
_Static_assert(sizeof(StageCamera) == 120, "StageCamera size");
_Static_assert(sizeof(StagePart) == 8, "StagePart size");
_Static_assert(sizeof(StageHeader) == 204 && offsetof(StageHeader, textures) == 36 &&
               offsetof(StageHeader, sky) == 68 && offsetof(StageHeader, gravity_grid) == 188 &&
               offsetof(StageHeader, parts) == 200, "StageHeader layout");
_Static_assert(sizeof(FigureHeader) == 64 && offsetof(FigureHeader, textures) == 24 &&
               offsetof(FigureHeader, lod_textures) == 48 && offsetof(FigureHeader, lod_points) == 56,
               "FigureHeader layout");

#endif
