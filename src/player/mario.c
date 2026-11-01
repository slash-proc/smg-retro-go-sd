#define C_WALK_SPEED           13.0f
#define C_JUMP_FRONT_SPEED     12.5f
#define C_JUMP_CONNECT_TIME    7
#define C_JUMP_CONNECT_SPEED   R14(0.6f)
#define C_SQUAT_JUMP_FRONT     G(16.5f)
#define C_SQUAT_JUMP_BACK      G(5.0f)
#define C_TURN_JUMP_FRONT      G(5.0f)
#define C_MAX_JUMP_SPEED       G(32.0f)
#define C_HIPDROP_GRAVITY      12.0f
#define C_HIPDROP_LIMIT        G(150.0f)
#define C_HIPDROP_MIN_HEIGHT   FX(100)
#define C_SPIN_JUMP_HEIGHT     G(12.0f)
#define C_SPIN_COOLDOWN        79
#define C_WALL_JUMP_XZ         G(13.0f)
#define C_WALL_JUMP_Y          G(30.0f)
#define C_WALL_STICK_TIME      15
#define C_WALL_RELEASE_TIME    180
#define C_WALL_DROP_SPEED      14.0f
#define C_WALL_STICK_GR_HEIGHT FX(100)
#define C_WALL_SIDE_MOVE_RATIO R14(0.003f)
#define C_WALL_CANCEL_COS      R14(0.1f)
#define C_WALL_TRI_JUMP_MARGIN R14(0.996f)
#define C_WALL_SPIN_FLIP       G(5.0f * 0.75f)
#define C_WALL_SPIN_HOP        G(12.0f)
#define C_WAIT_NEUTRAL_TIMER   15
#define C_DAMAGE_DIST          G(9.0f)
#define C_DAMAGE_HEIGHT        G(15.0f)
#define C_TRAMPLE              G(19.0f)
#define C_TRAMPLE_LONG         G(4.0f)
#define C_INVINCIBLE_TIME      120
#define C_COYOTE_TIME          5
#define C_BUFFER_TIME          4
#define C_STEP_HEIGHT          FX(50)
#define C_SQUAT_BODY_HEIGHT    FX(50)

#define C_SLOW_START_TIME     10
#define C_INERTIA_STD_STOP    R14(0.94f)
#define C_INERTIA_STD_MAX     R14(0.90f)
#define C_INERTIA_START_SPIN  R14(0.95f)
#define C_INERTIA_STOP        R14(0.85f)
#define C_INERTIA_TURN_SLIP   R14(0.91f)
#define C_INERTIA_TURNING     R14(0.85f)
#define C_INERTIA_JUMP_FINISH R14(0.8f)
#define C_INERTIA_SQUAT       R14(0.95f)
#define C_INERTIA_OVER_SPEED  R14(0.8f)
#define C_TURN_ANGLE_SPEED    FXC(0.9f)
#define C_TURN_ANGLE_SPEED2   FXC(0.15f)
#define C_FAST_TURN_SPEED     R14(0.1f)
#define C_WEAK_TURN_TIME      15
#define C_STANDING_TURN_TIME  10
#define C_TURN_SLIP_COS       R14(-0.856889f)
#define C_SLIP_SPEED          R14(0.25f)
#define C_TURN_READY_TIME     10
#define C_TURN_SLIP_TIME      6
#define C_WALL_PUSH_COS       R14(0.939693f)
#define C_SLEEP_TIME          1800
#define C_STEP_IN_PLACE_END   17
#define WALK_INDEX_FULL       7

static const int32_t jump_height[9] = {G(22), G(26), G(36), G(24), G(32), G(15), G(32), G(22), G(1)};
static const int32_t jump_gravity[16] = {G(1.8f), G(1.8f), G(1.8f), G(1.8f), G(1.8f), G(0.95f),
                                         G(1.8f), G(2.2f), G(0.4f), G(0.9f), G(1.8f), G(0.5f),
                                         G(1.8f), G(0.9f), G(1.8f), G(1.0f)};

static EWRAM_BSS MarioPhysics g_mario;

static int g_mario_cling;
static bool g_mario_held;

static EWRAM_BSS MarioTower g_mario_tower;
static int g_mario_star_power;

static fx fx_atan2_fast(int32_t y, int32_t x){
    if(x == 0 && y == 0) return 0;
    int32_t ax = x < 0 ? -x : x, ay = y < 0 ? -y : y;
    bool swap = ay > ax;
    int32_t num = swap ? ax : ay, den = swap ? ay : ax;
    while(den >= (1 << 17)){
        num >>= 1;
        den >>= 1;
    }
    int32_t z = den ? (int32_t)(((uint32_t)num << 14) / (uint32_t)den) : 0;

    int32_t a = mul14(z, R14(0.78539816f)) + mul14(mul14(z, R14(0.2732f)), N14_ONE - z);
    if(swap) a = R14(1.5707963f) - a;
    if(x < 0) a = R14(3.14159265f) - a;
    if(y < 0) a = -a;
    return a >> 6;
}

static fx angle_between14(v3 a, v3 b){
    v3 c = v3_cross14(a, b);
    return fx_atan2_fast(v3_length_exact(c), v3_dot14(a, b));
}

static inline int32_t vertical_speed(v3 v, v3 up){ return v3_dot14(v, up); }
static inline v3 horizontal(v3 v, v3 up){ return v3_reject14(v, up); }

static v3 v3_set_length(v3 v, int32_t length, v3 fallback){
    return v3_scale14(v3_direction14(v, fallback), length);
}

static void mario_start_anim(int group, int anim, int anim2, int interpolation){
    Player* p = &g_game.player;
    p->anim_group = group;
    p->anim = anim;
    p->anim_frame = 0;
    p->anim2 = anim2;
    p->anim2_frame = 0;
    p->anim2_weight = 0;
    p->anim_speed = FX_ONE;
    p->anim_blend_ticks = p->anim_blend_total = interpolation;
    if(interpolation && p->pose_valid){
        for(int i = 0; i < 160; i++)
            for(int k = 0; k < 3; k++) p->pose_from[i][k] = p->pose[i][k];
    }
}

static void mario_set_anim(int anim){
    Player* p = &g_game.player;
    int interpolation = anim_interpolation(anim);
    anim = bee_anim(anim);
    if(p->anim == anim && p->anim_group == ANIM_GROUP_SINGLE) return;
    mario_start_anim(ANIM_GROUP_SINGLE, anim, -1, interpolation);
}

static void mario_restart_anim(int anim){
    mario_start_anim(ANIM_GROUP_SINGLE, bee_anim(anim), -1, anim_interpolation(anim));
}

static void mario_set_group(int group, int anim, int anim2, int weight2, int interpolation){
    Player* p = &g_game.player;
    anim = bee_anim(anim);
    if(anim2 >= 0) anim2 = bee_anim(anim2);
    if(p->anim_group != group) mario_start_anim(group, anim, anim2, interpolation);
    if(p->anim2 != anim2) p->anim2_frame = 0;
    p->anim = anim;
    p->anim2 = anim2;
    p->anim2_weight = weight2;
}

static bool mario_anim_done(void){
    Player* p = &g_game.player;
    return (p->anim_frame >> 8) >= anim_length(p->anim) - 1;
}

__attribute__((noinline)) static void mario_close_swim(void){
    mario_swim_reset();
    g_game.player.figure.local_pitch = 0;
}

static void mario_set_action(MarioAction a){
    Player* p = &g_game.player;

    if(p->action == MA_SWIM && a != MA_SWIM) mario_close_swim();

    if((a == MA_DEATH || a == MA_STAR_GET) && p->action != a) audio_stop_sub_music();
    p->action = a;
    p->action_timer = 0;
}

static void mario_stop_walk(void){
    MarioPhysics* m = &g_mario;
    m->walk_index = 0;
    m->ratio = 0;
    m->brake_first = 0;
    m->brake_second = 0;
    m->run_weight = 0;
    m->turning = false;
    m->turn_ready = 0;
    m->turn_armed = false;
    m->start_spin = 0;
    m->run_end = false;
    m->brake_pose = false;
}

static void mario_record_turn_slip(void){
    MarioPhysics* m = &g_mario;
    m->turn_front = g_game.player.front;
    m->turn_ready = C_TURN_READY_TIME;
}

static bool mario_stick_back(void){
    MarioPhysics* m = &g_mario;
    return m->stick && v3_dot14(m->move_dir, g_game.player.front) < -R14(0.5f);
}

static void mario_take_off(int type, v3 jump_vec, bool digital){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    m->jump_type = type;
    m->jump_vec = jump_vec;
    m->jumping = true;
    m->hold = true;
    m->digital = digital;
    m->air_frames = 0;
    m->back_frames = 0;
    m->squat = false;
    m->skidding = false;
    m->skid_timer = 0;
    m->hipdrop_timer = 0;
    m->turn_ready = 0;
    m->turn_armed = false;
    m->brake_pose = false;
    m->run_end = false;
    m->step_ticks = 0;
    m->idle_stage = 0;
    m->idle_ticks = 0;
    p->on_ground = false;
}

