enum { ICEKING_FACING_WALL = 0x08 };

#define SFX_ICEKING_PRE_ANGRY2 SFX_ICEKING_PRE_ANGRY1
#define SFX_ICEKING_ANGRY2     SFX_ICEKING_ANGRY1
enum {
    ICEKING_SEARCH = 120,
    ICEKING_BEGIN_DEMO,
    ICEKING_THROW,
    ICEKING_THROW2,
    ICEKING_EXTINGUISH,
    ICEKING_ESCAPE,
    ICEKING_ESCAPE_JUMP,
    ICEKING_DAMAGE,
    ICEKING_PRE_RECOVER,
    ICEKING_RECOVER,
    ICEKING_PRE_ATTACK,
    ICEKING_ATTACK,
    ICEKING_ATTACK_AFTER,
    ICEKING_ANGRY1,
    ICEKING_ANGRY2,
    ICEKING_DEATH_WAIT,
    ICEKING_DEATH
};
static EWRAM_BSS IceKingState g_iceking;
#define ICEKING_MOVE_LIMIT_READY 2
static v3 iceking_limit_vertex(const CollisionTriangle* t, int i, v3 origin){
    return V3(t->v[i][0] - origin.x, t->v[i][1] - origin.y, t->v[i][2] - origin.z);
}
__attribute__((noinline)) static bool iceking_limit_ray(v3 from, v3 to, RayHit* hit){
    v3 d = v3_sub(to, from);
    int32_t best = N14_ONE + 1;
    for(int i = 0; i < ICEKING_MOVE_LIMIT_COUNT; i++){
        const CollisionTriangle* t = &iceking_move_limit[i];
        v3 n = v3_from_i16(t->normal), a = iceking_limit_vertex(t, 0, from);
        fx d0 = -v3_dot14(a, n), d1 = d0 + v3_dot14(d, n);
        if(d0 < 0 || d1 >= 0) continue;
        int32_t tt = ratio14(d0, (int64_t)d0 - d1);
        if(tt >= best) continue;
        v3 p = V3((int32_t)(((int64_t)d.x * tt + 8192) >> 14), (int32_t)(((int64_t)d.y * tt + 8192) >> 14),
                  (int32_t)(((int64_t)d.z * tt + 8192) >> 14));
        v3 b = iceking_limit_vertex(t, 1, from), c = iceking_limit_vertex(t, 2, from);
        v3 pa = v3_sub(a, p), pb = v3_sub(b, p), pc = v3_sub(c, p);
        int64_t w0 = dot64(v3_cross(pa, pb), n), w1 = dot64(v3_cross(pb, pc), n),
                w2 = dot64(v3_cross(pc, pa), n);
        if(!((w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0))) continue;
        best = tt;
        hit->t = tt;
        hit->point = v3_add(from, p);
        hit->normal = n;
        hit->attr = t->attr;
        hit->tri = -1;
    }
    return best <= N14_ONE;
}
static bool iceking_limit_select(void){
    v3 position = v3_from_i32(iceking_spec.source.pos), up = v3_from_i16(iceking_spec.source.up);
    RayHit h;
    return iceking_limit_ray(v3_add(position, v3_scale14(up, 150)), v3_sub(position, v3_scale14(up, 850)),
                             &h);
}
GBA_ARM_TARGET_ATTR __attribute__((noinline)) static bool iceking_limit_sphere(
    int i, v3 centre, fx radius, SphereContact* out){
    const CollisionTriangle* t = &iceking_move_limit[i];
    v3 n = v3_from_i16(t->normal), a = iceking_limit_vertex(t, 0, centre);
    fx dist = -v3_dot14(a, n);
    if(dist > radius || dist < 0) return false;
    v3 b = iceking_limit_vertex(t, 1, centre), c = iceking_limit_vertex(t, 2, centre);
    for(int axis = 0; axis < 3; axis++){
        fx av = (&a.x)[axis], bv = (&b.x)[axis], cv = (&c.x)[axis];
        if((av > radius && bv > radius && cv > radius) || (av < -radius && bv < -radius && cv < -radius))
            return false;
    }
    v3 q = collectible_closest_point(a, b, c, v3_scale14(n, -dist));
    uint64_t d2 = v3_length_squared64(q);
    if(d2 > (uint64_t)((int64_t)radius * radius)) return false;
    fx d = (fx)isqrt64(d2);
    out->normal = d > 2 && dist > 0 ? v3_direction14(v3_neg(q), n) : n;
    out->depth = radius - d;
    out->point = v3_add(centre, q);
    out->attr = t->attr;
    out->tri = -1;
    return true;
}
static void iceking_limit_contact(Actor* a, const SphereContact* hit, bool child){
    v3 up = child ? a->up : g_iceking.gravity_up;
    int dot = v3_dot14(hit->normal, up);
    if(dot > R14(.5f)){
        a->flags |= ACTOR_ON_GROUND;
        if(!child) g_iceking.ground_normal = hit->normal;
    } else if(!child && dot > -R14(.5f) && v3_dot14(hit->normal, v3_neg(a->front)) > R14(.25f))
        a->flags |= ICEKING_FACING_WALL;
    if(!child){
        int v = v3_dot14(a->velocity, hit->normal);
        if(v < 0) a->velocity = v3_sub(a->velocity, v3_scale14(hit->normal, v));
    }
}
__attribute__((noinline)) static v3 iceking_limit_bind(Actor* a, v3 from, v3 centre, fx radius, bool child){
    if(!(g_iceking.ready & ICEKING_MOVE_LIMIT_READY)) return centre;
    RayHit ray;
    if(iceking_limit_ray(from, centre, &ray)){
        centre = v3_add(ray.point, v3_scale14(ray.normal, radius));
        SphereContact hit = {.normal = ray.normal};
        iceking_limit_contact(a, &hit, child);
    }
    for(int i = 0; i < ICEKING_MOVE_LIMIT_COUNT; i++){
        SphereContact hit;
        if(!iceking_limit_sphere(i, centre, radius, &hit) || hit.depth < 0) continue;
        centre = v3_add(centre, v3_scale14(hit.normal, hit.depth + 1));
        iceking_limit_contact(a, &hit, child);
    }
    return centre;
}
static Actor* iceking_part(int slot, int type){
    if(slot < 0 || slot >= g_game.actor_count) return NULL;
    Actor* a = &g_game.actors[slot];
    return a->active && a->type == type && a->value == g_iceking.owner ? a : NULL;
}
static bool iceking_valid(const Actor* a){
    return g_iceking.stage == g_game.stage_index && a->type == OBJ_ICEKING &&
           actor_index(a) == g_iceking.owner;
}
static int iceking_frame(const Actor* a){
    int count = iceking_pose_count[a->anim], frame = a->anim_frame >> 8;
    return a->anim == ANIM_ICEMERAMERAKING_WAIT ? frame % (count - 1) : fx_min(frame, count - 1);
}
static const IceKingPose* iceking_pose(const Actor* a){
    return &iceking_pose_clips[a->anim][iceking_frame(a)];
}
static v3 iceking_local_world(const Actor* a, v3 local){
    v3 right = v3_cross14(a->front, a->up);
    return v3_add(v3_scale14(right, local.x),
                  v3_add(v3_scale14(a->front, local.y), v3_scale14(a->up, local.z)));
}
static v3 iceking_joint(const Actor* a, const int16_t p[3]){
    return v3_add(a->position, iceking_local_world(a, V3(-p[0], p[2], p[1])));
}
static v3 iceking_centre(const Actor* a){ return iceking_joint(a, iceking_pose(a)->root); }
static void iceking_set(Actor* a, int state, int anim){ actor_set_state(a, state, anim); }
static bool iceking_action_end(const Actor* a){
    return (a->anim_frame >> 8) >= iceking_pose_count[a->anim] - 1;
}
static void iceking_quat_normalize(void){
    int32_t q[4];
    uint32_t sum = 0;
    for(int i = 0; i < 4; i++){
        q[i] = g_iceking.roll[i];
        sum += (uint32_t)(q[i] * q[i]);
    }
    uint32_t length = isqrt32(sum);
    if(!length){
        g_iceking.roll[0] = g_iceking.roll[1] = g_iceking.roll[2] = 0;
        g_iceking.roll[3] = N14_ONE;
        return;
    }
    for(int i = 0; i < 4; i++) g_iceking.roll[i] = (int16_t)(q[i] * N14_ONE / (int32_t)length);
}
static v3 iceking_quat_vector(v3 v){
    v3 q = V3(g_iceking.roll[0], g_iceking.roll[1], g_iceking.roll[2]);
    v3 cross = v3_cross14(q, v);
    v3 add = v3_add(v3_scale14(cross, g_iceking.roll[3]), v3_cross14(q, cross));
    return v3_add(v, v3_scale14(add, N14_ONE * 2));
}
static void iceking_roll(Actor* a){
    v3 moment = v3_cross14((a->flags & ACTOR_ON_GROUND) ? g_iceking.ground_normal : g_iceking.gravity_up,
                           a->velocity);
    int length = v3_length_exact(moment);
    if(length < 1 || !a->radius) return;
    v3 axis = v3_direction14(moment, V3(0, 0, 0));
    fx angle = length / (a->radius / 2);
    v2 half = fx_sin_cos(angle / 2);
    v3 v = v3_scale14(axis, half.x << 6);
    v3 q = V3(g_iceking.roll[0], g_iceking.roll[1], g_iceking.roll[2]);
    int32_t w = g_iceking.roll[3], rw = half.y << 6;
    v3 nv = v3_add(v3_add(v3_scale14(v, w), v3_scale14(q, rw)), v3_cross14(q, v));
    int nw = mul14(w, rw) - v3_dot14(q, v);
    g_iceking.roll[0] = nv.x;
    g_iceking.roll[1] = nv.y;
    g_iceking.roll[2] = nv.z;
    g_iceking.roll[3] = nw;
    iceking_quat_normalize();
}
static void iceking_roll_recover(void){
    v3 q = V3(g_iceking.roll[0], g_iceking.roll[1], g_iceking.roll[2]);
    int w = g_iceking.roll[3];
    if(w < 0){
        q = v3_neg(q);
        w = -w;
    }
    int length = v3_length_exact(q);
    if(length < 4){
        memset(g_iceking.roll, 0, 6);
        g_iceking.roll[3] = N14_ONE;
        return;
    }

    fx half = fx_atan2(length, w) * 3 / 4;
    v2 sc = fx_sin_cos(half);
    q = v3_scale14(v3_direction14(q, V3(0, 0, 0)), sc.x << 6);
    g_iceking.roll[0] = q.x;
    g_iceking.roll[1] = q.y;
    g_iceking.roll[2] = q.z;
    g_iceking.roll[3] = sc.y << 6;
    iceking_quat_normalize();
}
static void iceking_figure(const Actor* a, Figure* f){
    v3 up = iceking_quat_vector(V3(0, 0, N14_ONE)), front = iceking_quat_vector(V3(0, N14_ONE, 0));
    const int16_t* root = iceking_pose(a)->root;
    v3 local = V3(-root[0], root[2], root[1]);

    v3 offset = v3_sub(local, iceking_quat_vector(local));
    f->position = v3_add(a->position, iceking_local_world(a, offset));
    f->basis_up = iceking_local_world(a, up);
    f->basis_front = iceking_local_world(a, front);
}
static void iceking_gravity(Actor* a){
    GravitySample g = gravity_sample(g_game.stage.header, a->position);
    g_iceking.gravity_up = g.index < 0 ? a->up : g.up;
    RayHit h;
    g_iceking.shadow = -1;
    if(collision_ray(a->position, v3_sub(a->position, v3_scale14(g_iceking.gravity_up, 1500)), &h, 0))
        g_iceking.shadow = v3_dot14(v3_sub(a->position, h.point), g_iceking.gravity_up);
}
static void iceking_keep_height(Actor* a, int height, int accel, int range){
    if(g_iceking.shadow < 0) return;
    int difference = g_iceking.shadow - height;
    int32_t force = accel * fx_clamp(difference, -range, range) / range;
    a->velocity = v3_sub(a->velocity, v3_scale14(g_iceking.gravity_up, force));
}
static void iceking_turn_move(Actor* a, int accel, int gravity, int damp, fx turn){
    v3 up = g_iceking.gravity_up;
    if(turn){
        v3 wanted = v3_tangent14(actor_to_mario(a), up, a->front);
        fx limit = turn;
        int32_t dot = v3_dot14(a->front, wanted);
        if(dot >= (fx_cos(limit) << 6)) a->front = wanted;
        else
            a->front = v3_tangent14(
                v3_rotate_axis14(a->front, up,
                                 v3_dot14(v3_cross14(a->front, wanted), up) >= 0 ? limit : -limit),
                up, wanted);
    }
    a->front = v3_tangent14(a->front, up, a->front);
    if(accel) a->velocity = v3_add(a->velocity, v3_scale14(a->front, accel));
    a->velocity = v3_scale14(a->velocity, damp);
    if(a->flags & ACTOR_ON_GROUND){
        int down = v3_dot14(a->velocity, g_iceking.ground_normal);
        if(down < 0) a->velocity = v3_sub(a->velocity, v3_scale14(g_iceking.ground_normal, down));
    }
    a->velocity =
        v3_sub(a->velocity, v3_scale14((a->flags & ACTOR_ON_GROUND) ? g_iceking.ground_normal : up, gravity));
}
static void iceking_home(Actor* a){
    v3 toward = v3_sub(a->home, a->position), up = g_iceking.gravity_up;
    if(v3_dot14(toward, up) < 0) toward = v3_reject14(toward, up);
    a->velocity = v3_add(a->velocity, v3_scale14(v3_direction14(toward, V3(0, 0, 0)), V16(.5f)));
}
__attribute__((noinline)) static void iceking_bind(Actor* a){
    v3 old = v3_add(a->position, v3_scale14(a->up, a->radius)), delta;
    for(int i = 0; i < 3; i++){
        int f = (&a->velocity.x)[i] + (&a->target.x)[i];
        (&delta.x)[i] = f >> 8;
        (&a->target.x)[i] = f & 255;
    }
    v3 centre = v3_add(old, delta);
    CollisionCandidates candidates;
    collision_gather_around(old, centre, a->radius + 1, &candidates);
    RayHit ray;
    a->flags &= ~(ACTOR_ON_GROUND | ICEKING_FACING_WALL);
    if(collision_raycast(&candidates, old, centre, &ray, 0))
        centre = v3_add(ray.point, v3_scale14(ray.normal, a->radius));
    for(int i = 0; i < candidates.count; i++){
        SphereContact hit;
        if(!collectible_sphere_triangle(candidates.index[i], centre, a->radius, &hit) || hit.depth < 0)
            continue;
        centre = v3_add(centre, v3_scale14(hit.normal, hit.depth + 1));
        int dot = v3_dot14(hit.normal, g_iceking.gravity_up);
        if(dot > R14(.5f)){
            a->flags |= ACTOR_ON_GROUND;
            g_iceking.ground_normal = hit.normal;
        } else if(dot > -R14(.5f) && v3_dot14(hit.normal, v3_neg(a->front)) > R14(.25f))
            a->flags |= ICEKING_FACING_WALL;
        int v = v3_dot14(a->velocity, hit.normal);
        if(v < 0) a->velocity = v3_sub(a->velocity, v3_scale14(hit.normal, v));
    }
    centre = iceking_limit_bind(a, old, centre, a->radius, false);
    a->position = v3_sub(centre, v3_scale14(a->up, a->radius));
}
static Actor* iceking_spawn_part(Actor* a, int type){
    Actor* part = actor_spawn_reuse(type, a->position, a->up, a->front);
    if(!part && stream_evict(a->position)) part = actor_spawn_reuse(type, a->position, a->up, a->front);
    if(part){
        part->value = actor_index(a);
        part->state = ICEKING_PART_DEAD;
        part->flags |= ACTOR_HIDDEN;
    }
    return part;
}
static void iceking_init(Actor* a, const StageObject* object){
    memset(&g_iceking, 0, sizeof g_iceking);
    g_iceking.stage = g_game.stage_index;
    g_iceking.owner = actor_index(a);
    memset(g_iceking.weapon, 255, sizeof g_iceking.weapon);
    memset(g_iceking.child, 255, sizeof g_iceking.child);
    g_iceking.shock = g_iceking.broken = 255;
    g_iceking.roll[3] = N14_ONE;
    g_iceking.ground_normal = g_iceking.gravity_up = a->up;
    a->health = 3;
    a->radius = 108;
    a->figure.scale = V3(96, 96, 96);
    a->value = 0;
    memset(a->args, 0, sizeof a->args);
    a->target = V3(0, 0, 0);
    iceking_set(a, ICEKING_SEARCH, ANIM_ICEMERAMERAKING_WAIT);
}
static bool iceking_prepare(Actor* a){
    if(!throwing_ice_pool_init(a)) return false;
    int n = 0;
    for(int i = 0; i < g_game.actor_count && n < 6; i++){
        Actor* p = &g_game.actors[i];
        if(p->active && p->type == OBJ_ICEKINGWEAPON && p->value == actor_index(a)) g_iceking.weapon[n++] = i;
    }
    const int type[4] = {OBJ_ICEKINGSHOCK, OBJ_ICEKINGBREAK, OBJ_ICECHILD, OBJ_ICECHILD};
    uint8_t* slots[4] = {&g_iceking.shock, &g_iceking.broken, &g_iceking.child[0], &g_iceking.child[1]};
    for(int i = 0; i < 4; i++)
        if(!iceking_part(*slots[i], type[i])){
            Actor* p = iceking_spawn_part(a, type[i]);
            if(!p) return false;
            *slots[i] = actor_index(p);
            if(i >= 2){
                const IceKingPlacedPose* pose = &iceking_spec.child[i - 2];
                p->position = p->home = v3_from_i32(pose->pos);
                p->up = v3_from_i16(pose->up);
                p->front = v3_from_i16(pose->front);
            }
        }
    g_iceking.ready = 1 | (iceking_limit_select() ? ICEKING_MOVE_LIMIT_READY : 0);
    return true;
}
static bool iceking_weapons_idle(void){
    for(int i = 0; i < 6; i++){
        Actor* p = iceking_part(g_iceking.weapon[i], OBJ_ICEKINGWEAPON);
        if(p && p->state != THROWING_ICE_IDLE) return false;
    }
    return true;
}
static Actor* iceking_weapon(Actor* a){
    for(int i = 0; i < 6; i++){
        int at = (a->args[3] + i) % 6;
        Actor* p = iceking_part(g_iceking.weapon[at], OBJ_ICEKINGWEAPON);
        if(p && p->state == THROWING_ICE_IDLE){
            a->args[3] = (a->args[3] + 1) % 6;
            return p;
        }
    }
    return NULL;
}
static bool iceking_can_throw(Actor* a){
    if(!switch_on(iceking_spec.throw_switch)) return false;
    if(g_game.player.action == MA_SWIM || g_game.player.water_state != WATER_DRY) return false;
    v3 toward = v3_direction14(v3_sub(mario_centre(), a->position), V3(0, 0, 0));
    return v3_dot14(a->up, toward) > -R14(.6f);
}
static bool iceking_touches_mario(const Actor* a, int extra){
    if(g_mario_held || g_game.player.action == MA_DEATH || g_game.player.action == MA_STAR_LAUNCH ||
       g_game.player.action == MA_STAR_GET)
        return false;
    int radius = a->radius + C_BODY_RADIUS + extra;
    return v3_length_squared64(v3_sub(mario_centre(), iceking_centre(a))) <= (uint64_t)radius * radius;
}
static bool iceking_exposed(const Actor* a){
    return a->state == ICEKING_ESCAPE || a->state == ICEKING_ESCAPE_JUMP;
}
static void iceking_take_hit(Actor* a){
    a->health--;
    if(a->health){
        iceking_set(a, ICEKING_DAMAGE, ANIM_ICEMERAMERAKING_DAMAGE);
    } else {
        iceking_set(a, ICEKING_DEATH_WAIT, -1);
        g_iceking.demo_tick = 0;
    }
}
static void iceking_push(Actor* a){
    Player* p = &g_game.player;

    v3 centre = mario_centre(), delta = v3_sub(iceking_centre(a), centre);
    int sphere = a->radius + C_BODY_RADIUS - v3_length_exact(delta);
    if(sphere < 0) return;
    int axial = v3_dot14(delta, p->up);
    if(axial <= -a->radius) return;
    v3 radial = v3_reject14(delta, p->up);
    int rpen = a->radius + C_BODY_RADIUS - v3_length_exact(radial);
    int apen = a->radius + C_BODY_HEIGHT - axial;
    if(rpen <= 0 || apen <= 0) return;
    v3 push;
    if(sphere < rpen && sphere < apen) push = v3_scale14(v3_direction14(v3_neg(delta), p->front), sphere);
    else if(rpen < apen) push = v3_scale14(v3_direction14(v3_neg(radial), p->front), rpen);
    else push = v3_sub(v3_scale14(p->up, -apen), v3_direction14(radial, V3(0, 0, 0)));
    push = v3_scale14(push, R14(1.1f));
    p->figure.position = v3_add(p->figure.position, push);
    g_mario.position16 = v3_add(g_mario.position16, V3(push.x * 256, push.y * 256, push.z * 256));
}
static void iceking_contacts(Actor* a){
    if(g_mario_held) return;
    bool spin = g_mario.attack_timer > 0;
    if(iceking_exposed(a)){
        if(spin && iceking_touches_mario(a, 40)){
            iceking_take_hit(a);
            return;
        }
    } else if(spin && a->state != ICEKING_DEATH && a->state != ICEKING_DEATH_WAIT &&
              a->state != ICEKING_EXTINGUISH && a->state != ICEKING_DAMAGE && a->state != ICEKING_ANGRY1 &&
              a->state != ICEKING_ANGRY2 && a->state != ICEKING_PRE_RECOVER && a->state != ICEKING_RECOVER){
        v3 to = v3_sub(mario_centre(), iceking_centre(a));
        if(v3_length_squared64(to) <= 600u * 600u &&
           v3_length_squared64(v3_reject14(v3_sub(mario_centre(), a->position), g_iceking.gravity_up)) >=
               (uint64_t)a->radius * a->radius){
            iceking_set(a, ICEKING_EXTINGUISH, ANIM_ICEMERAMERAKING_GOOUT);
            return;
        }
    }
    if(!iceking_touches_mario(a, 0)) return;
    if(mario_power_mode() == 3 || a->state == ICEKING_DAMAGE || a->state == ICEKING_EXTINGUISH ||
       iceking_exposed(a) || a->state == ICEKING_PRE_RECOVER || a->state == ICEKING_RECOVER ||
       a->state >= ICEKING_ANGRY1){
        iceking_push(a);
    } else (void)mario_freeze_hit();
}

