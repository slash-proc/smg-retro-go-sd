static const uint8_t mission_kinds[GALAXY_COUNT][6] = {
    [GALAXY_HONEYHIVE] = {MISSION_NORMAL, MISSION_NORMAL, MISSION_NORMAL, MISSION_COMET_GHOST,
                          MISSION_COMET_PURPLE, MISSION_HIDDEN},
    [GALAXY_FREEZEFLAME] = {MISSION_NORMAL, MISSION_NORMAL, MISSION_NORMAL, MISSION_COMET_GHOST,
                            MISSION_COMET_PURPLE, MISSION_HIDDEN},
    [GALAXY_BUOYBASE] = {MISSION_NORMAL, MISSION_HIDDEN},
    [GALAXY_BEACHBOWL] = {MISSION_NORMAL, MISSION_NORMAL, MISSION_NORMAL, MISSION_COMET_QUICK,
                          MISSION_COMET_PURPLE, MISSION_HIDDEN},
    [GALAXY_GOODEGG] = {MISSION_NORMAL, MISSION_NORMAL, MISSION_NORMAL, MISSION_COMET_RED,
                        MISSION_COMET_PURPLE, MISSION_HIDDEN},
    [GALAXY_DEEPDARK] = {MISSION_NORMAL, MISSION_NORMAL, MISSION_NORMAL, MISSION_COMET_DARK,
                         MISSION_COMET_PURPLE, MISSION_HIDDEN},
    [GALAXY_GUSTYGARDEN] = {MISSION_NORMAL, MISSION_NORMAL, MISSION_NORMAL, MISSION_COMET_DARK,
                            MISSION_COMET_PURPLE, MISSION_HIDDEN},
};
static const uint8_t ss_galaxy_art[GALAXY_COUNT] = {
    [GALAXY_HONEYHIVE] = SS_ART_G_HONEYHIVE, [GALAXY_GOODEGG] = SS_ART_G_GOODEGG,
    [GALAXY_BEACHBOWL] = SS_ART_G_BEACHBOWL, [GALAXY_FREEZEFLAME] = SS_ART_G_FREEZEFLAME,
    [GALAXY_DEEPDARK] = SS_ART_G_DEEPDARK,   [GALAXY_GUSTYGARDEN] = SS_ART_G_GUSTYGARDEN,
    [GALAXY_BUOYBASE] = SS_ART_G_BUOYBASE,
};
static const uint8_t ss_scenario_art[GALAXY_COUNT] = {
    [GALAXY_HONEYHIVE] = SS_ART_S_HONEYHIVE_1, [GALAXY_GOODEGG] = SS_ART_S_GOODEGG_1,
    [GALAXY_BEACHBOWL] = SS_ART_S_BEACHBOWL_1, [GALAXY_FREEZEFLAME] = SS_ART_S_FREEZEFLAME_1,
    [GALAXY_DEEPDARK] = SS_ART_S_DEEPDARK_1,   [GALAXY_GUSTYGARDEN] = SS_ART_S_GUSTYGARDEN_1,
    [GALAXY_BUOYBASE] = SS_ART_S_BUOYBASE_1,
};

enum {
    SELECT_STAR_APPEAR,
    SELECT_STAR_NOT_POINTING,
    SELECT_STAR_POINTING,
    SELECT_STAR_END_POINTING,
    SELECT_STAR_SELECTED,
    SELECT_STAR_SELECTED_MOVE,
    SELECT_STAR_NOT_SELECTED,
    SELECT_STAR_DEAD
};
enum {
    SCENARIO_SELECT_APPEAR_STAR,
    SCENARIO_SELECT_COMET_APPEAR,
    SCENARIO_SELECT_COMET_WAIT,
    SCENARIO_SELECT_COMET_END,
    SCENARIO_SELECT_APPEAR,
    SCENARIO_SELECT_WAIT,
    SCENARIO_SELECT_DECIDE,
    SCENARIO_SELECT_AFTER,
    SCENARIO_SELECT_CANCEL,
    SCENARIO_SELECT_CANCEL_FADE
};

#define SS_DEG(d) ((int32_t)((d) * 65536 / 360))

typedef struct {
    uint8_t nerve, slot, state;
    int8_t mission;
    int16_t step, appear_frame, rotate_time;
    int32_t rot, rot_speed;
    int32_t scale, scale_on_select;
    v3 base, trans, trans_on_select;
    Figure figure;
} ScenarioSelectStar;

typedef struct {
    int galaxy, nerve, step, count;
    uint32_t frame;
    ScenarioSelectStar stars[7];
    int pointed;
    int text_mission, text_step, text_out;
    int new_star, new_step;
    int back_step, back_out;
    int comet, comet_step;
    int hand_x16, hand_y16;
    int8_t slot_art[SS_SLOT_COUNT];
    int selected;
    bool vram;
    int dev_mode;
} ScenarioSelect;

static ScenarioSelect g_ss EWRAM_BSS;

static struct {
    uint32_t start;
    int16_t frames;
    int8_t dir;
    bool white;
} g_screen_fade EWRAM_BSS;