static void mario_jump(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    v3 up = p->up, front = p->front;

    if(m->skidding || (m->turn_ready && mario_stick_back())){
        if(m->turn_ready){
            front = v3_tangent14(v3_neg(m->turn_front), up, v3_neg(front));
            p->front = front;
        }
        v3 v = v3_add(v3_scale14(front, C_TURN_JUMP_FRONT), v3_scale14(up, jump_height[JUMP_TURN]));
        m->ratio = 0;
        mario_take_off(JUMP_TURN, v, true);
        m->chainable = false;
        mario_set_action(MA_TURN_JUMP);
        mario_restart_anim(ANIM_MARIO_JUMPTURN);
        audio_play_effect(SFX_JUMP_FLIP, 2);
        return;
    }
    m->wall_free = !p->on_wall;
    if(m->squat && m->ratio > R14(0.3f)){
        v3 v = v3_add(v3_scale14(front, C_SQUAT_JUMP_FRONT), v3_scale14(up, jump_height[JUMP_LONG]));
        mario_take_off(JUMP_LONG, v, true);
        m->chainable = false;
        mario_set_action(MA_LONG_JUMP);
        mario_restart_anim(ANIM_MARIO_JUMPBROAD);
        audio_play_effect(SFX_JUMP_LONG, 2);
        return;
    }
    if(m->squat){
        v3 v = v3_add(v3_scale14(front, -C_SQUAT_JUMP_BACK), v3_scale14(up, jump_height[JUMP_BACK]));
        m->ratio = 0;
        mario_take_off(JUMP_BACK, v, true);
        m->chainable = false;
        mario_set_action(MA_BACKFLIP);
        mario_restart_anim(ANIM_MARIO_JUMPBACK);
        audio_play_effect(SFX_JUMP_FLIP, 2);
        return;
    }
    int type = JUMP_FIRST;
    if(m->frames_since_landing <= C_JUMP_CONNECT_TIME && m->chainable) type = m->jump_type + 1;
    if(type > JUMP_TRIPLE) type = JUMP_FIRST;
    if(type == JUMP_TRIPLE && m->ratio < C_JUMP_CONNECT_SPEED) type = JUMP_FIRST;

    int32_t forward = mul14(G(C_JUMP_FRONT_SPEED), m->ratio);
    v3 v = v3_add(v3_scale14(front, forward), v3_scale14(up, jump_height[type]));
    mario_take_off(type, v, false);
    m->chainable = true;
    mario_set_action(type == JUMP_FIRST ? MA_JUMP : type == JUMP_SECOND ? MA_JUMP2 : MA_JUMP3);
    mario_restart_anim(type == JUMP_FIRST    ? ANIM_MARIO_JUMP
                       : type == JUMP_SECOND ? ANIM_MARIO_JUMP2
                                             : ANIM_MARIO_JUMPROLL);
    audio_play_effect(type == JUMP_FIRST ? SFX_JUMP : type == JUMP_SECOND ? SFX_JUMP2 : SFX_JUMP3, 2);
}

static void mario_air_spin(void){
    if(mario_fireball_spin(actors_player_carrying())) return;
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    m->attack_timer = 15;
    if(m->spin_used || m->spin_timer > 0){
        mario_set_action(MA_SPIN);
        mario_restart_anim(ANIM_MARIO_SPIN);
        audio_play_effect(SFX_SPIN, 1);
        return;
    }
    v3 v = v3_add(v3_scale14(p->front, mul14(G(5.0f), m->ratio)), v3_scale14(p->up, C_SPIN_JUMP_HEIGHT));
    m->jump_type = JUMP_SPIN;
    m->jump_vec = v;
    m->spin_timer = C_SPIN_TIME;
    m->ratio = 0;
    m->chainable = false;
    m->attack_timer = 25;
    mario_set_action(MA_SPIN);
    mario_restart_anim(ANIM_MARIO_SPIN);
    audio_play_effect(SFX_SPIN, 1);
}

static void mario_start_hipdrop(void){
    MarioPhysics* m = &g_mario;
    m->jump_vec = V3(0, 0, 0);
    m->spin_timer = 0;
    m->hipdrop_timer = 0;
    mario_set_action(MA_HIPDROP_START);
    mario_restart_anim(ANIM_MARIO_HIPDROPSTART);
    audio_play_effect(SFX_HIPDROP_START, 1);
}

static void mario_land(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    MarioAction prev = p->action;
    m->jumping = false;
    m->frames_since_landing = 0;
    m->spin_used = false;
    m->spin_timer = 0;
    m->stomp_count = 0;
    p->on_ground = true;
    if(prev == MA_SWIM) return;
    if(prev == MA_FIRE_DANCE || prev == MA_FIRE_RUN) return;
    if(prev == MA_FREEZE) return;
    if(mario_skate_land()) return;
    if(flight_land()) return;
    m->wall_cancel = false;

    if(prev == MA_WALL_SLIDE){
        p->front = v3_tangent14(p->wall_normal, p->up, p->front);
        mario_stop_walk();
        mario_set_action(MA_LAND);
        mario_start_anim(ANIM_GROUP_SINGLE, ANIM_MARIO_LAND, -1, 1);
        m->push_after = v3_scale14(p->wall_normal, G(30.0f));
        audio_play_effect(SFX_LAND, 0);
        return;
    }

    if(prev == MA_HIPDROP){
        m->ratio = 0;
        mario_set_action(MA_HIPDROP_LAND);
        mario_restart_anim(ANIM_MARIO_HIPDROPLAND);
        audio_play_effect(SFX_HIPDROP_LAND, 3);
        g_game.camera.recentre_ticks = 0;
        return;
    }
    if(prev == MA_DAMAGE || prev == MA_DAMAGE_FALL){
        mario_set_action(MA_DAMAGE);
        p->action_timer = 20;
        mario_restart_anim(ANIM_MARIO_DAMAGELAND);
        return;
    }
    audio_play_effect(SFX_LAND, 0);

    v3 h = horizontal(m->jump_vec, p->up);
    int32_t fwd = v3_dot14(h, p->front);
    bool special = m->jump_type == JUMP_TRIPLE || m->jump_type == JUMP_TURN || m->jump_type == JUMP_LONG ||
                   m->jump_type == JUMP_BACK || m->jump_type == JUMP_SPIN;
    if(m->stick && m->back_frames == 0){
        mario_set_action(MA_WAIT);
        mario_record_turn_slip();
        return;
    }
    if(!special && fwd > G(6.0f)){
        m->ratio = fx_clamp((int32_t)(((int64_t)fwd << 14) / G(C_JUMP_FRONT_SPEED)), 0, R14(2.0f));
        m->brake_first = 0;
        mario_set_action(MA_WAIT);
        mario_record_turn_slip();
        return;
    }
    mario_stop_walk();
    mario_set_action(MA_LAND);
    int anim = ANIM_MARIO_LAND;
    switch(m->jump_type){
    case JUMP_SECOND: anim = ANIM_MARIO_JUMP2LAND; break;
    case JUMP_TRIPLE: anim = ANIM_MARIO_JUMPROLLLAND; break;
    case JUMP_TURN: anim = ANIM_MARIO_JUMPTURNLAND; break;
    case JUMP_LONG: anim = ANIM_MARIO_JUMPBROADLAND; break;
    case JUMP_BACK: anim = ANIM_MARIO_JUMPBACKLAND; break;
    default: break;
    }
    mario_restart_anim(anim);
}

static void mario_damage(v3 from, int amount){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    if((g_debug_options & DEBUG_NO_DAMAGE) || g_mario_star_power > 0 || g_mario_held) return;
    if(mario_freeze_blocks_damage()) return;
    if(p->invincible_timer > 0 || p->action == MA_DAMAGE || p->action == MA_DAMAGE_FALL ||
       p->action == MA_DEATH)
        return;
    if(p->action == MA_TALK || p->action == MA_FIRE_DANCE || p->action == MA_FIRE_RUN) return;
    if(p->action == MA_STAR_LAUNCH) return;
    bee_end();
    mario_power_end();
    int previous_health = p->health;
    p->health -= amount;
    mario_lifeup_health_changed(previous_health);
    actors_player_damaged();
    v3 away = horizontal(v3_sub(p->figure.position, from), p->up);
    away = v3_tangent14(v3_direction14(away, v3_neg(p->front)), p->up, v3_neg(p->front));
    bool from_front = v3_dot14(away, p->front) < 0;
    mario_take_off(JUMP_DAMAGE, v3_add(v3_scale14(away, C_DAMAGE_DIST), v3_scale14(p->up, C_DAMAGE_HEIGHT)),
                   true);
    m->ratio = 0;
    m->spin_timer = 0;
    p->invincible_timer = C_INVINCIBLE_TIME;
    mario_set_action(MA_DAMAGE_FALL);
    mario_restart_anim(from_front ? ANIM_MARIO_DAMAGEFRONT : ANIM_MARIO_DAMAGEBACK);
    audio_play_effect(SFX_DAMAGE, 4);
}

