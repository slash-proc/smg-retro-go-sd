#define STAR_GET_STOP      10
#define STAR_GET_LAYOUT    247
#define STAR_GET_WIPE      325
#define STAR_GET_WIPE_LEN  80
#define STAR_GET_CAM_CLOSE 230

typedef struct {
    bool active;
    int t;
    v3 pos, up, front;
    Figure star;
} StarGet;

static StarGet g_star_get EWRAM_BSS;

#define REG_WIN0H   REG16(0x04000040)
#define REG_WIN0V   REG16(0x04000044)
#define REG_WININ   REG16(0x04000048)
#define REG_WINOUT  REG16(0x0400004A)
#define REG_DMA0SAD REG32(0x040000B0)
#define REG_DMA0DAD REG32(0x040000B4)
#define REG_DMA0CNT REG32(0x040000B8)

enum { WIPE_OFF, WIPE_CLOSING, WIPE_OPENING, WIPE_CLOSED };
static struct {
    volatile uint8_t mode;
    int16_t step, frames;
    int32_t r0;
    volatile uint8_t front;
} g_wipe EWRAM_BSS;
static uint16_t g_wipe_lines[2][160] EWRAM_BSS;

static void wipe_start(int mode, int frames){
    g_wipe.mode = (uint8_t)mode;
    g_wipe.step = 0;
    g_wipe.frames = (int16_t)frames;
    g_wipe.r0 = -1;
}

static void wipe_centre(int* cx, int* cy){
    *cx = 120;
    *cy = 80;
    CameraView view;
    camera_build_view(&g_game.camera, &view);
    const Player* p = &g_game.player;
    v3 c = xform_point(&view.to_screen, v3_add(p->figure.position, v3_scale14(p->up, FX(75))));
    if(c.z <= FX(8)) return;
    v2 s = project_to_screen(c);
    *cx = (s.x >> 8) * 2;
    *cy = (s.y >> 8) * 2;
}

static void wipe_update(void){
    if(g_wipe.mode == WIPE_OFF) return;
    int cx, cy;
    wipe_centre(&cx, &cy);
    int32_t r = 0;
    if(g_wipe.mode != WIPE_CLOSED){
        if(g_wipe.r0 < 0){
            int dx = cx > 120 ? cx : 240 - cx, dy = cy > 80 ? cy : 160 - cy;
            g_wipe.r0 = (int32_t)isqrt32((uint32_t)(dx * dx + dy * dy)) + 1;
        }
        fx t = (fx)(g_wipe.step * (FX_PI / 2) / g_wipe.frames);
        if(g_wipe.mode == WIPE_CLOSING) r = g_wipe.r0 * (256 - fx_sin(t)) / 256;
        else r = g_wipe.r0 * (256 - fx_cos(t)) / 256;
    }
    uint16_t* lines = g_wipe_lines[g_wipe.front ^ 1];
    for(int y = 0; y < 160; y++){
        int dy = y - cy;
        int32_t h2 = r * r - dy * dy;
        if(h2 <= 0){
            lines[y] = 0;
            continue;
        }
        int h = (int)isqrt32((uint32_t)h2);
        int l = cx - h, rr = cx + h;
        if(l < 0) l = 0;
        if(rr > 240) rr = 240;
        if(l >= rr){
            lines[y] = 0;
            continue;
        }
        lines[y] = (uint16_t)(l << 8 | rr);
    }
    g_wipe.front ^= 1;
}

static void wipe_after_flip(void){
#ifndef __GNW__
    if(g_wipe.mode == WIPE_OFF) return;
    REG_DISPCNT |= 0x2000;
#endif
}

static void wipe_vblank(void){
#ifndef __GNW__
    REG_DMA0CNT = 0;
    if(g_wipe.mode == WIPE_OFF) return;
    const uint16_t* lines = g_wipe_lines[g_wipe.front];
    REG_WIN0V = 160;
    REG_WININ = 0x0034;
    REG_WINOUT = 0x0000;
    REG_WIN0H = lines[0];
    REG_DMA0SAD = (uint32_t)(lines + 1);
    REG_DMA0DAD = (uint32_t)&REG_WIN0H;
    REG_DMA0CNT = 1 | (0x8000u | 0x2000u | 0x0200u | 0x0040u) << 16;
#endif
}