static void screen_fade_start(int frames, int dir, bool white){
    g_screen_fade.start = g_game.tick;
    g_screen_fade.frames = (int16_t)frames;
    g_screen_fade.dir = (int8_t)dir;
    g_screen_fade.white = white;
}

static int screen_fade_level(void){
    if(!g_screen_fade.dir) return 0;
    int t = (int)(g_game.tick - g_screen_fade.start);
    if(t >= g_screen_fade.frames){
        if(g_screen_fade.dir < 0){
            g_screen_fade.dir = 0;
            return 0;
        }
        return 16;
    }
    int level = t * 16 / g_screen_fade.frames;
    return g_screen_fade.dir > 0 ? level : 16 - level;
}

static bool ss_mission_collected(int galaxy, int mission){
    return g_game.stars_collected[galaxy] >> galaxies[galaxy].star_ids[mission - 1] & 1;
}

static void ss_load_vram(void){
    hud_lifeup_restore();
    hud_copy_tiles(0, ss_tiles, (int)(ss_tiles_SIZE / 32));
    const uint16_t* pal = (const uint16_t*)ss_palette;
    for(int i = 0; i < (SS_BANK_COUNT - 2) * 16; i++) OBJ_PALETTE[i] = pal[i];
    for(int i = 0; i < SS_SLOT_COUNT; i++) g_ss.slot_art[i] = -1;
    for(int i = 0; i < SLOT_COUNT; i++) g_hud.slot_art[i] = -1;
    g_ss.vram = true;
}

static void ss_restore_vram(void){
    hud_copy_tiles(0, hud_tiles, (int)(hud_tiles_SIZE / 32));
    const uint16_t* pal = (const uint16_t*)hud_palette;
    for(uint32_t i = 0; i < hud_palette_SIZE / 2; i++) OBJ_PALETTE[i] = pal[i];
    for(int i = 0; i < SLOT_COUNT; i++) g_hud.slot_art[i] = -1;
    g_ss.vram = false;
}

static void ss_obj(int x, int y, int tile, int bank, int obj, bool semi){
    if(g_oam_count >= 128 || x <= -64 || x >= 240 || y <= -64 || y >= 160) return;
    OamEntry* e = &g_oam[g_oam_count++];
    e->attr0 = (uint16_t)((y & 0xFF) | (semi ? 0x0400 : 0) | (obj >> 2) << 14);
    e->attr1 = (uint16_t)((x & 0x1FF) | (obj & 3) << 14);
    e->attr2 = (uint16_t)((512 + tile) | bank << 12);
}

static void ss_art(int art, int x, int y, bool semi){
    const SsArt* a = &ss_arts[art];
    int base = 0;
    if(a->slot){
        if(g_ss.slot_art[a->slot] != art){
            int n = a->tiles < ss_slot_size[a->slot] ? a->tiles : ss_slot_size[a->slot];
            ui_text_copy_tiles(ss_slot_base[a->slot], ss_text, a->data / 32, n);
            g_ss.slot_art[a->slot] = (int8_t)art;
        }
        base = ss_slot_base[a->slot];
    }
    for(int i = 0; i < a->count; i++){
        const SsPiece* p = &ss_pieces[a->first + i];
        ss_obj(x + p->x, y + p->y, base + p->tile, a->bank, p->obj, semi);
    }
}

static v3 ss_world_from_screen(int x16, int y16){
    return V3((fx)(((int64_t)(x16 - 120 * 16) * 5178) / 16000),
              (fx)(((int64_t)(80 * 16 - y16) * 5178) / 16000), 0);
}

static void ss_star_setup(ScenarioSelectStar* s, int mission, int state, int slot, int x16, int y16){
    *s = (ScenarioSelectStar){0};
    s->mission = (int8_t)mission;
    s->state = (uint8_t)state;
    s->slot = (uint8_t)slot;
    s->base = ss_world_from_screen(x16, y16);
    s->rot = SS_DEG(150 * slot);
    s->appear_frame = (int16_t)(slot * 15 + 60);
    s->rotate_time = (int16_t)(slot * 15 + (slot % 2 ? 0 : 90));
    s->scale = 230;
    s->nerve = SELECT_STAR_APPEAR;
    figure_init(&s->figure, state == 0 ? figure_powerstarempty : figure_powerstar);
}

static void ss_star_update_pos(ScenarioSelectStar* s){
    fx a = (fx)(((int64_t)s->rotate_time * FX_TAU / 180) % FX_TAU);
    s->trans = v3_add(s->base, V3(0, fx_sin(a) * 15 / 256, 0));
}

static void ss_star_set(ScenarioSelectStar* s, int nerve){
    s->nerve = (uint8_t)nerve;
    s->step = 0;
}