static void mario_stomp_bounce(bool a_held){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    v3 h = horizontal(m->jump_vec, p->up);
    int32_t up_speed = C_TRAMPLE + (a_held ? C_TRAMPLE_LONG : 0);
    mario_take_off(JUMP_FORCED, v3_add(h, v3_scale14(p->up, up_speed)), true);
    m->spin_used = false;
    m->stomp_count++;
    mario_set_action(MA_JUMP);
    mario_restart_anim(m->stomp_count == 1   ? ANIM_MARIO_JUMP
                       : m->stomp_count == 2 ? ANIM_MARIO_JUMPPRESS2ND
                                             : ANIM_MARIO_JUMPPRESS3RD);
}

static void mario_spin_hop(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    if(!m->jumping) return;
    int32_t vs = vertical_speed(m->jump_vec, p->up);
    int32_t want = vs < 0 ? G(15.0f) : fx_min(vs + G(10.0f), G(10.0f));
    if(want > vs) m->jump_vec = v3_add(horizontal(m->jump_vec, p->up), v3_scale14(p->up, want));
}

static void mario_nudge(v3 d){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    m->position16 = v3_add(m->position16, V3(d.x << 8, d.y << 8, d.z << 8));
    p->figure.position = v3_add(p->figure.position, d);
}

static void mario_hold_begin(void){
    MarioPhysics* m = &g_mario;
    g_mario_held = true;
    m->jumping = true;
    m->jump_vec = V3(0, 0, 0);
    mario_set_action(MA_FALL);
    mario_restart_anim(ANIM_MARIO_SPACEFLY);
}

static void mario_hold_move(v3 position, v3 front){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    p->figure.position = position;
    m->position16 = V3(position.x << 8, position.y << 8, position.z << 8);
    p->front = v3_tangent14(front, p->up, p->front);
}

static void mario_hold_release(v3 velocity){
    MarioPhysics* m = &g_mario;
    g_mario_held = false;
    mario_take_off(JUMP_FORCED, velocity, true);
    m->spin_used = false;
    mario_set_action(MA_FALL);
    mario_set_anim(ANIM_MARIO_FALL);
}

static void mario_update_gravity(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    if(bee_update_gravity()) return;
    bool free_sling = flight_free_sling_power_jump();

    v3 probe = v3_add(p->figure.position,
                      v3_scale14(free_sling ? p->gravity_up : p->up, free_sling ? FX(60) : C_BODY_HEIGHT));
    GravitySample g = gravity_sample(g_game.stage.header, probe);
    if(g.index < 0 && g_game.stage.header->gravity_count){
        m->void_ticks++;
        if(!p->on_ground || m->jumping || free_sling) return;
        g.up = p->ground_normal;
    } else m->void_ticks = 0;
    if(free_sling) g.up = flight_sling_gravity(p->gravity_up, g.up, &m->jump_vec);
    p->gravity_up = g.up;
    int32_t d = v3_dot14(g.up, p->up);
    int32_t rate = d < 0 ? R14(0.5f) : d < R14(0.99f) ? R14(0.1f) : R14(0.25f);
    if(d > N14_ONE - 4) return;
    v3 old_up = p->up;
    p->up = v3_blend14(p->up, g.up, rate);
    p->front = v3_tangent14(v3_rotate_between14(p->front, old_up, p->up), p->up, p->front);
    if(m->jumping && !free_sling){
        m->jump_vec = v3_rotate_between14(m->jump_vec, old_up, p->up);
    }
}

static void mario_screen_ground_basis(const Camera* cam, v3 up, v3* forward, v3* right){
    v3 screen_right = v3_cross14(cam->front, cam->up);
    int32_t y = v3_dot14(cam->up, up), z = -v3_dot14(cam->front, up);
    v3 ortho;
    if(fx_abs(z) > fx_abs(y)) ortho = z < 0 ? v3_neg(cam->up) : cam->up;
    else ortho = y < 0 ? v3_neg(cam->front) : cam->front;
    *right = v3_direction14(v3_cross14(ortho, up), screen_right);
    *forward = v3_direction14(v3_cross14(up, screen_right), ortho);
}

static void mario_read_stick(const KeyState* keys){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    const Camera* cam = &g_game.camera;
    int right = (keys->held & KEY_RIGHT) != 0, left = (keys->held & KEY_LEFT) != 0;
    int up = (keys->held & KEY_UP) != 0, down = (keys->held & KEY_DOWN) != 0;
    if((keys->held & KEY_L) && (left || right)) left = right = 0;
    if((keys->held & KEY_L)) up = down = 0;
    int sx = right - left, sy = up - down;
    if(g_mario_tower.on) sy = 0;
    int bits = (sx + 1) | (sy + 1) << 2;
    m->stick_changed = bits != m->stick_keys;
    m->stick_keys = bits;
    if(!sx && !sy){
        m->stick = 0;
        m->move_dir = V3(0, 0, 0);
        return;
    }
    v3 forward, rightv;
    mario_screen_ground_basis(cam, p->up, &forward, &rightv);
    v3 dir = v3_add(v3_scale14(forward, sy * N14_ONE), v3_scale14(rightv, sx * N14_ONE));
    if(g_mario_tower.on){
        v3 radial = v3_direction14(
            v3_reject14(v3_sub(p->figure.position, g_mario_tower.base), g_mario_tower.axis), forward);
        dir = v3_reject14(v3_reject14(dir, g_mario_tower.axis), radial);
        if(v3_length_squared64(dir) < (uint64_t)1024 * 1024){
            m->stick = 0;
            m->move_dir = V3(0, 0, 0);
            return;
        }
    }
    m->move_dir = v3_direction14(dir, forward);
    m->stick = N14_ONE;
}

static fx signed_angle14(v3 from, v3 to, v3 up){
    fx a = angle_between14(from, to);
    return v3_dot14(v3_cross14(from, to), up) >= 0 ? a : -a;
}

static void mario_turn(v3 dir){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    if(!m->stick){
        m->turning = false;
        if(m->turn_frames) m->turn_frames--;
        return;
    }
    int32_t dot = v3_dot14(p->front, dir);
    int sign = v3_dot14(v3_cross14(p->front, dir), p->up) >= 0 ? 1 : -1;
    fx rate = m->ratio > C_FAST_TURN_SPEED ? C_TURN_ANGLE_SPEED2 : C_TURN_ANGLE_SPEED;
    if(m->turning || m->stick_changed){
        if(sign != m->turn_sign && dot >= 0) m->turn_frames = 0;
        else if(m->turn_frames < C_WEAK_TURN_TIME) m->turn_frames++;
        if(m->walk_index > 4) rate = fx_mul(rate, fx_sqrt((m->turn_frames << 8) / C_WEAK_TURN_TIME));
    } else {
        if(m->turn_frames) m->turn_frames--;
        if(m->turn_frames == 0) rate /= 100;
    }
    m->turn_sign = sign;
    m->turning = dot <= R14(0.99f);
    fx angle = angle_between14(p->front, dir);
    if(angle < FXC(0.1f) || rate >= angle){
        p->front = dir;
        return;
    }
    if(dot < 0 && v3_length_exact(v3_cross14(p->front, dir)) < R14(0.02f)){
        m->turn_frames = 0;
        p->front = v3_tangent14(v3_rotate_axis14(p->front, p->up, FXC(0.3927f)), p->up, p->front);
        return;
    }
    p->front = v3_tangent14(v3_rotate_axis14(p->front, p->up, sign * rate), p->up, p->front);
}

