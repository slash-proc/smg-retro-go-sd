enum {
    JUMPGUARDER_HIDE = 160,
    JUMPGUARDER_UP,
    JUMPGUARDER_WAIT,
    JUMPGUARDER_DOWN,
    JUMPGUARDER_HOPSTART,
    JUMPGUARDER_HOPWAIT,
    JUMPGUARDER_HOPJUMP,
    JUMPGUARDER_HOPEND,
    JUMPGUARDER_PREOPEN,
    JUMPGUARDER_OPEN,
    JUMPGUARDER_CLOSE,
    JUMPGUARDER_INTER
};
static EWRAM_BSS struct {
    int16_t owner, step;
    uint8_t phase, requested;
    uint32_t distance;
} g_baby_attack_permit;
static void baby_attack_permit_reset(void){
    g_baby_attack_permit.owner = -1;
    g_baby_attack_permit.step = 0;
    g_baby_attack_permit.phase = 0;
    g_baby_attack_permit.requested = 0;
    g_baby_attack_permit.distance = 99999;
}
static Actor* actor_at_slot(int slot, int type){
    return slot >= 0 && slot < g_game.actor_count && g_game.actors[slot].type == type ? &g_game.actors[slot]
                                                                                      : NULL;
}
static Actor* jumpguarder_head(const Actor* a){ return actor_at_slot(a->target.x, OBJ_BUOYJUMPHEAD); }
static Actor* jumpguarder_baby(const Actor* a, int n){
    return actor_at_slot(n == 0   ? a->velocity.x
                         : n == 1 ? a->velocity.y
                         : n == 2 ? a->velocity.z
                                  : a->value,
                         OBJ_BUOYBEGOBABY);
}

static Actor* begoman_baby_parent(const Actor* b){
    Actor* p = actor_at_slot(b->args[3], OBJ_BUOYJUMPGUARDER);
    if(p) return b->args[2] >= 0 && b->args[2] < 4 && jumpguarder_baby(p, b->args[2]) == b ? p : NULL;
    p = actor_at_slot(b->args[3], OBJ_BUOYBABYLAUNCHER);
    return p && b->args[2] == 0 && p->value == actor_index(b) ? p : NULL;
}

static const BuoyGuarderMatrix* jumpguarder_joint(const Actor* a, bool head){
    int anim = a->anim, frame = a->anim_frame >> 8;
    int count = head ? ANIM_JUMPGUARDERHEAD_COUNT : ANIM_JUMPGUARDERBODY_COUNT;
    if(anim < 0 || anim >= count) anim = 0;
    int length = head ? buoy_guarder_head_lengths[anim] : buoy_guarder_body_lengths[anim];
    const AnimClip* clips = (const AnimClip*)((const uint8_t*)a->figure.model + a->figure.model->clips);
    frame = clips[anim].loop ? frame % length : fx_min(frame, length);
    if(frame < 0) frame = 0;
    return &buoy_guarder_joint_pool[(head ? buoy_guarder_head_clips[anim]
                                          : buoy_guarder_body_clips[anim])[frame]];
}