static bool iceking_knock_back_mario(v3 direction){
    Player* p = &g_game.player;
    MarioPhysics* m = &g_mario;
    if(g_mario_held || g_mario_star_power || (g_debug_options & DEBUG_NO_DAMAGE) ||
       mario_freeze_blocks_damage() || p->invincible_timer || p->action == MA_DEATH ||
       p->action == MA_DAMAGE || p->action == MA_DAMAGE_FALL || p->action == MA_SWIM ||
       p->action == MA_TALK || p->action == MA_FIRE_DANCE || p->action == MA_FIRE_RUN ||
       p->action == MA_STAR_LAUNCH || p->action == MA_STAR_GET)
        return false;
    v3 away = v3_tangent14(direction, p->up, p->front);
    bool front = v3_dot14(away, p->front) < 0;
    mario_take_off(JUMP_DAMAGE, v3_add(v3_scale14(away, G(15)), v3_scale14(p->up, G(15))), true);
    m->ratio = 0;
    m->spin_timer = m->attack_timer = 0;
    mario_set_action(MA_DAMAGE_FALL);
    mario_restart_anim(front ? ANIM_MARIO_DAMAGEFRONT : ANIM_MARIO_DAMAGEBACK);
    audio_play_effect(SFX_ICEKING_FLIP, 4);
    return true;
}
static void iceking_reset(void){
    if(g_iceking.held) iceking_demo_end();
    memset(&g_iceking, 0, sizeof g_iceking);
    g_iceking.owner = 255;
}
static bool iceking_unclipped(const Actor* a){
    if(a->type == OBJ_ICEKING)
        return iceking_valid(a) && (g_iceking.demo || switch_on(iceking_spec.throw_switch));
    return a->type == OBJ_ICEKINGSHOCK || a->type == OBJ_ICEKINGBREAK || a->type == OBJ_ICECHILD;
}
static void iceking_part_appear(Actor* p, const Actor* owner, int anim, int scale){
    if(!p) return;
    p->position = owner->position;

    p->up = v3_from_i16(iceking_spec.source.up);
    p->front = v3_from_i16(iceking_spec.source.front);
    p->flags &= ~ACTOR_HIDDEN;
    p->figure.scale = V3(scale, scale, scale);
    actor_set_state(p, ICEKING_PART_LIVE, anim);
}
static void iceking_shock_think(Actor* p){
    if(p->state == ICEKING_PART_DEAD){
        p->timer = 0;
        return;
    }
    int step = p->timer - 1;
    if(step >= 68){
        p->state = ICEKING_PART_DEAD;
        p->flags |= ACTOR_HIDDEN;
        return;
    }
    p->anim_frame = step * FX_ONE;
    v3 centre = v3_sub(p->position, v3_scale14(p->up, 100));
    v3 delta = v3_sub(mario_centre(), centre);
    int outside = iceking_shock_radii[step][0], inside = iceking_shock_radii[step][1];
    uint64_t square = v3_length_squared64(delta);
    if(square > (uint64_t)(outside + C_BODY_RADIUS) * (outside + C_BODY_RADIUS) ||
       square < (uint64_t)(inside + C_BODY_RADIUS) * (inside + C_BODY_RADIUS) ||
       fx_abs(v3_dot14(delta, p->up)) >= 200)
        return;
    (void)iceking_knock_back_mario(v3_reject14(delta, p->up));
}
static void iceking_break_think(Actor* p){
    if(p->state == ICEKING_PART_DEAD){
        p->timer = 0;
        return;
    }
    int step = p->timer - 1;
    p->anim_frame = fx_min(step, 159) * FX_ONE;
    if(step >= 159) p->flags |= ACTOR_HIDDEN;
}
static void iceking_finish(Actor* a){
    for(int i = 0; i < 6; i++){
        Actor* p = iceking_part(g_iceking.weapon[i], OBJ_ICEKINGWEAPON);
        if(p) p->active = 0;
    }
    Actor* broken = iceking_part(g_iceking.broken, OBJ_ICEKINGBREAK);
    if(broken) broken->active = 0;
    const StageObject* o = &g_game.stage.objects[a->stage_index];
    switch_set(o->sw[SWITCH_DEAD], true);

    if(!power_star_revealed(iceking_spec.star)){
        const StageObject* star = &g_game.stage.objects[iceking_spec.star];
        Actor* p = NULL;
        for(int i = 0; i < g_game.actor_count; i++)
            if(g_game.actors[i].type == OBJ_POWERSTAR && g_game.actors[i].stage_index == iceking_spec.star){
                p = &g_game.actors[i];
                break;
            }
        if(!p) p = actors_spawn_object(&g_game.stage, iceking_spec.star);
        if(p){
            power_star_reveal(p);
            p->position = a->position;
            p->up = v3_from_i16(star->up);
        }
    }

    a->active = 0;
    iceking_demo_end();
}
__attribute__((noinline)) static void iceking_think_nerve(Actor* a){
    if(!iceking_valid(a)) return;
    if(!g_iceking.ready && !iceking_prepare(a)) return;
    int step = a->timer - 1, state = a->state;
    iceking_gravity(a);
    a->radius = iceking_pose(a)->radius;
    if(!iceking_exposed(a)) iceking_roll_recover();
    if(!g_iceking.opened && switch_on(iceking_spec.opening_switch) && !g_mario_held && !camera_event_busy()){
        g_iceking.opened = 1;
        iceking_demo_start(0);
    }
    if(g_iceking.demo){
        int demo = g_iceking.demo, tick = g_iceking.demo_tick++;
        if(demo == 1 && tick >= 74){
            iceking_demo_start(1);
            a->args[1] = 0;
            a->front = v3_from_i16(iceking_spec.source.front);
            iceking_set(a, ICEKING_BEGIN_DEMO, ANIM_ICEMERAMERAKING_ATTACKTHROW);
            audio_play_effect(SFX_ICEKING_MEET, 4);
            step = 0;
            state = a->state;
        } else if(demo == 2 && tick >= 179){
            g_iceking.pinch = 1;
            g_iceking.first_pinch = 1;
            iceking_demo_end();
        } else if(demo == 3 && tick == 5){
            camera_event_start(iceking_spec.camera[5], 1, a->home, v3_from_i16(iceking_spec.source.up),
                               v3_from_i16(iceking_spec.source.front));
        } else if(demo == 3 && tick == 100){
            camera_event_start(iceking_spec.camera[2], 1, a->home, v3_from_i16(iceking_spec.source.up),
                               v3_from_i16(iceking_spec.source.front));
        }
    }
    switch(state){
    case ICEKING_SEARCH:
        if(!step){
            a->anim = ANIM_ICEMERAMERAKING_WAIT;
            a->anim_frame = 0;
        }
        iceking_home(a);
        iceking_turn_move(a, 0, V16(.5f), R14(.9f), DEG(1));
        iceking_keep_height(a, 400, V16(1.5f), 300);
        if(!g_iceking.demo && actor_mario_distance(a) <= 4500){
            if(actor_mario_distance(a) < 1000)
                iceking_set(a, ICEKING_PRE_ATTACK, ANIM_ICEMERAMERAKING_PREATTACK);
            else if(iceking_can_throw(a) && step > (a->health > 2 ? 200 : 100) && iceking_weapons_idle()){
                a->args[1] = 0;
                iceking_set(a, a->health > 2 ? ICEKING_THROW : ICEKING_THROW2,
                            ANIM_ICEMERAMERAKING_ATTACKTHROW);
            }
        }
        break;
    case ICEKING_BEGIN_DEMO:
    case ICEKING_THROW:
    case ICEKING_THROW2:
        iceking_turn_move(a, 0, V16(.5f), R14(.9f), DEG(1));
        iceking_keep_height(a, 400, V16(1.5f), 300);
        iceking_home(a);
        if(step == 22){
            Actor* p = iceking_weapon(a);
            if(!p){
                iceking_set(a, ICEKING_SEARCH, ANIM_ICEMERAMERAKING_WAIT);
                break;
            }
            a->args[1]++;
            Player* player = &g_game.player;
            v3 motion = g_mario_held      ? V3(0, 0, 0)
                        : g_mario.jumping ? g_mario.jump_vec
                                          : v3_scale14(player->front, player->forward_speed);
            v3 target =
                v3_add(mario_centre(), V3((motion.x * 35) >> 8, (motion.y * 35) >> 8, (motion.z * 35) >> 8));
            throwing_ice_emit(p, iceking_joint(a, iceking_pose(a)->top), target, g_iceking.gravity_up);
            audio_play_effect(SFX_ICEKING_THROW, 3);
        }
        if(iceking_action_end(a)){
            if(actor_mario_distance(a) < 1000)
                iceking_set(a, ICEKING_PRE_ATTACK, ANIM_ICEMERAMERAKING_PREATTACK);
            else if(a->args[1] >= (state == ICEKING_THROW2 ? 6 : 3) || !iceking_can_throw(a))
                iceking_set(a, ICEKING_SEARCH, ANIM_ICEMERAMERAKING_WAIT);
            else
                iceking_set(a, state == ICEKING_THROW2 ? ICEKING_THROW2 : ICEKING_THROW,
                            ANIM_ICEMERAMERAKING_ATTACKTHROW);
        }
        break;
    case ICEKING_EXTINGUISH:
        if(!step){
            v3 away = v3_reject14(v3_sub(a->position, mario_centre()), g_iceking.gravity_up);
            a->velocity = v3_scale14(v3_direction14(away, V3(0, 0, 0)), V16(20));
            g_iceking.height = g_iceking.shadow + 200;
            audio_play_effect(SFX_ICEKING_BLOW, 4);
        }
        if(step < 40){
            iceking_keep_height(a, g_iceking.height, V16(1.3f), 700);
            iceking_turn_move(a, 0, V16(.125f), R14(.96f), 0);
        } else if((a->flags & ACTOR_ON_GROUND) && iceking_action_end(a)){
            iceking_turn_move(a, 0, V16(1), R14(.8f), 0);
            a->args[0] = 0;
            audio_play_effect(SFX_ICEKING_LAND, 3);
            iceking_set(a, ICEKING_ESCAPE, -1);
        } else iceking_turn_move(a, 0, V16(4), R14(.96f), 0);
        break;
    case ICEKING_ESCAPE:
    case ICEKING_ESCAPE_JUMP:
        a->args[0]++;
        if(state == ICEKING_ESCAPE && a->args[0] > 600 && (a->flags & ACTOR_ON_GROUND)){
            a->args[0] = 0;
            iceking_set(a, ICEKING_PRE_RECOVER, -1);
            break;
        }
        if(state == ICEKING_ESCAPE_JUMP && step == 20){
            a->velocity = v3_add(a->velocity, v3_add(v3_scale14(a->front, V16(20)),
                                                     v3_scale14(g_iceking.gravity_up, V16(90))));
            audio_play_effect(SFX_ICEKING_JUMP, 3);
        }
        iceking_roll(a);
        if(a->flags & ACTOR_ON_GROUND){
            iceking_turn_move(a, V16(-.7f), V16(1), R14(.96f), DEG(180));
            if(state == ICEKING_ESCAPE && (a->flags & ICEKING_FACING_WALL))
                iceking_set(a, ICEKING_ESCAPE_JUMP, -1);
            else if(state == ICEKING_ESCAPE_JUMP && step > 20){
                iceking_set(a, ICEKING_ESCAPE, -1);
                audio_play_effect(SFX_ICEKING_LAND, 3);
            }
        } else iceking_turn_move(a, 0, V16(3), R14(.96f), 0);
        break;
    case ICEKING_DAMAGE:
        if(!step){
            a->velocity = v3_scale14(g_iceking.gravity_up, V16(80));
            audio_play_effect(SFX_ICEKING_DAMAGE, 4);
        }
        if(step > 20) iceking_keep_height(a, 900, V16(3), 300);
        iceking_turn_move(a, 0, (a->flags & ACTOR_ON_GROUND) ? V16(1) : V16(1.9f),
                          (a->flags & ACTOR_ON_GROUND) ? R14(.8f) : R14(.98f), 0);
        if(step > 60 && !g_mario_held && !camera_event_busy()){
            iceking_demo_start(2);
            iceking_set(a, a->health == 1 ? ICEKING_ANGRY2 : ICEKING_ANGRY1,
                        a->health == 1 ? ANIM_ICEMERAMERAKING_SUMMON : ANIM_ICEMERAMERAKING_RECOVER);
        }
        break;
    case ICEKING_PRE_RECOVER:
        if(!step){
            a->velocity = v3_scale14(g_iceking.gravity_up, V16(70));
            audio_play_effect(SFX_ICEKING_JUMP, 3);
        }
        iceking_home(a);
        if(step < 50) iceking_turn_move(a, 0, V16(1), R14(.96f), 0);
        else if(step == 50){
            a->anim = ANIM_ICEMERAMERAKING_PRERECOVER;
            a->anim_frame = 0;
            a->radius = 130;
            iceking_turn_move(a, 0, V16(.5f), R14(.9f), DEG(1));
        } else if(step < 120){
            iceking_keep_height(a, 750, V16(1.5f), 300);
            iceking_turn_move(a, 0, V16(.5f), R14(.9f), DEG(1));
        } else iceking_set(a, ICEKING_RECOVER, ANIM_ICEMERAMERAKING_RECOVER);
        break;
    case ICEKING_RECOVER:
        if(!step) audio_play_effect(SFX_ICEKING_PRE_RECOVER, 3);
        if(step == 80) audio_play_effect(SFX_ICEKING_RECOVER, 3);
        iceking_home(a);
        iceking_turn_move(a, 0, V16(.5f), R14(.9f), DEG(1));
        iceking_keep_height(a, 750, V16(1.5f), 300);
        if(step > 160) iceking_set(a, ICEKING_SEARCH, ANIM_ICEMERAMERAKING_WAIT);
        break;
    case ICEKING_PRE_ATTACK:
        if(step < 120) iceking_keep_height(a, 800, V16(1.5f), 300);
        else {
            Actor* shock = iceking_part(g_iceking.shock, OBJ_ICEKINGSHOCK);
            if(!shock || shock->state == ICEKING_PART_DEAD){
                iceking_set(a, ICEKING_ATTACK, ANIM_ICEMERAMERAKING_ATTACK);
                break;
            }
        }
        iceking_home(a);
        iceking_turn_move(a, 0, V16(.5f), R14(.9f), DEG(1));
        break;
    case ICEKING_ATTACK:
        if(!step) audio_play_effect(SFX_ICEKING_PRE_ATTACK, 3);
        if(!iceking_action_end(a)){
            iceking_keep_height(a, 800, V16(1.5f), 300);
            iceking_turn_move(a, 0, V16(.5f), R14(.9f), DEG(1));
        } else iceking_turn_move(a, 0, V16(6), R14(.98f), 0);
        if(a->flags & ACTOR_ON_GROUND) iceking_set(a, ICEKING_ATTACK_AFTER, ANIM_ICEMERAMERAKING_ATTACKEND);
        break;
    case ICEKING_ATTACK_AFTER:
        if(!step){
            iceking_part_appear(iceking_part(g_iceking.shock, OBJ_ICEKINGSHOCK), a,
                                ANIM_ICEMERAMERAKINGSHOCK_SHOCK, 64);
            audio_play_effect(SFX_ICEKING_HIP_DROP, 4);
            camera_shake(2);
        }
        iceking_turn_move(a, 0, V16(3), R14(.96f), 0);
        if(step > 90) iceking_set(a, ICEKING_SEARCH, ANIM_ICEMERAMERAKING_WAIT);
        break;
    case ICEKING_ANGRY1:
    case ICEKING_ANGRY2:
        if(!step)
            audio_play_effect(state == ICEKING_ANGRY1 ? SFX_ICEKING_PRE_ANGRY1 : SFX_ICEKING_PRE_ANGRY2, 4);
        if(step == 1){
            a->position = a->home;
            a->target = V3(0, 0, 0);
        }
        iceking_turn_move(a, 0, V16(.5f), R14(.9f), DEG(1.5f));
        iceking_keep_height(a, 400, V16(1.5f), 300);
        if(state == ICEKING_ANGRY2 && step == 90) iceking_children_appear();
        if(step == 85)
            audio_play_effect(state == ICEKING_ANGRY1 ? SFX_ICEKING_ANGRY1 : SFX_ICEKING_ANGRY2, 4);
        if(g_iceking.demo_tick >= 162){
            collectibles_pop_spread(OBJ_STARBIT, v3_add(a->position, v3_scale14(g_iceking.gravity_up, 200)),
                                    g_iceking.gravity_up, a->health > 2 ? 16 : 8, 15, 70);
            audio_play_effect(SFX_ICEKING_STAR_PIECE, 3);
            iceking_demo_end();
            iceking_set(a, ICEKING_SEARCH, ANIM_ICEMERAMERAKING_WAIT);
        }
        break;
    case ICEKING_DEATH_WAIT:
        if(!step) audio_stop_music();
        a->velocity = V3(0, 0, 0);
        if(!g_mario_held && !camera_event_busy()){
            iceking_demo_start(4);
            camera_event_target(a->position, a->up, a->front);
            iceking_set(a, ICEKING_DEATH, ANIM_ICEMERAMERAKING_DEATH);
        }
        break;
    case ICEKING_DEATH:
        if(!step){
            audio_play_effect(SFX_ICEKING_DEAD, 4);
            iceking_part_appear(iceking_part(g_iceking.broken, OBJ_ICEKINGBREAK), a,
                                ANIM_ICEMERAMERAKINGBREAK_BREAK, 96);
            iceking_children_kill();
        }
        if(a->flags & ACTOR_ON_GROUND) a->velocity = V3(0, 0, 0);
        else iceking_turn_move(a, 0, V16(3), R14(.96f), 0);
        if(step > 122) a->flags |= ACTOR_HIDDEN;
        if(iceking_action_end(a) && (a->flags & ACTOR_HIDDEN)){
            Actor* p = iceking_part(g_iceking.broken, OBJ_ICEKINGBREAK);
            if(!p || (p->flags & ACTOR_HIDDEN)) iceking_finish(a);
        }
        break;
    }
    if(a->active){
        iceking_bind(a);
        if(!g_iceking.demo) iceking_contacts(a);
    }
}

static void iceking_think(Actor* a){
    iceking_think_nerve(a);
    int end = (iceking_pose_count[a->anim] - 1) * FX_ONE;
    fx next = a->anim == ANIM_ICEMERAMERAKING_WAIT ? (a->anim_frame + FX_ONE) % end
                                                   : fx_min(a->anim_frame + FX_ONE, end);
    a->anim_frame = next - FX_ONE;
}