static void mario_ground_move(const KeyState* keys){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    bool frozen = false;
    v3 dir = m->move_dir;
    if(!m->stick){
        dir = p->front;
        if((m->turn_ready || m->ratio) && (m->last_dir.x | m->last_dir.y | m->last_dir.z))
            dir = v3_tangent14(m->last_dir, p->up, p->front);
    }
    m->hold_still = false;

    if(m->stick && !(m->walk_index && m->prev_walk_index) && !m->brake_pose && !m->skidding &&
       !m->turn_armed && m->ratio < R14(0.05f) && !m->squat){
        fx angle = angle_between14(p->front, dir);
        if(angle > FXC(0.1745f)){
            m->turn_yaw = signed_angle14(dir, p->front, p->up);
            m->stand_turn = C_STANDING_TURN_TIME;
            p->front = dir;
            if(!m->step_ticks) m->step_ticks = C_STEP_IN_PLACE_END;
            m->hold_still = true;
        } else {
            p->front = dir;
        }
    }

    int32_t dot = v3_dot14(p->front, dir);
    bool reversed = dot < C_TURN_SLIP_COS;
    if(!m->hold_still){
        if(m->turn_armed && !m->squat && mario_stick_back()){
            m->brake_pose = true;
            m->run_end = false;
            m->brake_second = 0;
        } else if(m->brake_pose){
            m->brake_pose = false;
            m->run_end = true;
            m->brake_second = 0;
            m->walk_index = 0;
        }

        bool flipped =
            m->stick && m->prev_stick && reversed && v3_dot14(p->front, m->prev_dir) >= C_TURN_SLIP_COS;
        if(p->action != MA_SKATE && ((!reversed && m->prev_stick > m->stick) || flipped) && !m->skidding &&
           m->ratio > C_SLIP_SPEED){
            mario_record_turn_slip();
            if(flipped) m->turn_armed = true;
        } else if(m->turn_ready > 0){
            m->turn_ready--;
        }
        if(m->turn_ready){
            if(!m->stick) m->turn_armed = true;
            if(m->ratio < R14(0.1f)){
                m->turn_ready = 0;
                m->ratio = 0;
            }
            if(m->brake_pose || m->turn_armed) frozen = true;
        } else {
            if(p->action != MA_SKATE && !m->squat && m->turn_armed && reversed && !m->skidding){
                p->front = v3_tangent14(v3_neg(m->turn_front), p->up, v3_neg(p->front));
                m->skid_timer = C_TURN_SLIP_TIME;
                m->skidding = true;
                m->ratio = 0;
                m->brake_pose = false;
                m->run_end = false;
                m->step_ticks = 0;
                mario_set_action(MA_SKID);
                mario_restart_anim(ANIM_MARIO_TURN);
                audio_play_effect(SFX_SKID, 0);
            }
            m->turn_armed = false;
        }

        if(m->skid_timer > 0){
            m->skid_timer--;
            m->ratio = 0;
        }
        if(m->skidding && m->skid_timer == 0){
            if(m->stick && v3_dot14(m->move_dir, p->front) < 0){
                p->front = v3_tangent14(v3_neg(p->front), p->up, p->front);
                m->skidding = false;
                mario_set_action(MA_WAIT);
            } else if(mario_anim_done()){
                m->skidding = false;
                mario_set_action(MA_WAIT);
                if(!m->stick) mario_stop_walk();
            }
        }

        if(!m->skidding){
            if(!frozen) mario_turn(dir);
        } else if(m->skid_timer == 0 && m->stick){
            fx rate = m->ratio > C_FAST_TURN_SPEED ? C_TURN_ANGLE_SPEED2 : C_TURN_ANGLE_SPEED;
            fx angle = signed_angle14(p->front, dir, p->up);
            if(angle > rate) angle = rate;
            if(angle < -rate) angle = -rate;
            p->front = v3_tangent14(v3_rotate_axis14(p->front, p->up, angle), p->up, p->front);
        }

        if(m->stand_turn && m->walk_index == 0 && !m->squat){
            if(!m->step_ticks) m->step_ticks = C_STEP_IN_PLACE_END;
        } else if(m->walk_index){
            m->step_ticks = 0;
        }
    }

    int32_t target = m->walk_index ? N14_ONE : 0;
    if(m->dash) target = mul14(target, R14(1.5f));

    if(g_mario_cling) target = fx_min(target, fx_max(0, (5 - g_mario_cling) * N14_ONE / WALK_INDEX_FULL));
    if(m->squat) target = 0;
    if(target == 0) m->slow_start = C_SLOW_START_TIME;
    else if(m->slow_start > 0){
        int32_t f = ((C_SLOW_START_TIME - m->slow_start) * N14_ONE) / C_SLOW_START_TIME;
        target = mul14(target, mul14(f, f));
        m->slow_start--;
    }
    if(m->skid_timer || m->turn_armed) target = 0;

    int32_t inertia;
    if(m->ratio > N14_ONE){
        inertia = C_INERTIA_OVER_SPEED;
    } else {
        inertia = C_INERTIA_STD_STOP + mul14(m->ratio, C_INERTIA_STD_MAX - C_INERTIA_STD_STOP);
        inertia += mul14(m->start_spin, C_INERTIA_START_SPIN - inertia);
        if(target == 0){
            inertia = C_INERTIA_STOP;
            if(m->turn_armed) inertia = C_INERTIA_TURN_SLIP;
            if(m->skidding) inertia = C_INERTIA_TURNING;
            if(m->frames_since_landing < 30) inertia = C_INERTIA_JUMP_FINISH;
            if(m->squat) inertia = C_INERTIA_SQUAT;
        }
    }
    if(p->ground_attr & COL_SLIPPERY) inertia = R14(0.985f);
    if(p->ground_attr & COL_ICE)
        inertia = p->action == MA_SKATE ? mario_ice_inertia(m->ratio, m->start_spin)
                                        : mario_ice_walk_inertia(m->ratio, m->start_spin);
    m->ratio = mul14(m->ratio, inertia) + mul14(target, N14_ONE - inertia);

    if(m->squat && m->ratio < R14(0.05f) && m->stick){
        m->ratio = R14(0.2f);
        mario_turn(dir);
    }

    m->prev_walk_index = m->walk_index;
    m->walk_index = m->stick && !m->squat ? WALK_INDEX_FULL : 0;
    m->prev_stick = m->stick;
    m->prev_dir = dir;
    if(m->stick && !m->turn_ready) m->last_dir = m->move_dir;
}

static GBA_ARM_TARGET_ATTR __attribute__((noinline)) void mario_begoma_air_control(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    int32_t vs = vertical_speed(m->jump_vec, p->up);
    v3 h = horizontal(m->jump_vec, p->up);
    if(m->stick && m->no_control_timer == 0){
        int32_t accel = vs <= 0 ? G(13.0f * 5 / 300) : G(13.0f * 3) / (m->air_frames + 300);
        h = v3_add(h, v3_scale14(m->move_dir, accel));
    }
    if(v3_length_squared64(h) > (uint64_t)((int64_t)G(7) * G(7))) h = v3_set_length(h, G(7), p->front);
    m->jump_vec = v3_add(h, v3_scale14(p->up, vs));
}

static void mario_air_control(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    v3 up = p->up;
    if(m->jump_type == 11){
        mario_begoma_air_control();
        return;
    }
    int32_t vs = vertical_speed(m->jump_vec, up);
    v3 h = horizontal(m->jump_vec, up);
    if(m->jump_type == JUMP_BELLY || m->no_control_timer > 0){
        m->jump_vec = v3_add(h, v3_scale14(up, vs));
        return;
    }
    v3 move = m->move_dir;
    int32_t fwd = v3_dot14(move, p->front);
    v3 side = v3_sub(move, v3_scale14(p->front, fwd));
    if(fwd >= 0 || !m->stick){
        int begin = m->jump_type == JUMP_LONG ? 30 : 15;
        if(m->air_frames > begin) h = v3_scale14(h, m->jump_type == JUMP_LONG ? R14(0.99f) : R14(0.98f));
    } else {
        m->back_frames++;
        int32_t k = p->action == MA_TURN_JUMP ? R14(0.1f)
                    : m->back_frames < 60     ? (int32_t)(R14(3.0f) * (60 - m->back_frames) / 60)
                                              : 0;
        move = v3_add(side, v3_scale14(p->front, mul14(fwd, k)));
    }
    if(m->stick){
        int32_t a = v3_dot14(v3_direction14(move, p->front), v3_direction14(h, p->front));
        if(a < 0 && (h.x | h.y | h.z)) h = v3_add(h, v3_scale14(h, mul14(R14(0.1f), a)));
        int32_t accel = m->jump_type == JUMP_SPIN ? G(13.0f * 5 / 300) : G(13.0f * 3) / (m->air_frames + 300);
        h = v3_add(h, v3_scale14(move, accel));
    }
    int32_t cap = m->jump_type == JUMP_SPIN ? G(6.0f) : C_MAX_JUMP_SPEED;
    if(v3_length_squared64(h) > (uint64_t)((int64_t)cap * cap)) h = v3_set_length(h, cap, p->front);
    m->jump_vec = v3_add(h, v3_scale14(up, vs));
}

static v3 mario_air_physics(const KeyState* keys){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    if(bee_owns_air()) return bee_air_physics(keys);
    bool free_sling = flight_free_sling_power_jump();
    v3 up = free_sling ? p->gravity_up : p->up;
    if(!(keys->held & KEY_A)) m->hold = false;
    int32_t vs = vertical_speed(m->jump_vec, up);
    bool rising = vs > 0;
    bool half = m->hold || m->digital || !rising || m->jump_type == JUMP_TRIPLE;
    int32_t accel = jump_gravity[m->jump_type];

    if(m->jump_type == 11){
        if(rising) m->hipdrop_timer = 0;
        else ++m->hipdrop_timer;
        if(rising || m->hipdrop_timer < 12) accel = jump_gravity[JUMP_FIRST];
    }
    if(g_mario_cling) accel = accel * (2 + g_mario_cling) / 2;
    if(m->jump_type == JUMP_SPIN){
        accel = G(0.25f);
        if(!rising) accel = mul14(accel, R14(0.25f));
    }
    int32_t scale = half ? C_GRAVITY_RATIO_A : N14_ONE;
    if(m->jump_type == JUMP_DAMAGE) scale = N14_ONE;
    if(m->jump_type == JUMP_FORCED) scale = C_GRAVITY_RATIO_A;
    if(m->jump_type == 13) scale = N14_ONE;

    if(free_sling) scale = flight_sling_gravity_scale(up, &m->jump_vec);
    else mario_air_control();
    v3 displacement = m->jump_vec;
    if(!rising && m->stick && m->no_control_timer == 0 && m->jump_type != JUMP_SPIN && m->jump_type != 11)
        displacement = v3_add(displacement, v3_scale14(m->move_dir, G(13.0f * 0.15f)));
    m->jump_vec = v3_sub(m->jump_vec, v3_scale14(up, mul14(accel, scale)));
    vs = vertical_speed(m->jump_vec, up);
    int32_t max_drop = (m->jump_type == 11 && !rising && m->hipdrop_timer >= 12) ? G(6.0f) : C_MAX_DROP_SPEED;
    if(vs < -max_drop) m->jump_vec = v3_add(horizontal(m->jump_vec, up), v3_scale14(up, -max_drop));
    m->air_frames++;
    return displacement;
}