static v3 jumpguarder_joint_apply(const BuoyGuarderMatrix* j, v3 p, bool position){
    v3 out;
    fx* v = &out.x;
    for(int r = 0; r < 3; r++)
        v[r] = (fx)((int64_t)j->m[r * 4] * p.x + (int64_t)j->m[r * 4 + 1] * p.y +
                    (int64_t)j->m[r * 4 + 2] * p.z + (position ? j->m[r * 4 + 3] : 0));
    return out;
}
static v3 jumpguarder_model_to_world(const Actor* a, v3 source16, bool position){
    const StageObject* o = actor_stage_object(a);
    v3 side = v3_neg(v3_cross14(a->up, a->front));
    v3 scaled = V3((fx)((int64_t)source16.x * (o ? o->scale[0] : 256) / 256),
                   (fx)((int64_t)source16.y * (o ? o->scale[1] : 256) / 256),
                   (fx)((int64_t)source16.z * (o ? o->scale[2] : 256) / 256));

    v3 out =
        V3((fx)(((int64_t)side.x * scaled.x + (int64_t)a->up.x * scaled.y + (int64_t)a->front.x * scaled.z) >>
                14),
           (fx)(((int64_t)side.y * scaled.x + (int64_t)a->up.y * scaled.y + (int64_t)a->front.y * scaled.z) >>
                14),
           (fx)(((int64_t)side.z * scaled.x + (int64_t)a->up.z * scaled.y + (int64_t)a->front.z * scaled.z) >>
                14));
    if(position) return v3_add(a->position, V3(out.x >> 16, out.y >> 16, out.z >> 16));
    return out;
}
static v3 jumpguarder_body_sensor(const Actor* a){
    return jumpguarder_model_to_world(
        a, jumpguarder_joint_apply(jumpguarder_joint(a, false), V3(0, 35, 0), true), true);
}
static v3 jumpguarder_head_to_model(const Actor* a, v3 point, bool direction){
    Actor* h = jumpguarder_head(a);
    fx angle = h ? h->target.x : 0;
    int s = fx_sin(angle), c = fx_cos(angle);

    v3 v = V3((fx)(((int64_t)c * point.x + (int64_t)s * point.z) >> 8), point.y,
              (fx)((-(int64_t)s * point.x + (int64_t)c * point.z) >> 8));
    if(!direction) v.y += 44 * 65536;
    const BuoyGuarderMatrix* body = jumpguarder_joint(a, false);
    v3 out;
    fx* q = &out.x;
    for(int r = 0; r < 3; r++)
        q[r] = (fx)(((int64_t)body->m[r * 4] * v.x + (int64_t)body->m[r * 4 + 1] * v.y +
                     (int64_t)body->m[r * 4 + 2] * v.z) >>
                    16) +
               (direction ? 0 : body->m[r * 4 + 3]);
    return out;
}
static v3 jumpguarder_jump_sensor(const Actor* a){
    Actor* h = jumpguarder_head(a);
    if(!h) return a->position;
    return jumpguarder_model_to_world(
        a,
        jumpguarder_head_to_model(
            a, jumpguarder_joint_apply(jumpguarder_joint(h, true), V3(0, -100, 0), true), false),
        true);
}
static void jumpguarder_pose_head(Actor* a){
    Actor* h = jumpguarder_head(a);
    if(!h) return;
    h->position = jumpguarder_model_to_world(a, jumpguarder_head_to_model(a, V3(0, 0, 0), false), true);
    v3 up = jumpguarder_model_to_world(a, jumpguarder_head_to_model(a, V3(0, 65536, 0), true), false),
       front = jumpguarder_model_to_world(a, jumpguarder_head_to_model(a, V3(0, 0, 65536), true), false);
    v3 right = jumpguarder_model_to_world(a, jumpguarder_head_to_model(a, V3(65536, 0, 0), true), false);
    h->up = v3_direction14(up, a->up);
    h->front = v3_tangent14(front, h->up, a->front);
    h->figure.scale = V3((fx)((uint64_t)isqrt64(v3_length_squared64(right)) * 64 >> 16),
                         (fx)((uint64_t)isqrt64(v3_length_squared64(up)) * 64 >> 16),
                         (fx)((uint64_t)isqrt64(v3_length_squared64(front)) * 64 >> 16));
}
static void jumpguarder_turn_head(Actor* a){
    Actor* h = jumpguarder_head(a);
    if(!h) return;
    v3 to = v3_sub(game_player()->figure.position, a->position), side = v3_neg(v3_cross14(a->up, a->front));
    fx x = v3_dot14(to, side), z = v3_dot14(to, a->front);
    if(!x && !z) return;
    fx aim = fx_atan2_small(x, z), delta = aim - h->target.x;
    while(delta > FX_PI) delta -= FX_TAU;
    while(delta < -FX_PI) delta += FX_TAU;
    h->target.x += fx_clamp(delta, -DEG(3), DEG(3));
    while(h->target.x > FX_PI) h->target.x -= FX_TAU;
    while(h->target.x < -FX_PI) h->target.x += FX_TAU;
}
static bool jumpguarder_can_appear(const Actor* a, int range){
    const StageObject* o = actor_stage_object(a);
    if(!o) return false;
    for(int k = SWITCH_A; k <= SWITCH_B; k++){
        int sw = stage_object_switch(o, k);
        if(sw >= 0 && !switch_on(sw)) return false;
    }
    v3 d = v3_sub(game_player()->figure.position, a->position);
    fx height = v3_dot14(d, a->up);
    return fx_abs(height) < 500 && v3_length_squared64(v3_reject14(d, a->up)) < (uint64_t)range * range;
}
static void jumpguarder_set(Actor* a, int state, int body, int head){
    actor_set_state(a, state, body);
    Actor* h = jumpguarder_head(a);
    if(h && head >= 0){
        h->anim = head;
        h->anim_frame = 0;
    }
}
static int mario_sphere_contact(v3 centre, v3 up, fx radius, bool spin){
    Player* p = game_player();
    MarioPhysics* m = &g_mario;
    if(p->action == MA_DEATH || p->action == MA_STAR_LAUNCH || g_mario_held) return CONTACT_NONE;
    v3 d = v3_sub(mario_centre(), centre);
    fx r = radius + C_BODY_RADIUS + (spin && m->attack_timer > 0 ? 60 : 0);
    if(fx_abs(d.x) > r || fx_abs(d.y) > r || fx_abs(d.z) > r || v3_length_squared64(d) > (uint64_t)r * r)
        return CONTACT_NONE;
    fx above = v3_dot14(d, up);
    if(p->action == MA_HIPDROP && above > 0) return CONTACT_HIPDROP;
    if(spin && m->attack_timer > 0) return CONTACT_SPIN;
    if(m->jumping && vertical_speed(m->jump_vec, p->up) <= 0 && above > 0) return CONTACT_TRAMPLE;
    return CONTACT_TOUCH;
}
static void jumpguarder_launch_mario(Actor* a){
    Player* p = game_player();
    bool held = g_game.keys.held & KEY_A;

    v3 h = horizontal(v3_scale14(a->up, V16(1)), p->up);
    mario_take_off(11, v3_add(h, v3_scale14(p->up, V16(40) + (held ? V16(4) : 0))), true);
    g_mario.spin_used = false;
    g_mario.turn_yaw = 0;
    g_mario.stand_turn = 0;
    mario_set_action(MA_JUMP);
    mario_restart_anim(ANIM_MARIO_FLICKAIR);

    const StageObject* o = actor_stage_object(a);
    if(o && o->camera >= 0){
        camera_event_start(o->camera, 2, a->position, a->up, a->front);
        a->target.z = 1;
    }
    jumpguarder_set(a, JUMPGUARDER_HOPJUMP, -1, ANIM_JUMPGUARDERHEAD_HOPJUMP);
}
static void jumpguarder_ensure_children(Actor* a){
    if(!jumpguarder_head(a)){
        Actor* h = actor_spawn_reuse(OBJ_BUOYJUMPHEAD, a->position, a->up, a->front);
        if(h){
            a->target.x = actor_index(h);
            h->args[3] = actor_index(a);
            h->anim = ANIM_JUMPGUARDERHEAD_WAIT;
            h->anim_frame = 0;
            h->target = V3(0, 0, 0);
            h->flags = ACTOR_WAITING;
        }
    }
    for(int n = 0; n < 4; n++)
        if(!jumpguarder_baby(a, n)){
            Actor* b = actor_spawn_reuse(OBJ_BUOYBEGOBABY, a->position, a->up, a->front);
            if(!b) break;
            if(n == 0) a->velocity.x = actor_index(b);
            else if(n == 1) a->velocity.y = actor_index(b);
            else if(n == 2) a->velocity.z = actor_index(b);
            else a->value = actor_index(b);
            b->args[3] = actor_index(a);
            b->args[2] = n;
            b->active = 0;
            b->flags = ACTOR_WAITING;
            b->target = b->front;
            b->home = a->position;
        }
}
static void jumpguarder_init(Actor* a, const StageObject* o){
    a->figure.scale = V3(64 * o->scale[0] / 256, 64 * o->scale[1] / 256, 64 * o->scale[2] / 256);
    a->velocity = V3(-1, -1, -1);
    a->value = -1;
    a->target = V3(-1, 0, 0);
    a->health = actor_arg_or(a, 0, 4);
    actor_set_state(a, JUMPGUARDER_HIDE, ANIM_JUMPGUARDERBODY_DOWN);
    a->anim_frame = FX_ONE * (buoy_guarder_body_lengths[ANIM_JUMPGUARDERBODY_DOWN] - 1);
    jumpguarder_ensure_children(a);
    jumpguarder_pose_head(a);
}
static bool begoman_baby_request_attack(Actor* b){
    g_baby_attack_permit.requested = 1;
    uint32_t distance = isqrt64(v3_length_squared64(v3_sub(b->position, game_player()->figure.position)));
    if(g_baby_attack_permit.phase <= 1){
        if(g_baby_attack_permit.owner < 0 || distance < g_baby_attack_permit.distance){
            g_baby_attack_permit.owner = actor_index(b);
            g_baby_attack_permit.distance = distance;
        }
        if(!g_baby_attack_permit.phase){
            g_baby_attack_permit.phase = 1;
            g_baby_attack_permit.step = 0;
        }
    }
    if(g_baby_attack_permit.phase == 2 && g_baby_attack_permit.owner == actor_index(b)){
        g_baby_attack_permit.step = 0;
        return true;
    }
    return false;
}
static void jumpguarder_pre_update(void){
    if(g_baby_attack_permit.phase == 1){
        if(!g_baby_attack_permit.requested) baby_attack_permit_reset();
        else if(++g_baby_attack_permit.step > 2){
            g_baby_attack_permit.phase = 2;
            g_baby_attack_permit.step = 0;
        }
    } else if(g_baby_attack_permit.phase == 2 && ++g_baby_attack_permit.step > 30) baby_attack_permit_reset();
    g_baby_attack_permit.requested = 0;
    for(int i = 0; i < g_game.actor_count; i++){
        Actor* child = &g_game.actors[i];
        if(child->type != OBJ_BUOYJUMPHEAD && child->type != OBJ_BUOYBEGOBABY) continue;
        Actor* parent = child->type == OBJ_BUOYBEGOBABY ? begoman_baby_parent(child)
                                                        : actor_at_slot(child->args[3], OBJ_BUOYJUMPGUARDER);
        if(!parent || !parent->active || parent->stage_index < 0){
            child->active = 0;
            child->flags = 0;
            child->args[3] = -1;
        }
    }
}
static void jumpguarder_release_babies(Actor* a, bool first){
    Actor* h = jumpguarder_head(a);
    if(!h) return;
    int mask = a->target.y & 15, count = 0;
    for(int n = 0; n < 4; n++)
        if(mask & (1 << n)) count++;
    if(!count) return;
    v3 radial = h->front;
    for(int n = 0; n < 4; n++)
        if(mask & (1 << n)){
            Actor* b = jumpguarder_baby(a, n);
            if(!b) continue;
            radial = v3_rotate_axis14(radial, h->up, FX_TAU / count);
            if(first){
                b->active = 1;
                b->flags = ACTOR_WAITING;
                actor_set_state(b, BEGOMAN_BABY_LAUNCH, ANIM_BEGOMANBABY_WAIT);
                b->health = 0;
                b->target = b->front;
                b->figure.scale = V3(64, 64, 64);
                if(a->health > 0) --a->health;
            }
            if(a->timer - 1 < 70){
                b->position = v3_add(a->position, v3_scale14(radial, 64));
                b->velocity = V3(0, 0, 0);
            } else if(a->timer - 1 == 70){
                int mod = actor_arg_or(a, 1, 0);
                b->velocity = v3_scale14(radial, V16(16) / (mod == 1 ? 2 : mod == 2 ? 4 : 1));
            }
        }
}

