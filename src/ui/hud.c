#if defined(__GNW__)
volatile uint16_t smg_obj_tiles[512 * 16] EWRAM_BSS;
volatile uint16_t smg_obj_palette[256] EWRAM_BSS;
volatile uint16_t smg_obj_oam[128 * 4] EWRAM_BSS;
#endif

static const uint16_t slot_base[SLOT_COUNT] = {HUD_STATIC_TILES, HUD_STATIC_TILES + 64,
                                               HUD_STATIC_TILES + 128};
static const uint16_t slot_size[SLOT_COUNT] = {64, 64, 224};

static OamEntry g_oam[128] EWRAM_BSS;
static HudState g_hud EWRAM_BSS;
static volatile uint8_t g_oam_ready;
static int g_oam_count;

static void hud_copy_tiles(int tile, const uint8_t* data, int tiles){
    const uint16_t* src = (const uint16_t*)data;
    volatile uint16_t* dst = OBJ_TILES + tile * 16;
    for(int i = 0; i < tiles * 16; i++) dst[i] = src[i];
}

static void hud_init(void){
    hud_copy_tiles(0, hud_tiles, (int)(hud_tiles_SIZE / 32));
    const uint16_t* pal = (const uint16_t*)hud_palette;
    for(uint32_t i = 0; i < hud_palette_SIZE / 2; i++) OBJ_PALETTE[i] = pal[i];
    for(int i = 0; i < 128; i++){
        g_oam[i] = (OamEntry){0x0200, 0, 0, 0};
        OAM[i * 4] = 0x0200;
    }
    g_hud = (HudState){0};
    for(int i = 0; i < COUNTER_COUNT; i++) g_hud.counters[i].visible_timer = -1;
    for(int i = 0; i < SLOT_COUNT; i++) g_hud.slot_art[i] = -1;
    g_hud.last_health = 3;
}

static void hud_load_text(int slot, int art){
    if(g_hud.slot_art[slot] == art) return;
    const HudArt* a = &hud_arts[art];
    int tiles = a->tiles < slot_size[slot] ? a->tiles : slot_size[slot];
    ui_text_copy_tiles(slot_base[slot], hud_text, a->data / 32, tiles);
    g_hud.slot_art[slot] = (int16_t)art;
}

static void hud_begin(void){
    hud_lifeup_restore();
    g_oam_count = 0;
}

static void hud_piece(int x, int y, int tile, int pal, int obj){
    if(g_oam_count >= 128 || x <= -64 || x >= 240 || y <= -64 || y >= 160) return;
    OamEntry* e = &g_oam[g_oam_count++];
    e->attr0 = (uint16_t)((y & 0xFF) | (obj >> 2) << 14);
    e->attr1 = (uint16_t)((x & 0x1FF) | (obj & 3) << 14);
    e->attr2 = (uint16_t)((512 + tile) | pal << 12);
}

static void hud_art(int art, int x, int y, int slot){
    const HudArt* a = &hud_arts[art];
    int base = 0;
    if(a->dynamic){
        hud_load_text(slot, art);
        base = slot_base[slot] - (int)(a->data / 32);
    }
    for(int i = 0; i < a->count; i++){
        const HudPiece* p = &hud_pieces[a->first + i];
        hud_piece(x + p->x, y + p->y, base + p->tile, a->bank, p->obj);
    }
}

static void hud_art_centred(int art, int cy, int slot){
    const HudArt* a = &hud_arts[art];
    hud_art(art, 120 - a->w / 2, cy - a->h / 2, slot);
}

static int hud_digits(int x, int y, int value){
    char buf[8];
    int n = 0;
    if(value < 0) value = 0;
    do {
        buf[n++] = (char)('0' + value % 10);
        value /= 10;
    } while(value && n < 7);
    int x16 = x << 4;
    for(int i = n - 1; i >= 0; i--, x16 += HUD_DIGIT_ADV_X16)
        hud_art(HUD_ART_DIGIT0 + (buf[i] - '0'), (x16 >> 4) + HUD_DIGIT_WIN_DX, y + HUD_DIGIT_WIN_DY, 0);
    return x16 >> 4;
}

static int hud_digit_count(int value){
    int n = 1;
    while(value >= 10 && n < 7){
        value /= 10;
        n++;
    }
    return n;
}

static void hud_end(void){
    for(int i = g_oam_count; i < 128; i++) g_oam[i].attr0 = 0x0200;
    g_oam_ready = 1;
}