static inline bool mario_body_face_is_floor(uint8_t floor_code, int32_t ny, bool dry){
    return floor_code == 3 && dry ? ny >= 8213 : ny >= R14(0.17f);
}

static void mario_move_and_collide(v3 displacement, bool want_ground){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    v3 up = p->up;
    v3 old_feet = p->figure.position;
    m->position16 = v3_add(m->position16, displacement);
    if(g_mario_tower.on){
        v3 at = V3(m->position16.x >> 8, m->position16.y >> 8, m->position16.z >> 8), ax = g_mario_tower.axis;
        v3 rel = v3_sub(at, g_mario_tower.base);
        v3 radial = v3_direction14(v3_reject14(rel, ax), v3_neg(p->up));
        v3 fixed = v3_add(g_mario_tower.base, v3_add(v3_scale14(ax, v3_dot14(rel, ax)),
                                                     v3_scale14(radial, g_mario_tower.radius - FX(200))));
        v3 d = v3_sub(fixed, at);
        m->position16 = v3_add(m->position16, V3(d.x << 8, d.y << 8, d.z << 8));
    }
    v3 feet = V3(m->position16.x >> 8, m->position16.y >> 8, m->position16.z >> 8);
    fx body_height = m->squat ? C_SQUAT_BODY_HEIGHT : C_BODY_HEIGHT;

    CollisionCandidates cand;
    v3 c_old = v3_add(old_feet, v3_scale14(up, body_height)),
       c_new = v3_add(feet, v3_scale14(up, body_height));
    collision_gather_around(c_old, c_new, C_BODY_RADIUS + FX(120), &cand);

    m->air_vec = m->jump_vec;
    p->on_wall = false;
    p->wall_code = 0;
    bool burn_wall = false;
    v3 burn_normal = up;
    v3 push_total = V3(0, 0, 0);

    bool stepping = p->on_ground && !(m->jumping && vertical_speed(m->jump_vec, up) > 0);
    for(int iter = 0; iter < 2; iter++){
        v3 c = v3_add(v3_add(feet, push_total), v3_scale14(up, body_height));
        bool pushed = false;
        for(int i = 0; i < cand.count; i++){
            SphereContact sc;
            if(!collision_sphere_triangle(cand.index[i], c, C_BODY_RADIUS, &sc)) continue;
            const CollisionTriangle* tri = col_tri(sc.tri);
            int32_t ny = v3_dot14(v3_from_i16(tri->normal), up);
            if(mario_body_face_is_floor(tri->floor_code, ny, p->water_state == WATER_DRY) &&
               !(tri->attr & COL_FLOOR_ONLY && ny < 0))
                continue;
            if(stepping && ny > -R14(0.17f) && !(tri->attr & COL_DEATH) &&
               v3_dot14(v3_sub(sc.point, feet), up) < C_STEP_HEIGHT)
                continue;

            if(ny <= -R14(0.17f) && v3_dot14(v3_sub(sc.point, c), up) < 0) continue;
            v3 n = sc.normal;
            if(ny > -R14(0.17f)){
                n = v3_tangent14(n, up, n);
                p->on_wall = true;
                p->wall_normal = n;
                p->wall_code = tri->wall_code;
                m->wall_ny = ny;
            }
            v3 push = v3_scale14(n, sc.depth + 1);
            pushed = true;
            push_total = v3_add(push_total, push);
            c = v3_add(c, push);

            int32_t into = v3_dot14(m->jump_vec, n);
            if(into < 0) m->jump_vec = v3_sub(m->jump_vec, v3_scale14(n, into));
            if(tri->attr & COL_DEATH){
                int previous_health = p->health;
                p->health = 0;
                mario_lifeup_health_changed(previous_health);
            }
            if(tri->attr & COL_BURN){
                burn_wall = true;
                burn_normal = n;
            }
        }
        if(!pushed) break;
    }
    feet = v3_add(feet, push_total);

    int32_t vs = vertical_speed(m->jump_vec, up);
    bool rising = m->jumping && vs > 0;
    fx snap = want_ground ? FX(60) : 0;

    fx drop = fx_max(0, v3_dot14(v3_sub(old_feet, feet), up));
    fx reach = C_STEP_HEIGHT + drop;
    v3 from = v3_add(feet, v3_scale14(up, reach));

    fx margin = 1 + (p->action == MA_HIPDROP ? 1 << g_collision.shift : 0);
    v3 to = v3_sub(feet, v3_scale14(up, snap + margin));
    RayHit hit;
    bool landed = false;
    bool ground_hit = !rising &&
        (drop > C_STEP_HEIGHT ? collision_ray(from, to, &hit, 0)
                             : collision_raycast(&cand, from, to, &hit, 0));
    if(ground_hit && v3_dot14(hit.normal, up) >= R14(0.17f)){
        landed = true;
        feet = hit.point;
        p->ground_normal = hit.normal;
        p->ground_attr = hit.attr;
        p->ground_code = col_tri(hit.tri)->floor_code;
        if(hit.attr & COL_DEATH){
            int previous_health = p->health;
            p->health = 0;
            mario_lifeup_health_changed(previous_health);
        }
    }

#if defined(__GNW__)
    if(p->on_ground && !landed && !rising){
        /* Large drops use a separate ray broad phase. Recreate only that
         * exceptional query for the trace; ordinary steps reuse body candidates. */
        if(drop > C_STEP_HEIGHT) collision_gather_around(from, to, FXC(0.05f), &cand);
        collision_record_support_loss(old_feet, feet, up, from, to, &cand, ground_hit, &hit);
    }
#endif
    if(landed){
        p->has_floor_below = true;
        p->floor_point = feet;
        p->height_above_ground = 0;
    } else if((g_game.tick & 1) || !p->was_on_ground || p->action == MA_HIPDROP){
        RayHit below;
        v3 b0 = v3_add(feet, v3_scale14(up, FX(10)));
        v3 b1 = v3_sub(feet, v3_scale14(up, FX(900)));
        if(collision_ray(b0, b1, &below, 0)){
            p->has_floor_below = true;
            p->floor_point = below.point;
            p->height_above_ground = v3_dot14(v3_sub(feet, below.point), up);
        } else {
            p->has_floor_below = false;
            p->height_above_ground = FX(900);
        }
    }

    m->position16 = V3(feet.x << 8, feet.y << 8, feet.z << 8);
    p->figure.position = feet;
    p->was_on_ground = p->on_ground;
    if(landed){
        if(m->jumping){
            mario_land();
        }
        p->on_ground = true;
        p->coyote_timer = C_COYOTE_TIME;
        m->last_safe_position = feet;
        m->last_safe_up = p->up;
        mario_save_safe_point(&hit);
    } else if(!m->jumping && p->action != MA_CLIMB_UP){
        if(p->coyote_timer > 0 && m->stick){
            p->coyote_timer--;
        } else {
            v3 v = v3_scale14(p->front, mul14(G(C_WALK_SPEED * 2.0f / 13.0f), m->ratio));
            mario_take_off(JUMP_FIRST, v, false);
            m->hold = false;
            m->chainable = false;
            if(p->action != MA_DAMAGE && p->action != MA_FIRE_DANCE && p->action != MA_FIRE_RUN &&
               p->action != MA_FREEZE){
                mario_set_action(MA_FALL);
                mario_set_anim(ANIM_MARIO_FALL);
            }
        }
    } else {
        p->on_ground = false;
    }
    if(burn_wall || (landed && (hit.attr & COL_BURN)))
        mario_fire_hit(burn_wall ? burn_normal : hit.normal, burn_wall);
}