static void ss_star_tick(ScenarioSelectStar* s){
    if(s->nerve == SELECT_STAR_DEAD) return;

    s->rot = (s->rot + s->rot_speed) & 0xFFFF;
    if(s->nerve == SELECT_STAR_NOT_POINTING || s->nerve == SELECT_STAR_POINTING ||
       s->nerve == SELECT_STAR_END_POINTING){
        s->rotate_time++;
        ss_star_update_pos(s);
    }
    int st = s->step;
    switch(s->nerve){
    case SELECT_STAR_APPEAR:
        if(st == 0) ss_star_update_pos(s);

        if(st == s->appear_frame) audio_play_effect_soft(SFX_SCEN_SEL_STAR_APPEAR, 1);
        if(st > s->appear_frame + STAR_ANIM_SCENARIO_APPEAR_FRAMES) ss_star_set(s, SELECT_STAR_NOT_POINTING);
        break;
    case SELECT_STAR_NOT_POINTING:
        s->scale = 230;
        s->rot_speed = SS_DEG(1.5);
        break;
    case SELECT_STAR_POINTING:
        s->rot_speed = SS_DEG(3);
        s->scale = st < 20 ? ss_point_scale[st] : 384;
        break;
    case SELECT_STAR_END_POINTING:
        s->rot_speed = SS_DEG(1.5);
        s->scale = 384 - (384 - 230) * fx_sin(st * (FX_PI / 2) / 10) / 256;
        if(st >= 10) ss_star_set(s, SELECT_STAR_NOT_POINTING);
        break;
    case SELECT_STAR_SELECTED:
        if(st == 0){
            s->trans_on_select = s->trans;
            s->scale_on_select = s->scale;
        }
        s->rot_speed = SS_DEG(3);
        s->scale = s->scale_on_select + (384 - s->scale_on_select) * st / 40;
        if(st >= 40) ss_star_set(s, SELECT_STAR_SELECTED_MOVE);
        break;
    case SELECT_STAR_SELECTED_MOVE:
        s->scale = 384;
        if(st < 60){
            int32_t t = fx_sin(st * (FX_PI / 2) / 60);
            s->trans = V3(fx_mul(s->trans_on_select.x, 256 - t), fx_mul(s->trans_on_select.y, 256 - t), 0);
            s->rot_speed = SS_DEG(3) + (SS_DEG(6) - SS_DEG(3)) * t / 256;
        } else {
            s->trans = V3(0, 0, 0);
            int32_t r = st - 60 > 120 ? 120 : st - 60;
            s->rot_speed = SS_DEG(6) + (SS_DEG(20) - SS_DEG(6)) * r / 120;
        }
        break;
    case SELECT_STAR_NOT_SELECTED: {
        int delay = 10 * s->slot;
        if(st < delay) s->rot_speed = SS_DEG(1.5);
        if(st == delay){
            s->rot_speed = 0;
            s->rot = SS_DEG(150 * s->slot);
        }
        if(st > delay + STAR_ANIM_SCENARIO_HIDE_FRAMES) s->nerve = SELECT_STAR_DEAD;
        break;
    }
    default: break;
    }
    s->step++;
}

static inline v3 ss_game(v3 g){ return V3(-g.x, g.z, g.y); }

static void ss_star_transform(ScenarioSelectStar* s){
    const StarAnimFrame* jf = NULL;
    if(s->nerve == SELECT_STAR_APPEAR && s->step > s->appear_frame)
        jf = &star_anim_scenario_appear[s->step - s->appear_frame > STAR_ANIM_SCENARIO_APPEAR_FRAMES
                                            ? STAR_ANIM_SCENARIO_APPEAR_FRAMES
                                            : s->step - s->appear_frame];
    if(s->nerve == SELECT_STAR_NOT_SELECTED && s->step > 10 * s->slot)
        jf = &star_anim_scenario_hide[s->step - 10 * s->slot > STAR_ANIM_SCENARIO_HIDE_FRAMES
                                          ? STAR_ANIM_SCENARIO_HIDE_FRAMES
                                          : s->step - 10 * s->slot];
    v2 cs = fx_sin_cos((fx)((s->rot * FX_TAU) >> 16));
    fx c = cs.y, sn = cs.x;

    int32_t J[3][3];
    for(int r = 0; r < 3; r++)
        for(int k = 0; k < 3; k++) J[r][k] = jf ? jf->axis[k][r] : (r == k ? 16384 : 0);
    int32_t G[3][3];
    for(int k = 0; k < 3; k++){
        G[0][k] = (c * J[0][k] - sn * J[1][k]) >> 8;
        G[1][k] = (sn * J[0][k] + c * J[1][k]) >> 8;
        G[2][k] = J[2][k];
    }
    v3 jt = jf ? V3(jf->t[0], jf->t[1], jf->t[2]) : V3(0, 0, 0);
    int32_t js = jf ? jf->scale : 256;

    v3 st = V3(jt.x * s->scale / 256, jt.y * s->scale / 256, jt.z * s->scale / 256);
    v3 rt = V3((c * st.x - sn * st.y) >> 8, (sn * st.x + c * st.y) >> 8, st.z);

    static const int gi[3] = {0, 2, 1}, sg[3] = {-1, 1, 1};
    int32_t k = 64 * s->scale / 256 * js / 256;
    m3 m;
    fx* row[3] = {&m.r[0].x, &m.r[1].x, &m.r[2].x};
    for(int i = 0; i < 3; i++)
        for(int j = 0; j < 3; j++) row[i][j] = (fx)(((int64_t)sg[i] * sg[j] * G[gi[i]][gi[j]] * k) >> 14);
    s->figure.transform.m = m;
    s->figure.transform.o = ss_game(v3_add(s->trans, rt));
    s->figure.transform_dirty = false;
}