static void wipe_tick(void){
    if(g_wipe.mode != WIPE_CLOSING && g_wipe.mode != WIPE_OPENING) return;
    if(++g_wipe.step >= g_wipe.frames){
        if(g_wipe.mode == WIPE_CLOSING){
            g_wipe.mode = WIPE_CLOSED;
        } else {
            g_wipe.mode = WIPE_OFF;
#ifndef __GNW__
            REG_DMA0CNT = 0;
            REG_DISPCNT &= (uint16_t)~0x2000;
#endif
        }
    }
}

static Actor* star_get_find_actor(int star_id){
    Game* game = &g_game;
    Actor* best = NULL;
    uint64_t bd = ~0ull;
    for(int i = 0; i < game->actor_count; i++){
        Actor* a = &game->actors[i];
        if(a->type != OBJ_POWERSTAR || a->args[0] != star_id) continue;
        uint64_t d = v3_length_squared64(v3_sub(a->position, game->player.figure.position));
        if(d < bd){
            bd = d;
            best = a;
        }
    }
    return best;
}

static void star_get_start(int star_id){
    level_star_get();
    StarGet* sg = &g_star_get;
    Game* game = &g_game;
    *sg = (StarGet){0};
    sg->active = true;
    Actor* a = star_get_find_actor(star_id);
    const Player* p = &game->player;
    if(a){
        sg->pos = a->position;
        sg->up = a->up;
        sg->front = v3_tangent14(a->front, a->up, p->front);
        figure_init(&sg->star, (const uint8_t*)a->figure.model);
    } else {
        sg->pos = p->figure.position;
        sg->up = p->up;
        sg->front = p->front;
        figure_init(&sg->star, figure_powerstar);
    }

    m3 m = m3_from_basis14(sg->front, sg->up);
    for(int i = 0; i < 3; i++) m.r[i] = v3_scale(m.r[i], 64);
    sg->star.transform.m = m;
    sg->star.transform.o = sg->pos;
    sg->star.position = v3_sub(sg->pos, V3(0, 0, FXC(0.4f)));
    sg->star.transform_dirty = false;
}

static v3 star_get_local(const StarGet* sg, v3 l){
    v3 right = v3_cross14(sg->up, sg->front);
    return v3_add(
        sg->pos, v3_add(v3_add(v3_scale14(right, l.x), v3_scale14(sg->up, l.y)), v3_scale14(sg->front, l.z)));
}

static void star_get_place_star(StarGet* sg, int frame){
    const StarAnimFrame* f = &star_anim_get[frame > STAR_ANIM_GET_FRAMES ? STAR_ANIM_GET_FRAMES : frame];
    v3 right = v3_cross14(sg->up, sg->front);

    v3 col[3];
    for(int k = 0; k < 3; k++){
        v3 a = V3(f->axis[k][0], f->axis[k][1], f->axis[k][2]);
        col[k] = v3_add(v3_add(v3_scale14(right, a.x), v3_scale14(sg->up, a.y)), v3_scale14(sg->front, a.z));
    }

    int32_t k = 64 * f->scale / 256;
    v3 mc[3] = {v3_neg(col[0]), col[2], col[1]};
    m3 m;
    m.r[0] = V3(mc[0].x * k >> 14, mc[1].x * k >> 14, mc[2].x * k >> 14);
    m.r[1] = V3(mc[0].y * k >> 14, mc[1].y * k >> 14, mc[2].y * k >> 14);
    m.r[2] = V3(mc[0].z * k >> 14, mc[1].z * k >> 14, mc[2].z * k >> 14);
    sg->star.transform.m = m;
    sg->star.transform.o = star_get_local(sg, V3(f->t[0], f->t[1], f->t[2]));

    sg->star.position = v3_sub(sg->star.transform.o, V3(0, 0, FXC(0.4f)));
    sg->star.transform_dirty = false;
    sg->star.has_shadow = false;
}