IWRAM_CODE void hud_vblank(void){
    if(!g_oam_ready) return;
#if defined(__GNW__)
    for(int i = 0; i < 128 * 4; i++) OAM[i] = ((const uint16_t*)g_oam)[i];
#else
    volatile uint32_t* dst = (volatile uint32_t*)OAM;
    const uint32_t* src = (const uint32_t*)g_oam;
    for(int i = 0; i < 256; i++) dst[i] = src[i];
#endif
    g_oam_ready = 0;
}

#if defined(__GNW__)
static void smg_obj_dimensions(unsigned shape, unsigned size, int* width, int* height){
    static const uint8_t dimensions[3][4][2] = {
        {{8, 8}, {16, 16}, {32, 32}, {64, 64}},
        {{16, 8}, {32, 8}, {32, 16}, {64, 32}},
        {{8, 16}, {8, 32}, {16, 32}, {32, 64}},
    };
    if(shape >= 3 || size >= 4){
        *width = *height = 0;
        return;
    }
    *width = dimensions[shape][size][0];
    *height = dimensions[shape][size][1];
}

static uint16_t smg_obj_rgb565(unsigned index){
    uint16_t c = smg_obj_palette[index & 255];
    unsigned r = c & 31, g = (c >> 5) & 31, b = (c >> 10) & 31;
    return (uint16_t)((r << 11) | (g << 6) | (b << 1));
}

static unsigned smg_obj_pixel(unsigned tile, int x, int y, int width){
    unsigned tiles_per_row = (unsigned)width / 8;
    unsigned tile_offset = tile + (unsigned)(y / 8) * tiles_per_row + (unsigned)(x / 8);
    if(tile_offset >= 512) return 0;
    uint16_t packed = smg_obj_tiles[tile_offset * 16 + (unsigned)(y & 7) * 2 + ((unsigned)(x & 7) >> 2)];
    return (packed >> ((x & 3) * 4)) & 15;
}

void smg_game_render_ui(uint16_t* dst, unsigned stride){
    if(!dst || stride < 320 || g_display_full_res) return;
    for(int i = 0; i < 128; i++){
        uint16_t a0 = OAM[i * 4], a1 = OAM[i * 4 + 1], a2 = OAM[i * 4 + 2];
        bool affine = (a0 & 0x0100) != 0;
        if((a0 & 0x0200) && !affine) continue;
        unsigned shape = a0 >> 14, size = a1 >> 14;
        int width, height;
        smg_obj_dimensions(shape, size, &width, &height);
        if(!width || !height) continue;
        int x = a1 & 0x01ff, y = a0 & 0x00ff;
        if(x >= 256) x -= 512;
        if(y >= 160) y -= 256;
        unsigned tile = (a2 & 0x03ff);
        if(tile < 512) continue;
        tile -= 512;
        unsigned bank = (a2 >> 12) & 15;
        bool hflip = !affine && (a1 & 0x1000);
        bool vflip = !affine && (a1 & 0x2000);
        int native_left = x, native_top = y;
        int native_right = x + width, native_bottom = y + height;
        int pa = 256, pb = 0, pc = 0, pd = 256;
        if(affine){
            unsigned matrix = (a1 >> 9) & 31;
            pa = (int16_t)OAM[matrix * 16 + 3];
            pb = (int16_t)OAM[matrix * 16 + 7];
            pc = (int16_t)OAM[matrix * 16 + 11];
            pd = (int16_t)OAM[matrix * 16 + 15];
            int half_w = (a0 & 0x0200) ? width : width / 2;
            int half_h = (a0 & 0x0200) ? height : height / 2;
            native_left = x + width / 2 - half_w;
            native_right = x + width / 2 + half_w;
            native_top = y + height / 2 - half_h;
            native_bottom = y + height / 2 + half_h;
            if(pa == 0 && pb == 0 && pc == 0 && pd == 0) continue;
        }
        /* OAM remains in the GBA's 240x160 coordinate space. Rasterize that
         * layout directly into the native 320x240 LCD output. */
        int left = native_left * 4 / 3, top = native_top * 3 / 2;
        int right = (native_right * 4 + 2) / 3, bottom = (native_bottom * 3 + 1) / 2;
        if(left < 0) left = 0;
        if(top < 0) top = 0;
        if(right > 320) right = 320;
        if(bottom > 240) bottom = 240;
        for(int py = top; py < bottom; py++){
            int native_y = py * 2 / 3;
            int sy = native_y - y;
            for(int px = left; px < right; px++){
                int native_x = px * 3 / 4;
                int sx = native_x - x;
                if(affine){
                    int dx = native_x - (x + width / 2);
                    int dy = native_y - (y + height / 2);
                    sx = width / 2 + (pa * dx + pb * dy) / 256;
                    sy = height / 2 + (pc * dx + pd * dy) / 256;
                }
                if(sx < 0 || sx >= width || sy < 0 || sy >= height) continue;
                if(hflip) sx = width - 1 - sx;
                if(vflip) sy = height - 1 - sy;
                unsigned color = smg_obj_pixel(tile, sx, sy, width);
                if(color) dst[py * stride + px] = smg_obj_rgb565(bank * 16 + color);
            }
        }
    }
}
#endif