static int ss_kind(int galaxy, int mission){
    return mission <= 6 ? mission_kinds[galaxy][mission - 1] : MISSION_HIDDEN;
}

static void scenario_select_open(int galaxy){
    ScenarioSelect* ss = &g_ss;
    const GalaxyInfo* gi = &galaxies[galaxy];
    int dev_mode = ss->galaxy == galaxy ? ss->dev_mode : 0;
    bool dev = dev_mode == 1;
    *ss = (ScenarioSelect){0};
    ss->galaxy = galaxy;
    ss->dev_mode = dev_mode;
    ss->pointed = -1;
    ss->new_star = -1;
    ss->selected = -1;
    int n = gi->mission_count;

    dev = true;

    int normal[6], nn = 0, top = 0;
    bool fresh = false;
    for(int m = 1; m <= n; m++)
        if(ss_kind(galaxy, m) == MISSION_NORMAL) normal[nn++] = m;
    for(top = 0; top < nn; top++){
        if(!ss_mission_collected(galaxy, normal[top]) && dev_mode != 2){
            top++;
            fresh = true;
            break;
        }
    }

    int comet_mission = 0;
    if(!fresh && !dev){
        for(int m = 1; m <= n; m++){
            int k = ss_kind(galaxy, m);
            if(((k >= MISSION_COMET_RED && k <= MISSION_COMET_QUICK) || k == MISSION_COMET_DARK) &&
               (!ss_mission_collected(galaxy, m) || dev_mode == 2)){
                comet_mission = m;
                ss->comet = k;
                break;
            }
        }
    }
    int down[4], nd = 0;
    if(comet_mission){
        ss_star_setup(&ss->stars[0], comet_mission, 0, 0, ss_star_top_x16[0][0], SS_STAR_TOP_ALONE_Y16);
        ss->count = 1;
        ss->new_star = 0;
    } else {
        if(dev){
            top = n < 3 ? n : 3;
            for(int m = 4; m <= n && nd < 4; m++) down[nd++] = m;
        } else {
            for(int m = 1; m <= n && nd < 4; m++){
                int k = ss_kind(galaxy, m);
                if(k != MISSION_NORMAL && ss_mission_collected(galaxy, m)) down[nd++] = m;
            }
        }
        int y_top = nd ? SS_STAR_TOP_Y16 : SS_STAR_TOP_ALONE_Y16;
        ss->count = 0;
        for(int i = 0; i < top; i++){
            int m = dev ? i + 1 : normal[i];
            bool got = ss_mission_collected(galaxy, m);
            int state = got ? (fresh ? 1 : 2) : 0;
            ss_star_setup(&ss->stars[ss->count++], m, state, i, ss_star_top_x16[top - 1][i], y_top);
            if(!got && !dev) ss->new_star = ss->count - 1;
        }
        for(int i = 0; i < nd; i++){
            bool got = ss_mission_collected(galaxy, down[i]);
            ss_star_setup(&ss->stars[ss->count++], down[i], got ? (fresh ? 1 : 2) : 0, 3 + i,
                          ss_star_down_x16[nd - 1][i], SS_STAR_DOWN_Y16);
        }
    }

    ss->pointed = ss->new_star >= 0 ? ss->new_star : 0;
    ss->hand_x16 = 120 * 16;
    ss->hand_y16 = 100 * 16;
    ss->text_mission = 0;
    ss->nerve = SCENARIO_SELECT_APPEAR_STAR;

    audio_play_music(MUSIC_SENARIO_SEL);
    ss_load_vram();

    screen_fade_start(30, -1, true);
}

static void ss_close(void){
    ss_restore_vram();
    g_screen_fade.dir = 0;
}

static void ss_set(int nerve){
    g_ss.nerve = nerve;
    g_ss.step = 0;
}

static bool ss_point_moves(const KeyState* keys){
    ScenarioSelect* ss = &g_ss;
    int old = ss->pointed;
    if(ss->count <= 1) return false;
    if(keys->pressed & (KEY_LEFT | KEY_RIGHT)){
        int d = keys->pressed & KEY_RIGHT ? 1 : -1;
        ss->pointed = (ss->pointed + d + ss->count) % ss->count;
    }
    if(keys->pressed & (KEY_UP | KEY_DOWN)){
        bool down = keys->pressed & KEY_DOWN;
        int best = -1, bd = 1 << 30;
        const ScenarioSelectStar* p = &ss->stars[ss->pointed];
        for(int i = 0; i < ss->count; i++){
            const ScenarioSelectStar* s = &ss->stars[i];
            if((s->slot >= 3) != down || i == ss->pointed) continue;
            int d = s->base.x - p->base.x;
            if(d < 0) d = -d;
            if(d < bd){
                bd = d;
                best = i;
            }
        }
        if(best >= 0) ss->pointed = best;
    }
    return ss->pointed != old;
}

