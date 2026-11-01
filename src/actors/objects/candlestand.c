enum { CST_WAITFIRE = 64, CST_FIRE, CST_BURN, CST_ATTACK, CST_FLICKER };
enum {
    CST_EVENT_IGNITE = 1,
    CST_EVENT_OFF = 2,
    CST_EVENT_SWITCH = 4,
    CST_EVENT_BURN_LEVEL = 8,
    CST_EVENT_COMPLETED = 16
};

static inline unsigned div255(unsigned x){
    x++;
    return (x + (x >> 8)) >> 8;
}
static unsigned candle_effect_blit(uint8_t* restrict fb, const FFCXTexture* tex, const FFCXFrame* frame,
                                   int x0, int y0, int x1, int y1, bool additive,
                                   const uint8_t rgb5_index[32768], const uint8_t palette[256][3]){
    if(!frame->alpha || x1 <= x0 || y1 <= y0 || x1 <= 0 || y1 <= 0 || x0 >= FRAME_WIDTH ||
       y0 >= FRAME_HEIGHT)
        return 0;
    uint64_t wide = (uint64_t)((int64_t)x1 - x0), high = (uint64_t)((int64_t)y1 - y0);

    if(wide > 16384 || high > 16384 || !tex->width || !tex->height) return 0;
    unsigned w = (unsigned)wide, h = (unsigned)high;
    int left = x0 < 0 ? 0 : x0, top = y0 < 0 ? 0 : y0,
        right = x1 > FRAME_WIDTH ? FRAME_WIDTH : x1,
        bottom = y1 > FRAME_HEIGHT ? FRAME_HEIGHT : y1;
    uint32_t du = ((uint32_t)tex->width << 16) / w, dv = ((uint32_t)tex->height << 16) / h;
    uint32_t v = (uint32_t)(top - y0) * dv;
    unsigned written = 0;

    const int rb = frame->env[0] * 255, gb = frame->env[1] * 255, bb = frame->env[2] * 255;
    const int rd = (int)frame->prm[0] - frame->env[0];
    const int gd = (int)frame->prm[1] - frame->env[1];
    const int bd = (int)frame->prm[2] - frame->env[2];
    const unsigned alpha = frame->alpha;

    for(int y = top; y < bottom; y++, v += dv){
        unsigned sy = v >> 16;
        if(sy >= tex->height) sy = tex->height - 1;
        const uint8_t* row = tex->rgba + sy * tex->width * 4;
        uint32_t u = (uint32_t)(left - x0) * du;
        for(int x = left; x < right; x++, u += du){
            unsigned sx = u >> 16;
            if(sx >= tex->width) sx = tex->width - 1;
            const uint8_t* p = row + sx * 4;
            unsigned a = div255(p[3] * alpha);
            if(!a) continue;

            unsigned r = div255((unsigned)(rb + rd * p[0]));
            unsigned g = div255((unsigned)(gb + gd * p[1]));
            unsigned b = div255((unsigned)(bb + bd * p[2]));
            const uint8_t* dest = palette[frame_get(fb, x, y)];
            unsigned keep = additive ? 255 : 255 - a;
            r = r * a + dest[0] * keep;
            g = g * a + dest[1] * keep;
            b = b * a + dest[2] * keep;
            if(r > 65025) r = 65025;
            if(g > 65025) g = 65025;
            if(b > 65025) b = 65025;
            r = div255(r);
            g = div255(g);
            b = div255(b);
            unsigned index = rgb5_index[((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3)];
            frame_put(fb, x, y, index);
            written++;
        }
    }
    return written;
}

static void candle_effect_shift(Actor* a, int step){
    if(a->velocity.x > 0) a->velocity.x += (uint16_t)step + 1;
    if(a->velocity.y > 0) a->velocity.y += (uint16_t)step + 1;
}
static void candle_effect_fire(Actor* a){ a->velocity.x = a->timer ? 1 : 2; }
static void candle_effect_off(Actor* a){
    a->velocity.x = 0;
    a->velocity.y = a->timer ? 1 : 2;
    a->target = a->position;
}
static int candle_effect_age(const Actor* a, int carry){
    int64_t value = (uint16_t)a->timer + (int64_t)carry - 2;
    return value < 0 ? 0 : value > 0x7fffffff ? 0x7fffffff : (int)value;
}
__attribute__((noinline)) static void candle_effect_layer(
    uint8_t* fb, const CameraView* view, const Actor* a, const FFCXLayer* layer, v3 root, int age){
    if(age < 0 || (!layer->continuous && (unsigned)age >= layer->frame_count)) return;
    const FFCXFrame* frame = &layer->frame[(unsigned)age % layer->frame_count];
    if(!frame->alpha || !frame->half_x || !frame->half_y) return;

    v3 local = V3((int32_t)((int64_t)layer->offset[0] * a->figure.scale.x / 16384),
                  (int32_t)((int64_t)layer->offset[1] * a->figure.scale.y / 16384),
                  (int32_t)((int64_t)layer->offset[2] * a->figure.scale.z / 16384));
    v3 world = v3_add(root, v3_add(v3_scale14(v3_cross14(a->up, a->front), local.x),
                                   v3_add(v3_scale14(a->up, local.y), v3_scale14(a->front, local.z))));
    v3 point = xform_point(&view->to_screen, world);
    if(point.z <= FXC(.5f)) return;
    int half_x = (int32_t)((int64_t)frame->half_x * a->figure.scale.x / 16384);
    int half_y = (int32_t)((int64_t)frame->half_y * a->figure.scale.y / 16384);
    int dx = half_x * VIEW_CLIP_X / 256, dy = half_y * VIEW_CLIP_Y / 256;
    v2 lo = project_to_screen(V3(point.x - dx, point.y - dy, point.z));
    v2 hi = project_to_screen(V3(point.x + dx, point.y + dy, point.z));
    int x0 = lo.x >> 8, y0 = lo.y >> 8, x1 = (hi.x + 255) >> 8, y1 = (hi.y + 255) >> 8;
    candle_effect_blit(fb, &ffcx_textures[frame->texture], frame, x0, y0, x1, y1, layer->additive,
                       ffcx_rgb5_index, ffcx_palette);
}
__attribute__((noinline)) static void candle_effect_draw(
    uint8_t* fb, const CameraView* view, const Actor* a){
    if(a->velocity.x > 0){
        int age = candle_effect_age(a, a->velocity.x);
        for(unsigned i = 7; i < 11; i++) candle_effect_layer(fb, view, a, &ffcx_layers[i], a->position, age);
    }
    if(a->velocity.y > 0){
        int age = candle_effect_age(a, a->velocity.y);

        for(unsigned i = 3; i < 7; i++) candle_effect_layer(fb, view, a, &ffcx_layers[i], a->target, age);
    }
}
__attribute__((noinline)) static void candlestand_burn_request(const Actor* a){
    __asm__ volatile("" : : "r"(a) : "memory");
}
static bool candlestand_stream_keep(const Actor* a){
    return a->active && a->type == OBJ_CANDLESTANDICEVOLCANO &&
           (a->state == CST_FIRE || a->state == CST_ATTACK || a->state == CST_FLICKER);
}
static void candlestand_sound(int cue){
    if(cue >= 0) audio_play_effect(cue, 2);
}
#define CANDLE_IGNITE SFX_TORCH_IGNITE
#define CANDLE_OFF    SFX_TORCH_OFF
static void candlestand_init(Actor* a, const StageObject* o, bool own_collision){
    a->figure.scale = V3(64 * o->scale[0] / 256, 64 * o->scale[1] / 256, 64 * o->scale[2] / 256);
    a->radius = 150 * o->scale[0] / 256;
    a->value = -1;

    if(own_collision) mappart_place_kcl(a, kcl_candlestandicevolcano, KCL_CANDLESTANDICEVOLCANO_COUNT);
    int key = actor_switch_key(a, SWITCH_A);
    bool completed = stream_part_state(a->stage_index) & 1u;
    actor_set_state(a, completed || key < 0 ? CST_BURN : CST_WAITFIRE, -1);
    a->velocity = V3(0, 0, 0);
    a->target = a->position;

    if(a->state == CST_BURN) candle_effect_fire(a);
}
static v3 candlestand_centre(const Actor* a){
    return v3_add(a->position, v3_scale14(a->up, 220 * a->figure.scale.y / 64));
}
static bool candlestand_receive_fire(Actor* a, const Actor* by){
    if(!a->active || a->type != OBJ_CANDLESTANDICEVOLCANO || a->state != CST_WAITFIRE) return false;
    v3 centre = candlestand_centre(a);
    int radius = 150 * a->figure.scale.x / 64 + by->radius;
    if(v3_length_squared64(v3_sub(centre, by->position)) > (uint64_t)(radius * radius)) return false;
    actor_set_state(a, CST_FIRE, -1);
    return true;
}
static inline unsigned candlestand_events(int* state, int step){
    unsigned events = 0;
    if(*state == CST_FIRE){
        if(step == 0) events |= CST_EVENT_IGNITE;
        events |= CST_EVENT_BURN_LEVEL;
        if(step == 60){
            *state = CST_BURN;
            events |= CST_EVENT_SWITCH | CST_EVENT_COMPLETED;
        }
    } else if(*state == CST_FLICKER){
        if(step == 0) events |= CST_EVENT_OFF;
        if(step == 30) *state = CST_BURN;
    } else if(*state == CST_ATTACK){
        if(step < 30) events |= CST_EVENT_BURN_LEVEL;
        if(step == 30) *state = CST_BURN;
    } else if(*state == CST_BURN) events |= CST_EVENT_BURN_LEVEL;
    return events;
}
__attribute__((noinline)) static void candlestand_think(Actor* a){
    int step = a->timer - 1, state = a->state;
    unsigned events = candlestand_events(&state, step);
    if(events & CST_EVENT_IGNITE){
        candle_effect_fire(a);
        candlestand_sound(CANDLE_IGNITE);
    }
    if(events & CST_EVENT_OFF){
        candle_effect_off(a);
        candlestand_sound(CANDLE_OFF);
    }
    if(events & CST_EVENT_SWITCH){
        int key = actor_switch_key(a, SWITCH_A);
        if(key >= 0) switch_set(key, true);
    }
    if(events & CST_EVENT_COMPLETED)
        stream_part_state_set(a->stage_index, stream_part_state(a->stage_index) | 1u);
    if(state != a->state){
        bool fire_again = a->state == CST_FLICKER && step == 30;
        candle_effect_shift(a, step);
        actor_set_state(a, state, -1);
        if(fire_again) candle_effect_fire(a);
    }

    if(events & CST_EVENT_BURN_LEVEL) candlestand_burn_request(a);
    if(a->state != CST_BURN) return;
    v3 centre = candlestand_centre(a);
    int spin_range = 350 * a->figure.scale.x / 64,
        fire_range = 50 * a->figure.scale.x / 64 + (g_game.player.action == MA_HANG ? 60 : 100);
    if(g_mario.attack_timer > 0 &&
       v3_length_squared64(v3_sub(centre, mario_centre())) < (uint64_t)spin_range * spin_range){
        candle_effect_shift(a, step);
        actor_set_state(a, CST_FLICKER, -1);
    } else if(v3_length_squared64(v3_sub(centre, mario_centre())) < (uint64_t)fire_range * fire_range){
        if(mario_receive_enemy_attack_fire(centre)){
            candle_effect_shift(a, step);
            actor_set_state(a, CST_ATTACK, -1);
        }
    }
}

static void candlestand_init_with_collision(Actor* a, const StageObject* o){ candlestand_init(a, o, true); }