static void hud_set_fade(int level, bool white){
#if defined(__GNW__)
    (void)level;
    (void)white;
#else
    if(level <= 0){
        REG_BLDCNT = 0;
        return;
    }
    REG_BLDCNT = (white ? 0x0080 : 0x00C0) | 0x0004 | 0x0010 | 0x0020;
    REG_BLDY = (uint16_t)(level > 16 ? 16 : level);
#endif
}

static void hud_counter_tick(HudCounter* c, int value, bool show_all, bool moving, bool count_up){
    if(value != c->shown){
        if(c->visible_timer < 0){
            c->visible_timer = 0;
            c->slide = 0;
        }
        if(!count_up || value < c->shown) c->shown = value;
        else if(++c->step >= 3){
            c->step = 0;
            c->shown++;
        }
        c->visible_timer = 0;
    }
    if(c->visible_timer < 0){
        if(show_all){
            c->visible_timer = 0;
            c->slide = 0;
        }
        return;
    }
    c->visible_timer++;
    if(c->slide < 20) c->slide++;
    if(c->visible_timer > 240 && moving && c->shown == value) c->visible_timer = -1;
}

static void hud_tick(const Game* game, const KeyState* keys){
    HudState* h = &g_hud;
    const Player* p = &game->player;
    h->bee_ground_ticks =
        bee_active() && p->on_ground && bee_fly_time() == 180 ? fx_min(h->bee_ground_ticks + 1, 60) : 0;
    bool still = p->on_ground &&
                 !(keys->held & (KEY_UP | KEY_DOWN | KEY_LEFT | KEY_RIGHT | KEY_A | KEY_B | KEY_R)) &&
                 p->action == MA_WAIT;
    h->still_frames = still ? h->still_frames + 1 : 0;
    hud_counter_tick(&h->counters[COUNTER_COINS], game->coins, true, false, true);
    hud_counter_tick(&h->counters[COUNTER_STAR_BITS], game->star_bits, true, false, true);
    hud_counter_tick(&h->counters[COUNTER_LIVES], game->lives, true, false, false);

    HudCounter* s = &h->counters[COUNTER_STARS];
    s->shown = game->stars_total;
    if(s->visible_timer < 0){
        s->visible_timer = 0;
        s->slide = 0;
    } else {
        s->visible_timer++;
        if(s->slide < 20) s->slide++;
    }

    int hp = p->health;
    if(hp < h->last_health) h->meter_timer = 240;
    h->last_health = hp;
    if(h->meter_timer > 0) h->meter_timer--;
    if(h->meter_slide > 0) h->meter_slide--;
}

static void hud_reset_counters(const Game* game){
    for(int i = 0; i < COUNTER_COUNT; i++){
        g_hud.counters[i].visible_timer = -1;
        g_hud.counters[i].step = 0;
    }
    g_hud.counters[COUNTER_COINS].shown = game->coins;
    g_hud.counters[COUNTER_STAR_BITS].shown = game->star_bits;
    g_hud.counters[COUNTER_LIVES].shown = game->lives;
    g_hud.counters[COUNTER_STARS].shown = game->stars_total;
    g_hud.still_frames = 0;
    g_hud.meter_timer = 0;
    g_hud.last_health = game->player.health;
}

static int hud_slide_offset(int slide){
    int t = 20 - slide;
    return (t * t) / 20;
}