static void scenario_select_tick(const KeyState* keys){
    Game* game = &g_game;
    ScenarioSelect* ss = &g_ss;
    ss->frame++;
    int st = ss->step++;
    switch(ss->nerve){
    case SCENARIO_SELECT_APPEAR_STAR: {
        bool all = true;
        for(int i = 0; i < ss->count; i++)
            if(ss->stars[i].nerve == SELECT_STAR_APPEAR) all = false;
        if(all) ss_set(ss->comet ? SCENARIO_SELECT_COMET_APPEAR : SCENARIO_SELECT_APPEAR);
        break;
    }
    case SCENARIO_SELECT_COMET_APPEAR:
        if(st == 15) audio_play_effect(SFX_MENU_OPEN, 3);
        if(st >= 35) ss_set(SCENARIO_SELECT_COMET_WAIT);
        break;
    case SCENARIO_SELECT_COMET_WAIT:
        if(st % 80 == 25) audio_play_effect(SFX_MENU_MOVE, 2);
        if(st >= 180) ss_set(SCENARIO_SELECT_COMET_END);
        break;
    case SCENARIO_SELECT_COMET_END:
        if(st >= 20) ss_set(SCENARIO_SELECT_APPEAR);
        break;
    case SCENARIO_SELECT_APPEAR:
        if(st >= 30) ss_set(SCENARIO_SELECT_WAIT);
        break;
    case SCENARIO_SELECT_WAIT:
        if(st == 0){
            ss->back_step = 0;
            ss->back_out = 0;
        }
        if((keys->held & (KEY_L | KEY_R)) == (KEY_L | KEY_R) && (keys->pressed & (KEY_L | KEY_R))){
            ss->dev_mode = (ss->dev_mode + 1) % 3;
            scenario_select_open(ss->galaxy);
            screen_fade_start(1, -1, true);
            return;
        }
        if(ss_point_moves(keys)) audio_play_effect(SFX_MENU_MOVE, 1);
        if(keys->pressed & KEY_B){
            audio_play_effect(SFX_MENU_SELECT, 4);
            ss->back_out = 1;
            ss->back_step = 0;
            ss->pointed = -1;
            ss_set(SCENARIO_SELECT_CANCEL);
        } else if(keys->pressed & (KEY_A | KEY_START) && ss->pointed >= 0){
            audio_play_effect(SFX_MENU_SELECT, 4);
            ss->selected = ss->pointed;
            ss->back_out = 1;
            ss->back_step = 0;
            for(int i = 0; i < ss->count; i++)
                ss_star_set(&ss->stars[i],
                            i == ss->selected ? SELECT_STAR_SELECTED : SELECT_STAR_NOT_SELECTED);
            ss_set(SCENARIO_SELECT_DECIDE);
        }
        break;
    case SCENARIO_SELECT_DECIDE:
        if(st >= 40) ss_set(SCENARIO_SELECT_AFTER);
        break;
    case SCENARIO_SELECT_AFTER:
        if(st == 180){
            screen_fade_start(30, 1, true);
            audio_stop_music();
        }
        if(st > 210){
            int m = ss->stars[ss->selected].mission;
            const GalaxyInfo* gi = &galaxies[ss->galaxy];
            ss_close();
            game->scene = SCENE_PLAY;
            galaxy_map_clear_return();
            game_load_stage(level_mission_stage(gi->stage, m), gi->starts[m - 1], m);
            screen_fade_start(180, -1, true);
            return;
        }
        break;
    case SCENARIO_SELECT_CANCEL:
        if(st == 20){
            screen_fade_start(90, 1, false);
            audio_stop_music();
            ss_set(SCENARIO_SELECT_CANCEL_FADE);
        }
        break;
    case SCENARIO_SELECT_CANCEL_FADE:
        if(st >= 90){
            ss_close();
            game->scene = SCENE_PLAY;
            if(!galaxy_map_return_from_missions()) game_reload_keep_health(STAGE_OBSERVATORY, 1, 0);
            return;
        }
        break;
    }

    for(int i = 0; i < ss->count; i++){
        ScenarioSelectStar* s = &ss->stars[i];
        bool want = i == ss->pointed && ss->nerve == SCENARIO_SELECT_WAIT;
        if(want && (s->nerve == SELECT_STAR_NOT_POINTING || s->nerve == SELECT_STAR_END_POINTING))
            ss_star_set(s, SELECT_STAR_POINTING);
        if(!want && s->nerve == SELECT_STAR_POINTING) ss_star_set(s, SELECT_STAR_END_POINTING);
        ss_star_tick(s);
    }

    int want = ss->nerve == SCENARIO_SELECT_WAIT && ss->pointed >= 0 ? ss->stars[ss->pointed].mission : 0;
    if(ss->nerve == SCENARIO_SELECT_DECIDE || (ss->nerve == SCENARIO_SELECT_AFTER && st < 60))
        want = ss->text_mission;
    if(want && want != ss->text_mission){
        ss->text_mission = want;
        ss->text_step = 0;
        ss->text_out = 0;
    } else if(!want && ss->text_mission && !ss->text_out){
        ss->text_out = 1;
        ss->text_step = 0;
    } else {
        ss->text_step++;
        if(ss->text_out && ss->text_step > 20) ss->text_mission = 0;
    }
    ss->new_step++;
    ss->back_step++;

    if(ss->pointed >= 0){
        const ScenarioSelectStar* s = &ss->stars[ss->pointed];
        int tx = 120 * 16 + s->trans.x * 16000 / 5178, ty = 80 * 16 - s->trans.y * 16000 / 5178;
        ss->hand_x16 += (tx - ss->hand_x16) / 3;
        ss->hand_y16 += (ty - ss->hand_y16) / 3;
    }
    game->fade_target = game->fade;
}

