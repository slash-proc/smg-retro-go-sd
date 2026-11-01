enum { OCEANBOWL_MAX_POINTS = 64 + 5 };
#define OCEANBOWL_TINT 115
EWRAM_BSS uint8_t g_shade_water[256];
static bool g_oceanbowl_split;

static EWRAM_BSS OceanBowlState g_oceanbowl;
static void oceanbowl_reset(void){
    g_oceanbowl.alpha = 0;
    g_oceanbowl.color = -1;
    g_oceanbowl.depth = 0;
    g_oceanbowl.count = 0;
    int best = 1, best_d = 1 << 30;
    for(int i = 1; i < 256; i++){
        unsigned c = game_palette[i];
        int dr = (int)(c & 31) - 13, dg = (int)((c >> 5) & 31) - 18, db = (int)((c >> 10) & 31) - 24;
        int d = dr * dr + dg * dg + db * db;
        if(d < best_d){
            best_d = d;
            best = i;
        }
    }
    g_oceanbowl.blue = (uint8_t)best;

    unsigned w = game_palette[best];
    int wr = (int)(w & 31) * OCEANBOWL_TINT, wg = (int)((w >> 5) & 31) * OCEANBOWL_TINT,
        wb = (int)((w >> 10) & 31) * OCEANBOWL_TINT;
    for(int i = 0; i < 256; i++){
        unsigned c = game_palette[i];
        int r = (int)(c & 31) * (256 - OCEANBOWL_TINT) + wr,
            g = (int)((c >> 5) & 31) * (256 - OCEANBOWL_TINT) + wg,
            b = (int)((c >> 10) & 31) * (256 - OCEANBOWL_TINT) + wb;
        int pick = 1, pick_d = 1 << 30;
        for(int k = 1; k < 256; k++){
            unsigned q = game_palette[k];
            int dr = ((int)(q & 31) << 8) - r, dg = ((int)((q >> 5) & 31) << 8) - g,
                db = ((int)((q >> 10) & 31) << 8) - b;
            int d = (dr >> 4) * (dr >> 4) + (dg >> 4) * (dg >> 4) + (db >> 4) * (db >> 4);
            if(d < pick_d){
                pick_d = d;
                pick = k;
            }
        }
        g_shade_water[i] = (uint8_t)pick;
    }
}
static void oceanbowl_tick(void){
    if(g_game.camera_underwater) g_oceanbowl.alpha = (uint8_t)fx_min(g_oceanbowl.alpha + 20, 255);
    else g_oceanbowl.alpha = (uint8_t)fx_max(g_oceanbowl.alpha - 20, 0);
}

__attribute__((noinline)) static void oceanbowl_color_filter(void){
    int color = 0;
    if(level_has(LEVEL_OCEAN_BOWL) && g_oceanbowl.alpha){
        WaterSample w;

        if(g_game.camera_underwater && water_query(g_game.camera.position, g_game.player.gravity_up, &w))
            g_oceanbowl.depth = (uint16_t)fx_clamp(w.depth, 0, 3000);
        color = (int)((int64_t)50 * g_oceanbowl.depth * g_oceanbowl.alpha / (3000 * 255 * 8));
    }
    if(color == g_oceanbowl.color) return;
    for(unsigned i = 0; i < 256; i++){
        unsigned c = game_palette[i];
        unsigned r = (unsigned)fx_max((int)(c & 31) - color, 0),
                 g = (unsigned)fx_max((int)((c >> 5) & 31) - color, 0);
        BG_PALETTE[i] = (uint16_t)(r | (g << 5) | (c & 0x7c00));
    }
    g_oceanbowl.color = (int8_t)color;
}

static inline int oceanbowl_plane_distance(v3 v, int plane){
    switch(plane){
    case 0: return v.z - CLIP_NEAR_Z;
    case 1: return v.z + v.y;
    case 2: return v.z - v.y;
    case 3: return v.z - v.x;
    default: return v.z + v.x;
    }
}
static int oceanbowl_clip(FramePoint* out, const FramePoint* in, int count, int plane){
    int n = 0;
    v3 a = in[count - 1].position;
    int da = oceanbowl_plane_distance(a, plane);
    for(int i = 0; i < count && n < OCEANBOWL_MAX_POINTS - 1; i++){
        v3 b = in[i].position;
        int db = oceanbowl_plane_distance(b, plane);
        if((da >= 0) != (db >= 0)){
            int64_t t = ((int64_t)da << 16) / (da - db);
            out[n++].position = V3(a.x + (int)(((int64_t)(b.x - a.x) * t) >> 16),
                                   a.y + (int)(((int64_t)(b.y - a.y) * t) >> 16),
                                   a.z + (int)(((int64_t)(b.z - a.z) * t) >> 16));
        }
        if(db >= 0) out[n++].position = b;
        a = b;
        da = db;
    }
    return n;
}

static void oceanbowl_begin(void){
    g_oceanbowl_split = level_has(LEVEL_OCEAN_BOWL) &&
                        v3_length_squared64(g_game.player.figure.position) <= (int64_t)7340 * 7340;
    if(!g_oceanbowl_split) g_oceanbowl.count = 0;
}