static void begoman_baby_retire(Actor* b){
    b->active = 0;
    b->flags = ACTOR_WAITING;
}
static void begoman_baby_kill(Actor* b, bool items){
    bool launcher = actor_at_slot(b->args[3], OBJ_BUOYBABYLAUNCHER) != NULL;
    if(items){
        collectibles_pop(OBJ_STARBIT, v3_add(b->position, v3_scale14(b->up, 80)), b->up, b->value ? 3 : 1);
        if(launcher) audio_play_effect(SFX_METBO_BURST, 1);
    }

    if(b->active) audio_play_effect(SFX_METBO_EXPLODE, 3);
    begoman_baby_retire(b);
}

static GBA_ARM_TARGET_ATTR __attribute__((noinline)) bool begoman_baby_sphere_triangle(int tri, v3 centre,
                                                                                  SphereContact* out){
    const fx radius = 45;
    const CollisionTriangle* t = col_tri(tri);
    v3 n = v3_from_i16(t->normal);
    v3 a = col_vertex(t, 0, centre);
    fx dist = -v3_dot14(a, n);

    if(dist > radius || dist < 0) return false;
    v3 b = col_vertex(t, 1, centre), c = col_vertex(t, 2, centre);
    v3 q = closest_point_to_origin(a, b, c, v3_scale14(n, -dist));
    uint64_t d2 = v3_length_squared64(q);
    if(d2 > (uint64_t)((int64_t)radius * radius)) return false;
    fx d = (fx)isqrt64(d2);
    out->normal = d > 2 && dist > 0 ? v3_direction14(v3_neg(q), n) : n;
    out->depth = dist >= 0 ? radius - d : radius + d;
    out->point = v3_add(centre, q);
    out->attr = t->attr;
    out->tri = (int16_t)tri;
    return true;
}