static void scenario_select_draw(uint8_t* framebuffer){
    ScenarioSelect* ss = &g_ss;

    int t = (int)(ss->frame % 4000);
    int x = (SS_SKY_X0_16 - SS_SKY_DX_16 * t / 4000) >> 4, y = (SS_SKY_Y0_16 - SS_SKY_DY_16 * t / 4000) >> 4;
    x &= ~1;
    if(x < 0) x = 0;
    if(x > SS_SKY_W - FRAME_WIDTH) x = SS_SKY_W - FRAME_WIDTH;
    if(y < 0) y = 0;
    if(y > SS_SKY_H - FRAME_HEIGHT) y = SS_SKY_H - FRAME_HEIGHT;

    for(int row = 0; row < FRAME_HEIGHT; row++){
        const uint8_t* src = ss_sky + (y + row) * SS_SKY_W + x;
        uint16_t* dst = (uint16_t*)(framebuffer + row * FRAME_STRIDE);
        for(int i = 0; i < FRAME_WIDTH / 2; i++) dst[i] = (uint16_t)(src[4 * i] | src[4 * i + 2] << 8);
    }

    Camera cam = {0};
    cam.position = V3(0, FX(1000), 0);
    cam.front = V3(0, -N14_ONE, 0);
    cam.up = V3(0, 0, N14_ONE);
    CameraView view;
    camera_build_view(&cam, &view);

    int saved = g_game.view_distance;
    g_game.view_distance = VIEW_MODE_DEFAULT;
    int order[7], n = 0;
    for(int i = 0; i < ss->count; i++){
        ScenarioSelectStar* s = &ss->stars[i];
        if(s->nerve == SELECT_STAR_DEAD) continue;
        if(s->nerve == SELECT_STAR_APPEAR && s->step <= s->appear_frame + 1) continue;
        ss_star_transform(s);
        bool full = i == ss->pointed || s->nerve == SELECT_STAR_SELECTED ||
                    s->nerve == SELECT_STAR_SELECTED_MOVE || s->nerve == SELECT_STAR_POINTING ||
                    s->nerve == SELECT_STAR_END_POINTING || s->nerve == SELECT_STAR_APPEAR;
        s->figure.scale.z = full ? 0x7FFF : 32;
        int k = n++;
        while(k > 0 && ss->stars[order[k - 1]].figure.transform.o.y < s->figure.transform.o.y){
            order[k] = order[k - 1];
            k--;
        }
        order[k] = i;
    }
    for(int k = 0; k < n; k++){
        Figure* f = &ss->stars[order[k]].figure;
        v3 vp = xform_point(&view.to_screen, f->transform.o);
        if(vp.z < FX(100) * 4) continue;
        draw_figure(framebuffer, f, &view);
    }
    g_game.view_distance = saved;
}

static int ss_band_level(void){
    const ScenarioSelect* ss = &g_ss;
    switch(ss->nerve){
    case SCENARIO_SELECT_APPEAR: return ss_appear_alpha[ss->step > 30 ? 30 : ss->step];
    case SCENARIO_SELECT_WAIT:
    case SCENARIO_SELECT_DECIDE:
    case SCENARIO_SELECT_CANCEL:
    case SCENARIO_SELECT_CANCEL_FADE: return 16;
    case SCENARIO_SELECT_AFTER:
        return ss->step < 60 ? 16 : ss_end_alpha[ss->step - 60 > 30 ? 30 : ss->step - 60];
    default: return 0;
    }
}

static int ss_band_dy(void){
    const ScenarioSelect* ss = &g_ss;
    if(ss->nerve == SCENARIO_SELECT_APPEAR) return ss_appear_dy[ss->step > 30 ? 30 : ss->step];
    if(ss->nerve == SCENARIO_SELECT_AFTER && ss->step >= 60)
        return ss_end_dy[ss->step - 60 > 30 ? 30 : ss->step - 60];
    return 0;
}