static v3 mario_wall_push_move(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    v3 n = p->wall_normal;
    m->ratio = 0;

    v3 centre = v3_add(p->figure.position, v3_scale14(p->up, C_BODY_HEIGHT));
    RayHit hit;
    bool wall = p->on_wall;
    if(!wall && collision_ray(centre, v3_sub(centre, v3_scale14(n, FX(80))), &hit, 0)){
        int32_t ny = v3_dot14(hit.normal, p->up);
        wall = ny < R14(0.17f) && ny > -R14(0.17f);
    }
    if(!wall){
        mario_set_action(MA_WAIT);
        return V3(0, 0, 0);
    }
    v3 into = v3_scale14(n, -G(2.0f));
    if(!m->stick){
        mario_set_anim(ANIM_MARIO_WALLWAIT);
        return into;
    }
    int32_t forward = v3_dot14(m->move_dir, n);
    if(forward < -R14(0.866f)){
        mario_set_anim(ANIM_MARIO_PUSH);
        return into;
    }
    if(forward > 0){
        if(forward > R14(0.707f)) p->front = v3_tangent14(n, p->up, p->front);
        mario_set_action(MA_WAIT);
        return V3(0, 0, 0);
    }
    v3 side = v3_scale14(v3_sub(m->move_dir, v3_scale14(n, forward)), N14_ONE + forward / 4);
    v3 left = v3_cross14(p->up, p->front);
    mario_set_anim(v3_dot14(side, left) < 0 ? ANIM_MARIO_WALLWALKR : ANIM_MARIO_WALLWALKL);
    return v3_sub(v3_scale14(side, G(6.0f)), v3_scale14(n, G(6.0f)));
}

static bool mario_wall_probe(v3 dir, fx dist, v3* normal, fx* hit_dist){
    Player* p = &g_game.player;
    v3 centre = v3_add(p->figure.position, v3_scale14(p->up, C_BODY_HEIGHT));
    RayHit hit;
    if(!collision_ray(centre, v3_add(centre, v3_scale14(dir, dist)), &hit, 0)) return false;
    int32_t ny = v3_dot14(hit.normal, p->up);
    if(ny >= R14(0.17f) || ny <= -R14(0.17f)) return false;
    if(normal) *normal = v3_tangent14(hit.normal, p->up, hit.normal);
    if(hit_dist) *hit_dist = mul14(dist, hit.t);
    return true;
}

static bool mario_wall_stick_ok(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    if(!p->on_wall || p->action == MA_DAMAGE_FALL || p->wall_code == 1 || p->wall_code == 8) return false;
    v3 n = p->wall_normal;
    int32_t fn = v3_dot14(p->front, n);
    bool front = fn < -R14(0.5f);
    bool back = !front && fn > R14(0.707f);
    if(!front && !back) return false;
    if(m->wall_cancel && v3_dot14(m->wall_cancel_normal, n) < C_WALL_CANCEL_COS) m->wall_cancel = false;
    if(m->wall_cancel) return false;
    if(p->has_floor_below && p->height_above_ground < C_WALL_STICK_GR_HEIGHT) return false;
    if(p->action == MA_SPIN && p->action_timer < 25) return false;
    if(back && v3_dot14(horizontal(m->jump_vec, p->up), n) > 0) return false;
    if(vertical_speed(m->jump_vec, p->up) > 0) return false;
    bool into = m->stick && v3_dot14(m->move_dir, n) < -R14(0.5f);
    bool wall_jump_anim = p->anim == ANIM_MARIO_WALLJUMP && !mario_anim_done();
    if(!wall_jump_anim && !into && (back || !m->wall_free)) return false;
    return mario_wall_probe(back ? v3_neg(p->front) : p->front, FX(120), 0, 0);
}

static void mario_wall_start(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    m->wall_timer = 0;
    m->wall_side_timer = 0;
    m->wall_drop = 0;
    m->wall_in_dir = v3_direction14(horizontal(m->air_vec, p->up), V3(0, 0, 0));
    p->front = v3_tangent14(v3_neg(p->wall_normal), p->up, p->front);
    mario_stop_walk();
    m->spin_timer = 0;
    mario_set_action(MA_WALL_SLIDE);
    mario_restart_anim(ANIM_MARIO_WALLKEEP);
}

static void mario_wall_jump(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    v3 n = p->wall_normal, dir = n;
    if(m->wall_timer < 15 && (m->wall_in_dir.x | m->wall_in_dir.y | m->wall_in_dir.z)){
        v3 u = v3_neg(m->wall_in_dir);
        int32_t dot = v3_dot14(n, u);
        if(dot < C_WALL_TRI_JUMP_MARGIN && dot > 0)
            dir = v3_direction14(v3_sub(v3_scale14(n, 2 * dot), u), n);
    }
    v3 v = v3_add(v3_scale14(dir, C_WALL_JUMP_XZ), v3_scale14(p->up, C_WALL_JUMP_Y));
    m->wall_cancel = true;
    m->wall_cancel_normal = n;
    p->front = v3_tangent14(dir, p->up, p->front);
    mario_take_off(JUMP_WALL, v, true);
    m->no_control_timer = C_WAIT_NEUTRAL_TIMER;
    m->spin_used = false;
    m->chainable = false;
    m->wall_free = true;
    mario_set_action(MA_WALL_JUMP);
    mario_restart_anim(ANIM_MARIO_WALLJUMP);
    audio_play_effect(SFX_JUMP_WALL, 2);
}

static v3 mario_wall_update(bool b_pressed){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    v3 up = p->up, n = p->wall_normal;

    if(m->a_age < 6){
        mario_wall_jump();
        return V3(0, 0, 0);
    }
    m->wall_timer++;
    v3 wn;
    fx dist = 0;
    bool wall = mario_wall_probe(v3_neg(n), FX(120), &wn, &dist);
    if(m->wall_timer >= C_WALL_RELEASE_TIME + C_WALL_STICK_TIME || (m->wall_timer >= 3 && !wall)){
        m->wall_cancel = true;
        m->wall_cancel_normal = n;
        p->front = v3_tangent14(v3_neg(n), up, p->front);
        mario_take_off(JUMP_FIRST, V3(0, 0, 0), false);
        m->hold = false;
        m->chainable = false;
        mario_set_action(MA_FALL);
        mario_set_anim(ANIM_MARIO_FALL);
        m->push_after = v3_scale14(n, G(30.0f));
        return V3(0, 0, 0);
    }
    if(b_pressed){
        m->wall_cancel = true;
        m->wall_cancel_normal = n;
        v3 v = v3_add(v3_scale14(n, C_WALL_SPIN_FLIP), v3_scale14(up, C_WALL_SPIN_HOP));
        mario_take_off(13, v, true);
        m->wall_free = false;
        m->spin_used = true;
        m->attack_timer = 15;
        mario_set_action(MA_SPIN);
        mario_restart_anim(ANIM_MARIO_SPIN);
        audio_play_effect(SFX_SPIN, 1);
        return v;
    }
    if(wall) n = p->wall_normal = wn;

    p->front = v3_tangent14(v3_blend14(p->front, v3_neg(n), R14(0.1f)), up, p->front);
    v3 disp = wall ? v3_scale14(v3_neg(n), (dist - C_BODY_RADIUS - 1) << 8) : V3(0, 0, 0);

    bool away = m->stick && v3_dot14(m->move_dir, n) > R14(0.5f);
    int32_t blend = R14(0.9f);
    if(away){
        if(m->wall_timer < 165) m->wall_timer = 165;
        if(!m->wall_side_timer) m->wall_side_timer = 1;
    } else if(m->wall_timer >= C_WALL_STICK_TIME && !(p->anim == ANIM_MARIO_WALLKEEP && !mario_anim_done())){
        mario_set_anim(ANIM_MARIO_WALLSLIDE);
    }
    if(m->wall_side_timer) m->wall_side_timer++;
    if(m->wall_timer < C_WALL_STICK_TIME) blend = N14_ONE;
    m->wall_drop = mul14(m->wall_drop, blend) + mul14(G(C_WALL_DROP_SPEED), N14_ONE - blend);
    disp = v3_sub(disp, v3_scale14(up, m->wall_drop));
    if(!away && m->stick){
        int32_t ratio = m->wall_timer < C_WALL_STICK_TIME
                            ? 0
                            : fx_clamp(N14_ONE - (m->wall_timer - C_WALL_STICK_TIME) * C_WALL_SIDE_MOVE_RATIO,
                                       0, N14_ONE);
        v3 along = v3_cross14(v3_neg(up), n);
        int32_t d = v3_dot14(along, m->move_dir);
        if(d > R14(0.2f) || d < -R14(0.2f)){
            d = d < 0 ? ((d + R14(0.2f)) << 14) / R14(0.8f) : ((d - R14(0.2f)) << 14) / R14(0.8f);
            disp = v3_add(disp, v3_scale14(along, mul14(mul14(G(10.0f), d), ratio)));
        }
    }
    return disp;
}

#define C_PULL_HOLD_TIME 40
#define C_PULL_ANGLE     FXC(1.2566371f)
#define C_PULL_MIN_TIME  120

static void mario_save_safe_point(const RayHit* hit){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    if(p->on_wall || (hit->attr & (COL_DEATH | COL_SLIPPERY)) || m->pulling) return;
    if(v3_dot14(hit->normal, p->up) < R14(0.9659258f)) return;
    if(hit->tri >= COL_DYN_BASE) return;
    const CollisionTriangle* t = col_tri(hit->tri);
    v3 pos = p->figure.position, sum = V3(0, 0, 0);
    for(int i = 0; i < 3; i++) sum = v3_add(sum, col_vertex(t, i, pos));
    v3 d = V3(sum.x / 8, sum.y / 8, sum.z / 8);
    if(v3_length_squared64(d) > (uint64_t)(50 * 50)) d = v3_scale14(v3_direction14(d, p->front), FX(50));
    m->safe_point = v3_add(pos, d);
    m->safe_up = hit->normal;
}