static void star_get_tick(const KeyState* keys){
    StarGet* sg = &g_star_get;
    Game* game = &g_game;
    Player* p = &game->player;
    int t = sg->t++;
    game->scene_timer = t;
    if(t == STAR_GET_STOP) mario_restart_anim(ANIM_MARIO_STARGET);
    if(t >= STAR_GET_STOP){
        int a = t - STAR_GET_STOP;
        int bck = a > 0 ? a - 1 : 0;
        p->figure.position = sg->pos;
        p->up = sg->up;
        p->front = sg->front;
        g_mario.turn_yaw = 0;
        p->has_floor_below = false;
        p->invincible_timer = 0;
        p->anim_frame = bck << 8;
        p->anim_speed = 0;
        star_get_place_star(sg, bck);
    }
    if(t == STAR_GET_WIPE) wipe_start(WIPE_CLOSING, STAR_GET_WIPE_LEN);
    wipe_tick();
    if(t > STAR_GET_WIPE && g_wipe.mode == WIPE_CLOSED){
        sg->active = false;
        game->scene = SCENE_PLAY;
        game_load_stage(STAGE_OBSERVATORY, 1, 0);
        game->fade = 0;
        game->fade_target = 0;
        wipe_start(WIPE_OPENING, 90);
    }
}

static void star_get_wipe_tick(void){
    if(g_game.scene != SCENE_STAR_GET) wipe_tick();
}

static bool star_get_camera(Camera* camera){
    StarGet* sg = &g_star_get;
    if(!sg->active || sg->t <= STAR_GET_STOP) return sg->active;
    int f = sg->t - STAR_GET_STOP - 1;
    if(f > STAR_CAM_FRAMES) f = STAR_CAM_FRAMES;
    const StarCamFrame* c = &star_cam_get[f];
    v3 target = star_get_local(sg, V3(c->target[0], c->target[1], c->target[2]));
    v3 eye = star_get_local(sg, V3(c->eye[0], c->eye[1], c->eye[2]));

    int32_t k = STAR_CAM_FOVY_TAN_X4096 * STAR_GET_CAM_CLOSE / 1697;
    eye = v3_add(target, v3_scale(v3_sub(eye, target), k));

    int32_t d = v3_length_exact(v3_sub(eye, target));
    if(d > 0 && d < FX(200)) eye = v3_add(target, v3_scale(v3_sub(eye, target), (fx)(FX(200) * 256 / d)));
    camera->position = eye;
    camera->target = target;
    camera->front = v3_direction14(v3_sub(target, eye), camera->front);

    v3 up = v3_rotate_axis14(sg->up, camera->front, -(fx)((int64_t)c->twist16 * FX_TAU / (360 * 16)));
    camera->up = v3_tangent14(up, camera->front, sg->up);
    camera->smooth_up = sg->up;
    camera->heading = v3_tangent14(camera->front, sg->up, sg->front);
    camera_update_angles(camera);
    return true;
}

static void star_get_add_figures(void){
    StarGet* sg = &g_star_get;
    Game* game = &g_game;
    if(!sg->active || sg->star.hidden || game->figure_count >= MAX_FIGURES) return;
    game->figures[game->figure_count++] = &sg->star;
}

static int g_star_get_banner_alpha EWRAM_BSS;
static void star_get_draw_hud(void){
    const StarGet* sg = &g_star_get;
    g_star_get_banner_alpha = 16;
    int b = sg->t - STAR_GET_LAYOUT;
    if(b < 0) return;
    if(b > 80) b = 80;
    int first = g_oam_count;
    const HudArt* a = &hud_arts[HUD_ART_T_GETSTAR];
    int cy = HUD_GETSTAR_CY + SS_GETSTAR_FOLLOW_DY + ss_getstar_dy[b];
    hud_art(HUD_ART_T_GETSTAR, 120 - a->w / 2, cy - a->h / 2, SLOT_BANNER);
    g_star_get_banner_alpha = ss_getstar_alpha[b];
    if(g_star_get_banner_alpha < 16)
        for(int i = first; i < g_oam_count; i++) g_oam[i].attr0 |= 0x0400;
}

static void star_get_set_blend(void){
#ifndef __GNW__
    if(g_game.scene != SCENE_STAR_GET || g_star_get_banner_alpha >= 16) return;
    int a = g_star_get_banner_alpha;
    REG_BLDCNT = (uint16_t)((REG_BLDCNT & 0x00FF) | 0x0400);
    REG_BLDALPHA = (uint16_t)(a | (16 - a) << 8);
#endif
}