static void hud_draw_counters(const Game* game){
    const HudState* h = &g_hud;
    bool raised = level_has(LEVEL_RAISED_COUNTERS);
    const HudCounter* c;

    c = &h->counters[COUNTER_STARS];
    if(c->visible_timer >= 0){
        int x = HUD_STAR_X - hud_slide_offset(c->slide);
        hud_art(HUD_ART_STAR, x, HUD_STAR_Y, 0);
        hud_digits(x + HUD_STAR_DIGITS_DX, HUD_STAR_Y + HUD_STAR_DIGITS_DY, c->shown);
    }

    c = &h->counters[COUNTER_LIVES];
    if(c->visible_timer >= 0){
        int x = HUD_MARIO_X - hud_slide_offset(c->slide);
        hud_art(HUD_ART_MARIO, x, HUD_MARIO_Y, 0);
        hud_digits(x + HUD_MARIO_DIGITS_DX, HUD_MARIO_Y + HUD_MARIO_DIGITS_DY, c->shown);
    }

    c = &h->counters[COUNTER_COINS];
    if(c->visible_timer >= 0){
        int y = HUD_COIN_Y - (raised ? 14 : 0);
        int x = HUD_COIN_RIGHT - ((hud_digit_count(c->shown) * HUD_DIGIT_ADV_X16) >> 4) +
                hud_slide_offset(c->slide);
        hud_art(HUD_ART_COIN, x + HUD_COIN_ICON_DX, y + HUD_COIN_ICON_DY, 0);
        hud_digits(x, y, c->shown);
    }

    c = &h->counters[COUNTER_STAR_BITS];
    if(c->visible_timer >= 0){
        int y = HUD_BIT_Y - (raised ? 14 : 0);
        int x = HUD_COIN_RIGHT - ((hud_digit_count(c->shown) * HUD_DIGIT_ADV_X16) >> 4) +
                hud_slide_offset(c->slide);
        hud_art(HUD_ART_STARBIT, x + HUD_BIT_ICON_DX, y + HUD_BIT_ICON_DY, 0);
        hud_digits(x, y, c->shown);
    }

    if(mario_lifeup_meter_active()) hud_lifeup_draw();
    else if(h->meter_slide < 20){
        int hp = game->player.health;
        if(hp < 0) hp = 0;
        if(hp > 3) hp = 3;
        int dx = (h->meter_slide * h->meter_slide) / 20;
        hud_art(HUD_ART_LIFE, HUD_LIFE_X + dx, HUD_LIFE_Y, 0);
        hud_art(HUD_ART_METER0 + hp, HUD_METER_X + dx, HUD_METER_Y, 0);
    }
}

static void hud_draw_fly_meter(const Game* game){
    int time = bee_fly_time();
    if(time < 0 || g_hud.bee_ground_ticks >= 60 || game->player.action == MA_STAR_LAUNCH) return;
    const Camera* c = &game->camera;
    v3 d = v3_sub(game->player.figure.position, c->position);
    int32_t z = v3_dot14(d, c->front);
    if(z <= 50) return;
    v3 left = v3_cross14(c->up, c->front);
    int x = 120 - v3_dot14(d, left) * 193 / z + HUD_FLY_DX;
    int y = 80 - v3_dot14(d, c->up) * 193 / z + HUD_FLY_DY;
    int count = fx_clamp((180 - time) * 32 / 180, 0, 32);
    int pulse = time < 90 && ((game->tick * (90 + (90 - time) * 2) / 90) % 60 >= 15) &&
                ((game->tick * (90 + (90 - time) * 2) / 90) % 60 < 45);
    hud_art(HUD_ART_FLY0_0 + pulse * 33 + count, x, y, SLOT_BANNER);
}

static void hud_draw_air_meter(const Game* game){
    int oxygen = mario_swim_life();
    if(oxygen < 0 || game->player.action == MA_STAR_LAUNCH) return;
    const Camera* c = &game->camera;
    v3 d = v3_sub(game->player.figure.position, c->position);
    int z = v3_dot14(d, c->front);
    if(z <= 50) return;
    v3 left = v3_cross14(c->up, c->front);
    int x = 120 - v3_dot14(d, left) * 193 / z + HUD_AIR_DX;
    int y = 80 - v3_dot14(d, c->up) * 193 / z + HUD_AIR_DY;
    int count = fx_clamp((3600 - oxygen) * 32 / 3600, 0, 32);
    int frame = (game->tick * (3600 - oxygen) / 1800) % 60;
    int pulse = oxygen < 1800 && frame >= 15 && frame < 45;
    hud_art(HUD_ART_AIR0_0 + pulse * 33 + count, x, y, SLOT_BANNER);
}

static void hud_draw_stage_title(int galaxy_art, int scenario_art){
    hud_art_centred(galaxy_art, HUD_TITLE_GALAXY_CY, SLOT_GALAXY);
    hud_art_centred(scenario_art, HUD_TITLE_SCENARIO_CY, SLOT_SCENARIO);
}

static void hud_draw_too_bad(int frame){
    int to = HUD_TOOBAD_CY, from = -20;
    int cy;
    if(frame < 20) cy = from + (to - from) * frame * frame / 400;
    else if(frame < 40) cy = to - (6 * (100 - (frame - 30) * (frame - 30))) / 100;
    else cy = to;
    hud_art_centred(HUD_ART_T_TOOBAD, cy, SLOT_BANNER);
}