static void mario_pull_back_start(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    if(m->pulling || p->action == MA_DEATH || p->action == MA_STAR_LAUNCH || p->action == MA_STAR_GET) return;
    mario_stop_walk();
    m->squat = false;
    m->pulling = true;
    m->pull_hold = C_PULL_HOLD_TIME;
    m->pull_ticks = 0;
    m->pull_target = v3_add(m->safe_point, v3_scale14(m->safe_up, FX(160)));
    m->jump_vec = V3(0, 0, 0);
    m->jumping = true;
    m->hipdrop_timer = 0;
    m->spin_timer = 0;
    p->on_ground = false;
    mario_set_action(MA_FALL);
    mario_set_anim(ANIM_MARIO_PULLBACK);
}

static void mario_pull_back_axis(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    v3 cur = p->figure.position;
    v3 d = v3_sub(m->pull_target, cur);
    fx dist = v3_length_exact(d);
    v3 dn = v3_direction14(d, p->up);
    v3 axis = v3_direction14(v3_cross14(dn, v3_neg(p->up)), V3(0, 0, 0));
    if(!(axis.x | axis.y | axis.z)) axis = v3_tangent14(p->front, dn, V3(N14_ONE, 0, 0));
    v3 normal = v3_direction14(v3_cross14(axis, dn), v3_neg(p->up));
    fx radius = mul14(dist, R14(0.5257311f));
    fx height = mul14(dist, R14(0.1624598f));
    v3 mid = v3_add(cur, V3(d.x / 2, d.y / 2, d.z / 2));
    m->pull_axis = axis;
    m->pull_centre =
        v3_add(mid, V3(mul14(height, normal.x), mul14(height, normal.y), mul14(height, normal.z)));
    m->pull_arm = v3_neg(normal);
    m->pull_radius = radius;

    v3 a = v3_rotate_axis14(m->pull_arm, axis, -C_PULL_ANGLE),
       b = v3_rotate_axis14(m->pull_arm, axis, C_PULL_ANGLE);
    v3 pa = v3_add(m->pull_centre, V3(mul14(radius, a.x), mul14(radius, a.y), mul14(radius, a.z)));
    v3 pb = v3_add(m->pull_centre, V3(mul14(radius, b.x), mul14(radius, b.y), mul14(radius, b.z)));
    m->pull_angle = v3_length_squared64(v3_sub(pa, cur)) <= v3_length_squared64(v3_sub(pb, cur))
                        ? C_PULL_ANGLE
                        : -C_PULL_ANGLE;
    m->pull_ticks = fx_max(radius / 25, C_PULL_MIN_TIME);
    m->pull_total = m->pull_ticks;
}

static void mario_pull_back_tick(void){
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;
    if(m->pull_hold > 0){
        if(--m->pull_hold == 0) mario_pull_back_axis();
        return;
    }
    if(m->pull_ticks > 0){
        int total = m->pull_total, done = total - m->pull_ticks;
        fx a = (fx)(((int64_t)(2 * done - total) * FXC(1.5707963f)) / total);
        int32_t progress = (N14_ONE + (fx_sin(a) << 6)) / 2;
        fx rot = -m->pull_angle + (fx)(((int64_t)2 * m->pull_angle * progress) >> 14);
        v3 dir = v3_rotate_axis14(m->pull_arm, m->pull_axis, rot);
        v3 pos = v3_add(m->pull_centre, V3(mul14(m->pull_radius, dir.x), mul14(m->pull_radius, dir.y),
                                           mul14(m->pull_radius, dir.z)));
        int32_t remaining = (int32_t)(((int64_t)m->pull_ticks << 14) / total);
        int32_t follow = (N14_ONE - remaining) + remaining / 2;
        v3 cur = p->figure.position, t = m->pull_target;
        pos = V3(t.x + (fx)(((int64_t)(pos.x - t.x) * remaining) >> 14),
                 t.y + (fx)(((int64_t)(pos.y - t.y) * remaining) >> 14),
                 t.z + (fx)(((int64_t)(pos.z - t.z) * remaining) >> 14));
        cur = V3(cur.x + (fx)(((int64_t)(pos.x - cur.x) * follow) >> 14),
                 cur.y + (fx)(((int64_t)(pos.y - cur.y) * follow) >> 14),
                 cur.z + (fx)(((int64_t)(pos.z - cur.z) * follow) >> 14));
        p->figure.position = cur;
        m->position16 = V3(cur.x << 8, cur.y << 8, cur.z << 8);
        m->pull_ticks--;
        return;
    }

    m->pulling = false;
    mario_take_off(JUMP_FIRST, V3(0, 0, 0), false);
    m->hold = false;
    m->chainable = false;
    mario_set_action(MA_FALL);
    mario_set_anim(ANIM_MARIO_FALL);
}