static void ss_draw_digits(int x, int y, int value){
    char buf[4];
    int n = 0;
    if(value < 0) value = 0;
    if(value > 99) value = 99;
    do {
        buf[n++] = (char)('0' + value % 10);
        value /= 10;
    } while(value);
    int x16 = x << 4;
    for(int i = n - 1; i >= 0; i--, x16 += SS_LIVES_DIGIT_ADV16)
        ss_art(SS_ART_LDIGIT0 + buf[i] - '0', x16 >> 4, y, false);
}

static void scenario_select_draw_hud(void){
    ScenarioSelect* ss = &g_ss;

    if(ss->nerve == SCENARIO_SELECT_WAIT && ss->pointed >= 0)
        ss_art(SS_ART_HAND, (ss->hand_x16 >> 4) + 6 + SS_HAND_DX, (ss->hand_y16 >> 4) + 8 + SS_HAND_DY,
               false);

    if(ss->comet && (ss->nerve == SCENARIO_SELECT_COMET_APPEAR || ss->nerve == SCENARIO_SELECT_COMET_WAIT ||
                     ss->nerve == SCENARIO_SELECT_COMET_END)){
        int dx = 0;
        bool show = true;
        if(ss->nerve == SCENARIO_SELECT_COMET_APPEAR){
            int f = ss->step - 15;
            show = f >= 10;
            dx = f < 20 ? (100 - 5 * f) * 240 / 608 : 0;
        } else if(ss->nerve == SCENARIO_SELECT_COMET_END){
            show = ss->step < 10;
            dx = -5 * ss->step * 240 / 608;
        } else {
            int f = ss->step % 40;
            show = !(f >= 16 && f < 25);
        }
        if(show){
            int mark = ss->comet == MISSION_COMET_QUICK    ? SS_ART_CM_QUICK
                       : ss->comet == MISSION_COMET_RED    ? SS_ART_CM_RED
                       : ss->comet == MISSION_COMET_PURPLE ? SS_ART_CM_PURPLE
                                                           : SS_ART_CM_GHOST;
            int text = ss->comet == MISSION_COMET_QUICK    ? SS_ART_CT_QUICK
                       : ss->comet == MISSION_COMET_RED    ? SS_ART_CT_RED
                       : ss->comet == MISSION_COMET_PURPLE ? SS_ART_CT_PURPLE
                                                           : SS_ART_CT_GHOST;
            if(ss->comet == MISSION_COMET_DARK){
                mark = SS_ART_CM_DARK;
                text = SS_ART_CT_DARK;
            }
            const SsArt* m = &ss_arts[mark];
            ss_art(mark, SS_COMET_MARK_CX - m->w / 2 + dx, SS_COMET_MARK_CY - m->h / 2, false);
            const SsArt* t = &ss_arts[text];
            ss_art(text, 120 - t->w / 2 + dx, SS_COMET_TEXT_CY - t->h / 2, false);
        }
    }
    int band = ss_band_level();
    int dy = ss_band_dy();
    if(band > 0){
        const SsArt* g = &ss_arts[ss_galaxy_art[ss->galaxy]];
        int gdy = ss->nerve == SCENARIO_SELECT_APPEAR ? ss_galaxy_dy[ss->step > 30 ? 30 : ss->step] : dy;
        if(band >= 8) ss_art(ss_galaxy_art[ss->galaxy], 120 - g->w / 2, SS_GALAXY_CY - g->h / 2 + gdy, false);
        if(ss->text_mission){
            int f = ss->text_step > 20 ? 20 : ss->text_step;
            int tdx = ss->text_out ? ss_selout_dx[f] : ss_selin_dx[f];
            int a = ss->text_out ? ss_selout_a[f] : ss_selin_a[f];
            int art = ss_scenario_art[ss->galaxy] + ss->text_mission - 1;
            const SsArt* sa = &ss_arts[art];
            if(a >= 128 && band >= 8)
                ss_art(art, 120 - sa->w / 2 + tdx, SS_SCENARIO_CY - sa->h / 2 + dy, false);
        }

        if(band >= 8){
            int two = g_game.lives >= 10 ? SS_LIVES_TWO_DX : 0;
            ss_art(SS_ART_LIVES_ICON, SS_LIVES_ICON_X + two, SS_LIVES_ICON_Y - dy, false);
            ss_draw_digits(SS_LIVES_DIGIT_X + two, SS_LIVES_DIGIT_Y - dy, g_game.lives);
        }
    }

    if(ss->new_star >= 0 && !ss->comet){
        const ScenarioSelectStar* s = &ss->stars[ss->new_star];
        bool show = (ss->nerve == SCENARIO_SELECT_APPEAR && ss->step >= 10) ||
                    ss->nerve == SCENARIO_SELECT_WAIT ||
                    (ss->nerve == SCENARIO_SELECT_DECIDE && ss->step < 10);
        if(show && s->nerve != SELECT_STAR_DEAD){
            int sx = 120 + s->trans.x * 1000 / 5178, sy = 80 - s->trans.y * 1000 / 5178;
            ss_art(SS_ART_NEW, sx + SS_NEW_DX, sy + SS_NEW_DY, false);
        }
    }

    if(ss->nerve == SCENARIO_SELECT_WAIT ||
       ((ss->nerve == SCENARIO_SELECT_CANCEL || ss->nerve == SCENARIO_SELECT_DECIDE) &&
        ss->back_step <= 20)){
        int f = ss->back_step > 20 ? 20 : ss->back_step;
        int bdy = ss->back_out ? ss_back_end_dy[f] : ss_back_appear_dy[f];
        if(!(ss->back_out && f >= 15)) ss_art(SS_ART_BACK, SS_BACK_X, SS_BACK_Y + bdy, false);
    }

    if(band > 0){
        const SsArt* u = &ss_arts[SS_ART_BAND_U];
        const SsArt* d = &ss_arts[SS_ART_BAND_D];
        for(int x = 0; x < 240; x += 32){
            for(int i = 0; i < u->count; i++){
                const SsPiece* p = &ss_pieces[u->first + i];
                ss_obj(x, p->y + dy, p->tile, u->bank, p->obj, true);
            }
            for(int i = 0; i < d->count; i++){
                const SsPiece* p = &ss_pieces[d->first + i];
                ss_obj(x, 160 - SS_BAND_D_H + p->y - dy, p->tile, d->bank, p->obj, true);
            }
        }
    }
}