static void begoman_baby_move(Actor* b, int accel, int gravity, int damp, int turn, bool rebound){
    if(turn > 0){
        v3 aim = v3_tangent14(v3_sub(mario_centre(), b->position), b->up, b->front);
        actor_turn_toward(b, aim, DEG(turn));
    }
    b->velocity = v3_scale14(
        v3_add(v3_add(b->velocity, v3_scale14(b->front, accel)), v3_scale14(b->up, -gravity)), damp);
    begoman_baby_bind(b, rebound);
}
static void begoman_baby_bind(Actor* b, bool rebound){
    v3 old = b->position;
    b->position = v3_add(old, V3(b->velocity.x >> 8, b->velocity.y >> 8, b->velocity.z >> 8));
    b->flags &= ~ACTOR_ON_GROUND;
    CollisionCandidates cand;
    collision_gather_around(v3_add(old, v3_scale14(b->up, 45)), v3_add(b->position, v3_scale14(b->up, 45)),
                            46, &cand);
    for(int pass = 0; pass < 2; pass++)
        for(int j = 0; j < cand.count; j++){
            SphereContact contact;
            if(!begoman_baby_sphere_triangle(cand.index[j], v3_add(b->position, v3_scale14(b->up, 45)),
                                             &contact))
                continue;
            if(contact.attr & COL_DEATH){
                begoman_baby_kill(b, false);
                return;
            }
            if(contact.attr & COL_BURN){
                begoman_baby_kill(b, true);
                return;
            }
            b->position = v3_add(b->position, v3_scale14(contact.normal, contact.depth));
            int ny = v3_dot14(contact.normal, b->up), speed = v3_dot14(b->velocity, contact.normal);
            if(ny > R14(0.5)) b->flags |= ACTOR_ON_GROUND;
            if(speed < 0){
                int coefficient = rebound || ny < R14(0.5) ? N14_ONE : R14(1.5);
                b->velocity = v3_sub(b->velocity, v3_scale14(contact.normal, mul14(speed, coefficient)));
            }
            if(ny < R14(0.5) && ny > -R14(0.5)){
                b->velocity = v3_add(b->velocity, v3_scale14(contact.normal, V16(5)));
                b->front = v3_tangent14(b->velocity, b->up, b->front);
            }
        }
}
static void begoman_baby_think(Actor* b){
    Actor* parent = begoman_baby_parent(b);
    if(!parent || !parent->active){
        b->active = 0;
        b->flags = 0;
        b->args[3] = -1;
        return;
    }
    v3 gravity_up = gravity_sample(g_game.stage.header, b->position).up;
    b->up = v3_direction14(v3_add(v3_scale14(b->up, R14(.9)), v3_scale14(gravity_up, R14(.1))), b->up);
    b->front = v3_tangent14(b->front, b->up, b->front);
    int step = b->timer - 1;
    WaterSample water;
    if(b->state != BEGOMAN_BABY_WATER && b->state != BEGOMAN_BABY_NOCALC &&
       water_query(v3_add(b->position, v3_scale14(b->up, 45)), b->up, &water) && water.depth > 0){
        actor_set_state(b, BEGOMAN_BABY_WATER, ANIM_BEGOMANBABY_FLY);
        b->velocity = v3_set_length(v3_sub(b->velocity, v3_scale14(b->up, V16(1))), V16(10), v3_neg(b->up));
        step = 0;
    }
    int contact = step && b->state != BEGOMAN_BABY_LAUNCH && b->state != BEGOMAN_BABY_BLOW
                      ? actor_mario_contact(b, 0)
                      : CONTACT_NONE;
    if(contact == CONTACT_SPIN || contact == CONTACT_STAR){
        v3 away = v3_tangent14(v3_sub(b->position, mario_centre()), b->up, b->front);
        b->velocity = v3_add(v3_scale14(away, V16(35)), v3_scale14(b->up, V16(15)));
        b->value = parent->type == OBJ_BUOYBABYLAUNCHER && contact == CONTACT_STAR ? 1 : 0;
        actor_set_state(b, BEGOMAN_BABY_BLOW, ANIM_BEGOMANBABY_STOP);
        step = 0;
    } else if(contact == CONTACT_TRAMPLE){
        mario_stomp_bounce(g_game.keys.held & KEY_A);
        actor_set_state(b, BEGOMAN_BABY_TRAMPLE, ANIM_BEGOMANBABY_TRAMPLE);
        step = 0;
    } else if(contact == CONTACT_HIPDROP){
        mario_stomp_bounce(g_game.keys.held & KEY_A);
    } else if(contact == CONTACT_TOUCH && game_player()->on_ground && b->state != BEGOMAN_BABY_TRAMPLE &&
              b->state != BEGOMAN_BABY_BLOW){
        mario_damage(b->position, 1);
        actor_set_state(b, BEGOMAN_BABY_HIT, ANIM_BEGOMANBABY_HITREACTION);
        step = 0;
    }
    if((b->state == BEGOMAN_BABY_PURSUE || b->state == BEGOMAN_BABY_TURN) && ++b->health >= 180){
        actor_set_state(b, BEGOMAN_BABY_TIRED, ANIM_BEGOMANBABY_TIRED);
        step = 0;
    } else if(b->state != BEGOMAN_BABY_PURSUE && b->state != BEGOMAN_BABY_TURN) b->health = 0;
    fx dist = isqrt64(v3_length_squared64(v3_sub(mario_centre(), b->position)));
    switch(b->state){
    case BEGOMAN_BABY_AFTER_LAUNCH:
        if(step < 80)
            b->velocity = v3_add(v3_scale14(b->front, V16(5)),
                                 v3_scale14(b->up, ((V16(5) * fx_cos(step * FX_TAU / 10)) >> 8)));
        else if(step < 85) b->velocity = V3(0, 0, 0);
        else {
            if(!(b->flags & ACTOR_ON_GROUND)) b->velocity = v3_sub(b->velocity, v3_scale14(b->up, V16(3)));
            fx rise = v3_dot14(b->velocity, b->up);
            v3 along = v3_scale14(b->up, rise);
            if(rise > 0) along = v3_scale14(along, R14(.98));
            b->velocity = v3_add(
                v3_scale14(v3_reject14(b->velocity, b->up), b->flags & ACTOR_ON_GROUND ? R14(.8) : R14(.98)),
                along);
        }
        begoman_baby_bind(b, false);
        if(b->active && (b->flags & ACTOR_ON_GROUND))
            actor_set_state(b, BEGOMAN_BABY_AFTER_GROUND, ANIM_BEGOMANBABY_BOUND);
        break;
    case BEGOMAN_BABY_AFTER_GROUND: {
        if(!(b->flags & ACTOR_ON_GROUND)) b->velocity = v3_sub(b->velocity, v3_scale14(b->up, V16(3)));
        fx rise = v3_dot14(b->velocity, b->up);
        v3 along = v3_scale14(b->up, rise);
        if(rise > 0) along = v3_scale14(along, R14(.98));
        b->velocity = v3_add(
            v3_scale14(v3_reject14(b->velocity, b->up), b->flags & ACTOR_ON_GROUND ? R14(.8) : R14(.98)),
            along);
        begoman_baby_bind(b, false);
        if(b->active && (b->anim_frame >> 8) >= anim_length_of(b->figure.model, b->anim))
            actor_set_state(b, BEGOMAN_BABY_WAIT, ANIM_BEGOMANBABY_WAIT);
        break;
    }
    case BEGOMAN_BABY_LAUNCH:
        begoman_baby_move(b, 0, V16(1), R14(.98), 0, true);
        if(b->flags & ACTOR_ON_GROUND) actor_set_state(b, BEGOMAN_BABY_WAIT, ANIM_BEGOMANBABY_WAIT);
        break;
    case BEGOMAN_BABY_WAIT:
        begoman_baby_move(b, 0, V16(3), R14(.95), 1, false);
        if(dist < 1000 && actor_floor_ahead(b, 150)){
            bool attack = begoman_baby_request_attack(b);
            actor_set_state(b, attack ? BEGOMAN_BABY_SIGN : BEGOMAN_BABY_KEEP,
                            attack ? ANIM_BEGOMANBABY_SIGNATTACK : ANIM_BEGOMANBABY_BRAKE);
        } else if(dist >= 1000 && (b->flags & ACTOR_ON_GROUND))
            actor_set_state(b, BEGOMAN_BABY_NOCALC, ANIM_BEGOMANBABY_WAIT);
        break;
    case BEGOMAN_BABY_NOCALC:
        if(dist < 1000) actor_set_state(b, BEGOMAN_BABY_WAIT, ANIM_BEGOMANBABY_WAIT);
        break;
    case BEGOMAN_BABY_SIGN:
        begoman_baby_move(b, 0, V16(3), R14(.8), 3, false);
        if(!begoman_baby_request_attack(b)) actor_set_state(b, BEGOMAN_BABY_WAIT, ANIM_BEGOMANBABY_WAIT);
        else if(step >= 39) actor_set_state(b, BEGOMAN_BABY_PURSUE, -1);
        break;
    case BEGOMAN_BABY_PURSUE:
        if(step == 10){
            b->anim = ANIM_BEGOMANBABY_ATTACK;
            b->anim_frame = 0;
        }
        if(step == 18) b->velocity = v3_scale14(b->front, V16(15));
        if(!actor_floor_ahead(b, 150)){
            actor_set_state(b, BEGOMAN_BABY_BRAKE, ANIM_BEGOMANBABY_BRAKE);
            break;
        }
        begoman_baby_move(b, V16(.4), V16(3), R14(.97), 3, step <= 18);
        if(!begoman_baby_request_attack(b) || dist > 2000)
            actor_set_state(b, BEGOMAN_BABY_WAIT, ANIM_BEGOMANBABY_WAIT);
        else if(
            step > 180 ||
            v3_dot14(b->front, v3_tangent14(v3_sub(mario_centre(), b->position), b->up, b->front)) <
                R14(.866025))
            actor_set_state(b, BEGOMAN_BABY_TURN, ANIM_BEGOMANBABY_TURN);
        break;
    case BEGOMAN_BABY_TURN:
        begoman_baby_move(b, V16(.4), V16(3), R14(.95), 3 + fx_min(step, 60) * 7 / 60, true);
        if(!actor_floor_ahead(b, 150)) actor_set_state(b, BEGOMAN_BABY_BRAKE, ANIM_BEGOMANBABY_BRAKE);
        else if(step > 30 && !begoman_baby_request_attack(b))
            actor_set_state(b, BEGOMAN_BABY_WAIT, ANIM_BEGOMANBABY_WAIT);
        else if(step > 60) actor_set_state(b, BEGOMAN_BABY_PURSUE, -1);
        break;
    case BEGOMAN_BABY_BRAKE:
        if(!step) b->velocity = v3_reject14(b->velocity, b->front);
        begoman_baby_move(b, 0, V16(1), b->flags & ACTOR_ON_GROUND ? R14(.7) : R14(.95), 3, true);
        if(step > 50 && actor_floor_ahead(b, 150))
            actor_set_state(b, BEGOMAN_BABY_TURN, ANIM_BEGOMANBABY_TURN);
        break;
    case BEGOMAN_BABY_KEEP: {
        int accel = dist > 600 ? V16(.35) : dist < 400 ? V16(-.22) : 0;
        begoman_baby_move(b, accel, V16(1), R14(.95), 3, false);
        if(dist > 1200) actor_set_state(b, BEGOMAN_BABY_WAIT, ANIM_BEGOMANBABY_WAIT);
        else if(!actor_floor_ahead(b, 150)) actor_set_state(b, BEGOMAN_BABY_BRAKE, ANIM_BEGOMANBABY_BRAKE);
        else if(begoman_baby_request_attack(b))
            actor_set_state(b, BEGOMAN_BABY_SIGN, ANIM_BEGOMANBABY_SIGNATTACK);
        break;
    }
    case BEGOMAN_BABY_TRAMPLE:
        begoman_baby_move(b, 0, V16(3), R14(.95), 0, false);
        if(step > 180) actor_set_state(b, BEGOMAN_BABY_SIGN, ANIM_BEGOMANBABY_SIGNATTACK);
        break;
    case BEGOMAN_BABY_HIT:
        begoman_baby_move(b, 0, V16(3), R14(.95), 0, false);
        if(step >= 30) actor_set_state(b, BEGOMAN_BABY_PROVOKE, ANIM_BEGOMANBABY_PROVOKE);
        break;
    case BEGOMAN_BABY_PROVOKE:
        begoman_baby_move(b, 0, V16(3), R14(.95), 1, false);
        if(step > 120) actor_set_state(b, BEGOMAN_BABY_SIGN, ANIM_BEGOMANBABY_SIGNATTACK);
        break;
    case BEGOMAN_BABY_TIRED:
        begoman_baby_move(b, V16(.2), V16(1), R14(.95), 3, false);
        if(step > 120) actor_set_state(b, BEGOMAN_BABY_WAIT, ANIM_BEGOMANBABY_WAIT);
        break;
    case BEGOMAN_BABY_BLOW:
        begoman_baby_move(b, 0, V16(3), R14(.95), 0, false);
        if(step > 20) begoman_baby_kill(b, true);
        break;
    case BEGOMAN_BABY_WATER:
        b->position = v3_add(b->position, V3(b->velocity.x >> 8, b->velocity.y >> 8, b->velocity.z >> 8));
        if(step > 60) begoman_baby_kill(b, true);
        break;
    default: break;
    }
}
static void jumpguarder_think(Actor* a){
    jumpguarder_ensure_children(a);
    Actor* h = jumpguarder_head(a);
    if(!h) return;
    int step = a->timer - 1;
    if(a->target.y >> 8) a->target.y -= 256;
    if(a->target.z & 1){
        const StageObject* o = actor_stage_object(a);
        if(o && g_gcam.event_active && g_gcam.event_first == o->camera){
            if(g_gcam.event_index == 0 && vertical_speed(g_mario.jump_vec, game_player()->up) <= 0){
                g_gcam.event_index = 1;
                g_gcam.event_frame = 0;
            }
            if(v3_length_squared64(v3_sub(a->home, mario_centre())) > 6250000 || game_player()->on_ground ||
               game_player()->action == MA_STAR_LAUNCH){
                camera_event_cancel();
                a->target.z &= ~1;
            }
        } else a->target.z &= ~1;
    }
    bool jumpvalid =
        a->state == JUMPGUARDER_HOPWAIT || a->state == JUMPGUARDER_HOPJUMP || a->state == JUMPGUARDER_HOPEND;
    if(jumpvalid){
        int jump = mario_sphere_contact(jumpguarder_jump_sensor(a), a->up, 145, false);
        if(jump == CONTACT_TRAMPLE){
            jumpguarder_launch_mario(a);
            step = 0;
        } else if(jump == CONTACT_HIPDROP) mario_stomp_bounce(g_game.keys.held & KEY_A);
    }
    int contact = mario_sphere_contact(jumpguarder_body_sensor(a), a->up, a->radius, true);
    if(contact == CONTACT_SPIN && a->state != JUMPGUARDER_HOPSTART){
        jumpguarder_set(a, JUMPGUARDER_HOPSTART, ANIM_JUMPGUARDERBODY_DAMAGE, ANIM_JUMPGUARDERHEAD_HOPSTART);
        step = 0;
    } else if(contact == CONTACT_TRAMPLE && !jumpvalid){
        mario_stomp_bounce(g_game.keys.held & KEY_A);
        jumpguarder_set(a, JUMPGUARDER_HOPSTART, ANIM_JUMPGUARDERBODY_DAMAGE, ANIM_JUMPGUARDERHEAD_HOPSTART);
        step = 0;
    } else if(contact == CONTACT_HIPDROP && !jumpvalid) mario_stomp_bounce(g_game.keys.held & KEY_A);

    else if(contact == CONTACT_TOUCH){
        v3 flat = v3_reject14(v3_sub(mario_centre(), jumpguarder_body_sensor(a)), a->up);
        int length = isqrt64(v3_length_squared64(flat));
        if(length > 0 && length < a->radius + C_BODY_RADIUS)
            mario_nudge(v3_scale14(v3_direction14(flat, a->front), a->radius + C_BODY_RADIUS - length));
    }

    if(a->timer == 1) switch(a->state){
        case JUMPGUARDER_UP: audio_play_effect(SFX_BUOY_EM_JGUARDER_APPEAR, 2); break;
        case JUMPGUARDER_DOWN: audio_play_effect(SFX_BUOY_EM_JGUARDER_HIDE, 2); break;
        case JUMPGUARDER_HOPSTART: audio_play_effect(SFX_BUOY_EM_JGUARDER_HIT, 2); break;
        case JUMPGUARDER_HOPJUMP: audio_play_effect(SFX_BUOY_EM_JGUARDER_TRAMPLE, 2); break;
        case JUMPGUARDER_HOPEND: audio_play_effect(SFX_BUOY_EM_JGUARDER_CLOSE_SPRING, 2); break;
        case JUMPGUARDER_OPEN: audio_play_effect(SFX_BUOY_EM_JGUARDER_SHUTTER_OPEN, 2); break;
        default: break;
        }
    switch(a->state){
    case JUMPGUARDER_HIDE:
        jumpguarder_turn_head(a);
        a->anim_frame -= FX_ONE;
        if(jumpguarder_can_appear(a, 2000)) jumpguarder_set(a, JUMPGUARDER_UP, ANIM_JUMPGUARDERBODY_UP, -1);
        break;
    case JUMPGUARDER_UP:
        if(step >= buoy_guarder_body_lengths[ANIM_JUMPGUARDERBODY_UP])
            jumpguarder_set(a, JUMPGUARDER_WAIT, -1, -1);
        break;
    case JUMPGUARDER_WAIT: {
        jumpguarder_turn_head(a);
        if(!jumpguarder_can_appear(a, 2200)){
            jumpguarder_set(a, JUMPGUARDER_DOWN, ANIM_JUMPGUARDERBODY_DOWN, -1);
            break;
        }
        bool dead = false;
        for(int n = 0; n < 4; n++){
            Actor* b = jumpguarder_baby(a, n);
            if(b && !b->active) dead = true;
        }
        if(dead && step % 360 == 0) jumpguarder_set(a, JUMPGUARDER_PREOPEN, -1, -1);
        break;
    }
    case JUMPGUARDER_DOWN:
        if(step >= buoy_guarder_body_lengths[ANIM_JUMPGUARDERBODY_DOWN])
            jumpguarder_set(a, JUMPGUARDER_HIDE, -1, ANIM_JUMPGUARDERHEAD_WAIT);
        break;
    case JUMPGUARDER_HOPSTART:
        if(!step) a->radius = 120;
        if(step == 10){
            for(int n = 0; n < 4; n++)
                if(a->target.y & (1 << n)){
                    Actor* b = jumpguarder_baby(a, n);
                    if(b) begoman_baby_retire(b);
                }
            a->target.y &= ~15;
        }
        if((h->anim_frame >> 8) >= buoy_guarder_head_lengths[ANIM_JUMPGUARDERHEAD_HOPSTART])
            jumpguarder_set(a, JUMPGUARDER_HOPWAIT, -1, ANIM_JUMPGUARDERHEAD_HOPWAIT);
        break;
    case JUMPGUARDER_HOPWAIT:
        a->anim_frame -= FX_ONE;
        if(step == 300)
            jumpguarder_set(a, JUMPGUARDER_HOPEND, ANIM_JUMPGUARDERBODY_HOPEND, ANIM_JUMPGUARDERHEAD_HOPEND);
        break;
    case JUMPGUARDER_HOPJUMP:
        a->anim_frame -= FX_ONE;
        if((h->anim_frame >> 8) >= buoy_guarder_head_lengths[ANIM_JUMPGUARDERHEAD_HOPJUMP])
            jumpguarder_set(a, JUMPGUARDER_HOPWAIT, -1, ANIM_JUMPGUARDERHEAD_HOPWAIT);
        break;
    case JUMPGUARDER_HOPEND:
        if(step >= buoy_guarder_body_lengths[ANIM_JUMPGUARDERBODY_HOPEND] &&
           (h->anim_frame >> 8) >= buoy_guarder_head_lengths[ANIM_JUMPGUARDERHEAD_HOPEND]){
            a->radius = 145;
            jumpguarder_set(a, JUMPGUARDER_WAIT, -1, -1);
        }
        break;
    case JUMPGUARDER_PREOPEN:
        jumpguarder_turn_head(a);
        if(!jumpguarder_can_appear(a, 2200))
            jumpguarder_set(a, JUMPGUARDER_DOWN, ANIM_JUMPGUARDERBODY_DOWN, -1);
        else if(!step) jumpguarder_set(a, JUMPGUARDER_OPEN, ANIM_JUMPGUARDERBODY_OPEN, -1);
        break;
    case JUMPGUARDER_OPEN:
        if(step < 70) jumpguarder_turn_head(a);
        if(!step){
            a->target.y &= ~15;
            for(int n = 0; n < 4; n++){
                Actor* b = jumpguarder_baby(a, n);
                if(b && !b->active) a->target.y |= 1 << n;
            }
        }
        jumpguarder_pose_head(a);
        jumpguarder_release_babies(a, !step);
        if(step == 74) a->target.y &= ~15;
        if(step >= buoy_guarder_body_lengths[ANIM_JUMPGUARDERBODY_OPEN])
            jumpguarder_set(a, JUMPGUARDER_CLOSE, ANIM_JUMPGUARDERBODY_CLOSE, -1);
        break;
    case JUMPGUARDER_CLOSE:
        if(step >= buoy_guarder_body_lengths[ANIM_JUMPGUARDERBODY_CLOSE])
            jumpguarder_set(a, JUMPGUARDER_INTER, -1, -1);
        break;
    case JUMPGUARDER_INTER:
        jumpguarder_turn_head(a);
        jumpguarder_set(a, jumpguarder_can_appear(a, 2200) ? JUMPGUARDER_WAIT : JUMPGUARDER_DOWN,
                        jumpguarder_can_appear(a, 2200) ? -1 : ANIM_JUMPGUARDERBODY_DOWN, -1);
        break;
    }
    jumpguarder_pose_head(a);
}
static bool jumpguarder_dispatch(Actor* a){
    if(a->type == OBJ_BUOYJUMPGUARDER){
        jumpguarder_think(a);
        return true;
    }
    if(a->type == OBJ_BUOYJUMPHEAD){
        Actor* p = actor_at_slot(a->args[3], OBJ_BUOYJUMPGUARDER);
        if(p && p->active) jumpguarder_pose_head(p);
        else a->active = 0;
        return true;
    }
    if(a->type == OBJ_BUOYBEGOBABY){
        begoman_baby_think(a);
        return true;
    }
    return false;
}
static bool __attribute__((noinline)) jumpguarder_unclipped(const Actor* a){
    if(a->type == OBJ_BUOYJUMPHEAD) return true;
    if(a->type == OBJ_BUOYJUMPGUARDER) return a->state != JUMPGUARDER_HIDE;
    return a->type == OBJ_BUOYBEGOBABY && a->state != BEGOMAN_BABY_WAIT && a->state != BEGOMAN_BABY_NOCALC;
}