static void mario_tick(const KeyState* keys){
    if(level_player_request(keys)) return;
    MarioPhysics* m = &g_mario;
    Player* p = &g_game.player;

    static const KeyState no_keys;
    if(flight_input_locked()) keys = &no_keys;
    p->action_timer++;
    if(p->invincible_timer > 0) p->invincible_timer--;
    if(m->no_control_timer > 0) m->no_control_timer--;
    if(m->attack_timer > 0) m->attack_timer--;
    if(m->spin_cooldown > 0 && !m->jumping) m->spin_cooldown--;
    if(m->jumping) m->spin_cooldown = 0;
    if(!m->jumping && m->frames_since_landing < 1000) m->frames_since_landing++;
    if(g_mario_star_power > 0) g_mario_star_power--;
    mario_power_tick();
    mario_freeze_tick_passive();
    if(g_mario_held) return;
    if(actors_hold_player()) return;

    if(m->jump_type == 11 && m->jumping && p->anim == ANIM_MARIO_FLICKAIR){
        m->stand_turn = 0;
        m->turn_yaw =
            (m->turn_yaw + (vertical_speed(m->jump_vec, p->up) > 0 ? FXC(.6f) : FXC(.25f))) % FX_TAU;
    } else if(m->stand_turn > 0){
        m->turn_yaw = m->turn_yaw * (m->stand_turn - 1) / m->stand_turn;
        m->stand_turn--;
    } else {
        m->turn_yaw = 0;
    }

    if(p->action == MA_STAR_LAUNCH || p->action == MA_STAR_GET || p->action == MA_DEATH ||
       p->action == MA_TALK)
        return;
    if(m->pulling){
        mario_pull_back_tick();
        return;
    }

    mario_update_gravity();
    mario_read_stick(keys);
    if(mario_fire_tick(keys)) return;
    if(mario_freeze_tick(keys)) return;
    if(mario_hang_tick(keys)) return;
    if(!mario_power_water() && mario_swim_tick(keys)){
        mario_swim_body_tick();
        return;
    }
    if(mario_skate_tick(keys)) return;
    bool a_pressed = keys->pressed & KEY_A, b_pressed = keys->pressed & KEY_B;
    bool r_held = keys->held & KEY_R, r_pressed = keys->pressed & KEY_R;
    if(m->dash) m->dash--;
    if(m->dash_chance){
        m->dash_chance--;
        if(a_pressed && p->on_ground && m->stick && v3_dot14(m->move_dir, p->front) >= R14(.5f)){
            m->dash = r_held ? 180 : 60;
            m->dash_chance = 0;
            m->ratio = (r_held ? 5 : 2) * N14_ONE / 13;
            m->slow_start = 0;
            m->squat = false;
            r_held = r_pressed = a_pressed = false;
            mario_set_action(MA_WAIT);
        }
    }
    v3 displacement;
    if(a_pressed) m->a_age = 0;
    else if(m->a_age < 100) m->a_age++;

    if(a_pressed && m->jumping) m->jump_buffer = C_BUFFER_TIME;
    else if(m->jump_buffer > 0) m->jump_buffer--;

    if(!m->jumping){
        switch(p->action){
        case MA_HIPDROP_LAND:
            m->ratio = 0;
            if(a_pressed && p->action_timer > 5){
                mario_jump();
                break;
            }
            if((m->stick && p->action_timer > 15) || mario_anim_done()) mario_set_action(MA_WAIT);
            break;
        case MA_LAND:
            if(a_pressed){
                mario_jump();
                break;
            }
            if(m->stick || mario_anim_done()) mario_set_action(MA_WAIT);
            break;
        case MA_DAMAGE:
            m->ratio = 0;
            if(p->action_timer > 15 && (m->stick || a_pressed || mario_anim_done()))
                mario_set_action(MA_WAIT);
            break;
        default: break;
        }
        if(!m->jumping && p->action != MA_HIPDROP_LAND && p->action != MA_DAMAGE){
            m->squat = r_held && !m->skidding && !m->dash;
            if(m->squat && p->action != MA_SQUAT && p->action != MA_SQUAT_SLIDE) mario_set_action(MA_SQUAT);
            if(!m->squat && (p->action == MA_SQUAT || p->action == MA_SQUAT_SLIDE)) mario_set_action(MA_WAIT);
            if(p->action == MA_LAND && m->stick) mario_set_action(MA_WAIT);
            if((a_pressed || (m->jump_buffer > 0 && m->frames_since_landing <= 1)) && p->action != MA_LAND){
                m->jump_buffer = 0;

                if(p->action == MA_WALL_PUSH)
                    p->front = v3_tangent14(v3_neg(p->wall_normal), p->up, p->front);
                mario_jump();
            } else if(b_pressed && m->spin_cooldown == 0 && !m->squat &&
                      !mario_fireball_spin(actors_player_carrying())){
                m->spin_cooldown = C_SPIN_COOLDOWN;
                m->ground_spin_timer = anim_length(ANIM_MARIO_SPINGROUND);
                m->attack_timer = 25;
                mario_set_action(MA_SPIN);
                mario_restart_anim(ANIM_MARIO_SPINGROUND);
                audio_play_effect(SFX_SPIN, 1);
            }
        }
        if(!m->jumping){
            if(p->action == MA_SPIN){
                if(--m->ground_spin_timer <= 0) mario_set_action(MA_WAIT);
            }
            if(p->action == MA_WALL_PUSH) displacement = mario_wall_push_move();
            else if(p->action != MA_HIPDROP_LAND && p->action != MA_DAMAGE) mario_ground_move(keys);
            else m->ratio = mul14(m->ratio, R14(0.95f));
            if(p->action == MA_WALL_PUSH){
                m->jump_vec = displacement;
            } else if(m->hold_still){
                displacement = V3(0, 0, 0);
                m->jump_vec = displacement;
            } else if(p->action == MA_DAMAGE){
                displacement = v3_scale14(horizontal(m->jump_vec, p->up), R14(0.95f));
                m->jump_vec = displacement;
            } else {
                int32_t speed = mul14(G(C_WALK_SPEED), m->ratio);
                v3 v = v3_scale14(p->front, speed);

                v3 n = p->ground_normal;
                if((n.x | n.y | n.z) && v3_dot14(n, p->up) > R14(0.5f)){
                    v3 along = v3_reject14(v, n);
                    v = v3_set_length(along, speed, p->front);
                }
                displacement = v;
                m->jump_vec = v;
            }
        } else {
            displacement = m->jump_vec;
        }
    } else {
        if(p->action == MA_HIPDROP_START){
            m->jump_vec = V3(0, 0, 0);
            displacement = V3(0, 0, 0);
            if(mario_anim_done()){
                if(++m->hipdrop_timer > 2){
                    mario_set_action(MA_HIPDROP);
                    mario_set_anim(ANIM_MARIO_HIPDROP);
                    m->hipdrop_timer = 0;
                }
            }
        } else if(p->action == MA_HIPDROP){
            displacement = m->jump_vec;
            if(p->has_floor_below){
                int32_t limit = fx_max(p->height_above_ground, 0) << 8;
                if(v3_length_squared64(displacement) >= (uint64_t)((int64_t)limit * limit))
                    displacement = v3_set_length(displacement, limit, v3_neg(p->up));
            }
            int32_t speed = v3_length_exact(m->jump_vec);
            int32_t rate = fx_clamp(
                R14(1.1f) -
                    (int32_t)(((uint32_t)fx_min(speed, C_HIPDROP_LIMIT) << 8) / (C_HIPDROP_LIMIT >> 6)),
                0, N14_ONE);
            m->jump_vec = v3_sub(m->jump_vec, v3_scale14(p->up, mul14(G(C_HIPDROP_GRAVITY), rate)));
            v3 h = horizontal(m->jump_vec, p->up);
            if(v3_length_squared64(h) > (uint64_t)((int64_t)G(10.0f) * G(10.0f)))
                m->jump_vec = v3_add(V3(h.x / 2, h.y / 2, h.z / 2),
                                     v3_scale14(p->up, vertical_speed(m->jump_vec, p->up)));
            if(v3_length_squared64(m->jump_vec) > (uint64_t)((int64_t)C_HIPDROP_LIMIT * C_HIPDROP_LIMIT))
                m->jump_vec = v3_set_length(m->jump_vec, C_HIPDROP_LIMIT, v3_neg(p->up));

            if(p->figure.position.x == m->hipdrop_last.x && p->figure.position.y == m->hipdrop_last.y &&
               p->figure.position.z == m->hipdrop_last.z){
                if(++m->hipdrop_timer >= 15){
                    m->jumping = false;
                    m->jump_vec = V3(0, 0, 0);
                    displacement = V3(0, 0, 0);
                    p->on_ground = true;
                    mario_set_action(MA_WAIT);
                }
            } else {
                m->hipdrop_timer = 0;
            }
            m->hipdrop_last = p->figure.position;
        } else if(p->action == MA_WALL_SLIDE){
            displacement = mario_wall_update(b_pressed);
            if(p->action == MA_WALL_SLIDE) m->jump_vec = v3_scale14(p->up, -m->wall_drop);
        } else {
            if(b_pressed && p->action != MA_DAMAGE_FALL) mario_air_spin();
            if(r_pressed && p->action != MA_LONG_JUMP && p->action != MA_DAMAGE_FALL &&
               p->height_above_ground >= C_HIPDROP_MIN_HEIGHT){
                mario_start_hipdrop();
                displacement = V3(0, 0, 0);
            } else {
                if(m->spin_timer > 0 && --m->spin_timer == 0){
                    m->jump_type = JUMP_FIRST;
                    m->spin_used = true;
                }
                if(p->action == MA_SPIN && mario_anim_done()){
                    mario_set_action(MA_FALL);
                    mario_set_anim(ANIM_MARIO_FALL);
                }
                displacement = mario_air_physics(keys);

                int32_t vs = vertical_speed(m->jump_vec, p->up);
                if(mario_wall_stick_ok() && !bee_active()) mario_wall_start();

                if((p->action == MA_JUMP || p->action == MA_JUMP2 || p->action == MA_WALL_JUMP) && vs < 0 &&
                   mario_anim_done())
                    mario_set_anim(ANIM_MARIO_FALL);
            }
        }
    }

    bool want_ground = !m->jumping;
    displacement = v3_add(displacement, m->push_after);
    m->push_after = V3(0, 0, 0);
    mario_move_and_collide(displacement, want_ground);
    mario_hang_check();

    if(p->action == MA_WAIT && !m->jumping && p->on_ground && p->on_wall && m->walk_index && !m->squat &&
       m->wall_ny < R14(0.075f) && m->wall_ny > -R14(0.082f) &&
       v3_dot14(p->front, v3_neg(p->wall_normal)) >= C_WALL_PUSH_COS){
        mario_stop_walk();
        m->step_ticks = 0;
        mario_set_action(MA_WALL_PUSH);
        mario_set_anim(ANIM_MARIO_PUSH);
    }

    bool still = p->action == MA_WAIT && p->on_ground && !m->jumping && !m->stick &&
                 !(keys->held & (KEY_A | KEY_R)) && !b_pressed && m->ratio < R14(0.001f) &&
                 p->water_state == WATER_DRY;
    if(!still){
        m->idle_stage = 0;
        m->idle_ticks = 0;
    } else if(++m->idle_ticks >= C_SLEEP_TIME && m->idle_stage < 2){
        m->idle_stage++;
        m->idle_ticks = 0;
    }

    if((m->void_ticks > 150 && !g_game.stage.header->galaxy) || p->health <= 0 ||
       (g_game.stage.header->death_height && p->figure.position.z < g_game.stage.header->death_height)){
        if(p->action != MA_DEATH){
            mario_set_action(MA_DEATH);
            mario_restart_anim(p->health <= 0 ? ANIM_MARIO_DIEOVER : ANIM_MARIO_DIEFALL);
            audio_play_effect(SFX_DIE, 5);
        }
    }
}

static void mario_reset(v3 position, v3 up, v3 front){
    Player* p = &g_game.player;
    MarioPhysics* m = &g_mario;
    *m = (MarioPhysics){0};

    g_mario_cling = 0;
    g_mario_held = false;
    g_mario_star_power = 0;
    flight_reset();
    bee_reset();
    mario_swim_reset();
    mario_hang_reset();
    mario_fire_reset();
    mario_freeze_reset();
    mario_skate_reset();
    mario_power_end();
    int health = p->health;
    *p = (Player){0};
    p->health = health > 0 ? health : 3;
    figure_init(&p->figure, figure_mario);
    p->figure.use_basis = true;

    p->figure.scale = V3(64, 64, 64);
    p->figure.position = position;
    p->up = up;
    p->gravity_up = up;
    p->front = v3_tangent14(front, up, V3(0, N14_ONE, 0));
    m->position16 = V3(position.x << 8, position.y << 8, position.z << 8);
    m->last_safe_position = position;
    m->last_safe_up = up;
    m->safe_point = position;
    m->safe_up = up;
    m->a_age = 100;
    p->on_ground = false;
    m->jumping = true;
    m->jump_type = JUMP_FIRST;
    p->action = MA_FALL;
    p->anim = ANIM_MARIO_WAIT;
    p->anim2 = -1;
}