static uint16_t ss_lerp555(uint16_t a, uint16_t b, int t, int n){
    int r = (a & 31) + ((b & 31) - (a & 31)) * t / n;
    int g = (a >> 5 & 31) + ((b >> 5 & 31) - (a >> 5 & 31)) * t / n;
    int bl = (a >> 10 & 31) + ((b >> 10 & 31) - (a >> 10 & 31)) * t / n;
    return (uint16_t)(r | g << 5 | bl << 10);
}

static void ss_write_band_palette(void){
    int f = (int)(g_ss.frame % 1800);
    int k = f / SS_WAIT_STEP, k2 = (k + 1) % (1800 / SS_WAIT_STEP), t = f % SS_WAIT_STEP;
    uint16_t c[4];
    for(int i = 0; i < 4; i++)
        c[i] = ss_lerp555(ss_wait_colours[k][i], ss_wait_colours[k2][i], t, SS_WAIT_STEP);
    int white = g_screen_fade.white ? screen_fade_level() : 0;
    for(int i = 0; i < 15; i++){
        OBJ_PALETTE[SS_BANK_BAND_U * 16 + 1 + i] =
            ss_lerp555(ss_lerp555(c[0], c[1], 2 * i + 1, 30), 0x7FFF, white, 16);
        OBJ_PALETTE[SS_BANK_BAND_D * 16 + 1 + i] =
            ss_lerp555(ss_lerp555(c[2], c[3], 2 * i + 1, 30), 0x7FFF, white, 16);
    }
}

static void scenario_select_set_fade(Game* game){
    int mine = screen_fade_level();
    if(g_screen_fade.dir && game->scene != SCENE_MISSION_SELECT && game->fade > mine) mine = game->fade;
    if(game->scene == SCENE_MISSION_SELECT){
        ss_write_band_palette();
        hud_set_fade(mine, g_screen_fade.white);
#ifndef __GNW__
        /* These effects use GBA blending hardware. Like hud_set_fade(),
         * leave them disabled until the software compositor implements them. */
        int band = ss_band_level();
        if(screen_fade_level() == 0 && g_ss.comet && g_ss.nerve >= SCENARIO_SELECT_COMET_APPEAR &&
           g_ss.nerve <= SCENARIO_SELECT_COMET_END){
            int a = g_ss.nerve == SCENARIO_SELECT_COMET_APPEAR ? (g_ss.step - 15) * 12 / 20
                    : g_ss.nerve == SCENARIO_SELECT_COMET_END  ? 12 - g_ss.step * 12 / 20
                                                               : 12;
            if(a < 0) a = 0;
            REG_BLDCNT = 0x00C0 | 0x0004 | 0x0020;
            REG_BLDY = (uint16_t)a;
        } else {
            REG_BLDCNT = (uint16_t)((REG_BLDCNT & 0x00FF) | 0x0400);
        }

        int eva = band * SS_BAND_EVA / 16, evb = 16 - eva;
        if(g_screen_fade.white){
            eva += (16 - eva) * mine / 16;
            evb -= evb * mine / 16;
        } else {
            eva -= eva * mine / 16;
            evb -= evb * mine / 16;
        }
        REG_BLDALPHA = (uint16_t)(eva | evb << 8);
#endif
        return;
    }
    if(g_ss.vram) ss_close();
    if(g_screen_fade.dir) hud_set_fade(mine, g_screen_fade.white);
    else hud_set_fade(game->fade, game->scene == SCENE_STAR_GET);
    star_get_set_blend();
}

static const GameScene scenario_select_scene = {
    .tick = scenario_select_tick,
    .draw = scenario_select_draw,
    .draw_hud = scenario_select_draw_hud,
};