static const int16_t oceanbowl_ring[3][2] = {{3170, -5491}, {5491, -3170}, {6340, 0}};
static const int16_t oceanbowl_sector[8][2] = {{16384, 0},  {11585, 11585},   {0, 16384},  {-11585, 11585},
                                               {-16384, 0}, {-11585, -11585}, {0, -16384}, {11585, -11585}};
static void oceanbowl_queue_bowl(const CameraView* view){
    v3 p[25];
    uint32_t clip[25];
    p[0] = xform_point(&view->to_screen, V3(0, 0, -6340));
    for(int r = 0; r < 3; r++)
        for(int k = 0; k < 8; k++)
            p[1 + r * 8 + k] =
                xform_point(&view->to_screen,
                            V3((oceanbowl_ring[r][0] * oceanbowl_sector[k][0]) >> 14,
                               (oceanbowl_ring[r][0] * oceanbowl_sector[k][1]) >> 14, oceanbowl_ring[r][1]));
    for(int i = 0; i < 25; i++) clip[i] = view_clip_mask(p[i]);
    for(int r = 0; r < 3; r++)
        for(int k = 0; k < 8; k++){
            int k1 = (k + 1) % 8;
            int a = 1 + r * 8 + k, b = 1 + r * 8 + k1;
            int tri[2][3] = {{r ? 1 + (r - 1) * 8 + k : 0, a, b},
                             {r ? 1 + (r - 1) * 8 + k : 0, b, r ? 1 + (r - 1) * 8 + k1 : 0}};
            for(int f = 0; f < (r ? 2 : 1); f++){
                const int* t = tri[f];
                if(clip[t[0]] & clip[t[1]] & clip[t[2]] & CLIP_SIDES) continue;
                fx z = fx_max(p[t[0]].z, fx_max(p[t[1]].z, p[t[2]].z));
                if(z <= 0) continue;
                int bin = depth_bin_for_z(z);
                if(bin < g_farthest_visible_bin) bin = g_farthest_visible_bin;
                ClipVertex c[3] = {{p[t[0]], 0, 0}, {p[t[1]], 0, 0}, {p[t[2]], 0, 0}};
                engine_emit_clipped(bin + BIN_COUNT, c, RM_FLAT, g_oceanbowl.blue, NULL);
            }
        }
}

__attribute__((noinline)) static void oceanbowl_queue(const CameraView* view){
    g_oceanbowl.count = 0;
    if(!g_oceanbowl_split) return;
    oceanbowl_queue_disc(view);
    oceanbowl_queue_bowl(view);
    oceanbowl_color_filter();
}
static void oceanbowl_queue_disc(const CameraView* view){
    const m3* vm = &view->view;
    int spread = ((fx_abs(vm->r[0].z) * 160 + fx_abs(vm->r[1].z) * 107 + 255) >> 8) + 3;
    int vertical = vm->r[2].z;
    if((g_game.camera.position.z > 0 && vertical - spread > 0) ||
       (g_game.camera.position.z < 0 && vertical + spread < 0))
        return;
    int base = g_point_count + FAR_POINTS_RESERVE;
    if(base + 2 * OCEANBOWL_MAX_POINTS > MAX_POINTS) return;
    FramePoint* a = g_frame_points + base;
    FramePoint* b = a + OCEANBOWL_MAX_POINTS;
    for(int i = 0; i < 64; i++)
        a[i].position =
            xform_point(&view->to_screen, V3(oceanbowl_outline[i][0], oceanbowl_outline[i][1], 0));
    int count = 64;
    for(int plane = 0; plane < 5 && count >= 3; plane++){
        count = oceanbowl_clip(b, a, count, plane);
        FramePoint* t = a;
        a = b;
        b = t;
    }
    if(count < 3) return;

    if(a != g_frame_points + base)
        for(int i = 0; i < count; i++) g_frame_points[base + i].position = a[i].position;
    a = g_frame_points + base;
    g_point_count = base + count;
    g_oceanbowl.base = (uint16_t)base;

    for(int i = 0; i < count; i++)
        g_screen_positions[base + i] = screen_pack_position(project_to_screen(a[i].position));
    g_oceanbowl.count = (uint8_t)count;
}

__attribute__((noinline)) static void oceanbowl_draw_surface(void){
    const uint32_t* screen = g_screen_positions + g_oceanbowl.base;
    for(int i = 1; i + 1 < g_oceanbowl.count; i++){
        uint32_t p0 = screen[0], p1 = screen[i], p2 = screen[i + 1];
        int area = screen_area(p0, p1, p2);
        if(!area) continue;
        if(area < 0){
            uint32_t t = p1;
            p1 = p2;
            p2 = t;
        }
        const FramePoint* points = g_frame_points + g_oceanbowl.base;
        engine_set_depth(points[0].position.z, points[area < 0 ? i + 1 : i].position.z,
                         points[area < 0 ? i : i + 1].position.z);
        engine_out(-1, p0, p1, p2, 0, 0, 0, RM_TINT, 0, NULL);
    }
}
