#define DROP_POP      2
#define DROP_WATER    4
#define DROP_CUSTOM   16
#define DROP_MOVE     64
#define DROP_GROUND   1
#define DROP_WALL     2
#define DROP_ROOF     4
#define DROP_SQUEEZED 8
#define DROP_BURN     16

GBA_ARM_TARGET_ATTR __attribute__((always_inline)) static inline v3 collectible_closest_point(
    v3 a, v3 b, v3 c, v3 projected){
    v3 ab = v3_sub(b, a), ac = v3_sub(c, a);
    int64_t d1 = -dot64(ab, a), d2 = -dot64(ac, a);
    if(d1 <= 0 && d2 <= 0) return a;
    int64_t d3 = -dot64(ab, b), d4 = -dot64(ac, b);
    if(d3 >= 0 && d4 <= d3) return b;
    int64_t d5 = -dot64(ab, c), d6 = -dot64(ac, c);

    uint64_t big = (uint64_t)((d1 ^ (d1 >> 63)) | (d2 ^ (d2 >> 63)) | (d3 ^ (d3 >> 63)) | (d4 ^ (d4 >> 63)) |
                              (d5 ^ (d5 >> 63)) | (d6 ^ (d6 >> 63)));
    int sh = big >> 30 ? 34 - __builtin_clzll(big) : 0;
    int64_t s1 = d1 >> sh, s2 = d2 >> sh, s3 = d3 >> sh, s4 = d4 >> sh;
    int64_t vc = s1 * s4 - s3 * s2;
    if(vc <= 0 && d1 >= 0 && d3 <= 0) return lerp_point(a, b, d1, d1 - d3);
    if(d6 >= 0 && d5 <= d6) return c;
    int64_t s5 = d5 >> sh, s6 = d6 >> sh;
    int64_t vb = s5 * s2 - s1 * s6;
    if(vb <= 0 && d2 >= 0 && d6 <= 0) return lerp_point(a, c, d2, d2 - d6);
    int64_t va = s3 * s6 - s5 * s4;
    if(va <= 0 && (d4 - d3) >= 0 && (d5 - d6) >= 0) return lerp_point(b, c, d4 - d3, (d4 - d3) + (d5 - d6));
    return projected;
}

GBA_ARM_TARGET_ATTR __attribute__((noinline)) static bool collectible_sphere_triangle(
    int tri, v3 centre, fx radius, SphereContact* out){
    const CollisionTriangle* t = col_tri(tri);
    v3 n = v3_from_i16(t->normal);
    v3 a = col_vertex(t, 0, centre);
    fx dist = -v3_dot14(a, n);

    if(dist > radius || dist < 0) return false;
    v3 b = col_vertex(t, 1, centre), c = col_vertex(t, 2, centre);

    for(int axis = 0; axis < 3; axis++){
        fx av = (&a.x)[axis], bv = (&b.x)[axis], cv = (&c.x)[axis];
        if((av > radius && bv > radius && cv > radius) || (av < -radius && bv < -radius && cv < -radius))
            return false;
    }
    v3 projected = v3_scale14(n, -dist), q;
    bool inside = false;

    if(n.x == 0 && n.y == 0 && n.z == N14_ONE && a.z == b.z && b.z == c.z && fx_abs(a.x) <= 4096 &&
       fx_abs(a.y) <= 4096 && fx_abs(b.x) <= 4096 && fx_abs(b.y) <= 4096 && fx_abs(c.x) <= 4096 &&
       fx_abs(c.y) <= 4096){
        int64_t e0 = (int64_t)a.x * b.y - (int64_t)a.y * b.x;
        int64_t e1 = (int64_t)b.x * c.y - (int64_t)b.y * c.x;
        int64_t e2 = (int64_t)c.x * a.y - (int64_t)c.y * a.x;
        inside = (e0 > 0 && e1 > 0 && e2 > 0) || (e0 < 0 && e1 < 0 && e2 < 0);
    }
    q = inside ? projected : collectible_closest_point(a, b, c, projected);
    uint64_t d2 = v3_length_squared64(q);
    if(d2 > (uint64_t)((int64_t)radius * radius)) return false;

    int axis = q.x == 0 && q.y == 0 ? 2 : q.x == 0 && q.z == 0 ? 1 : q.y == 0 && q.z == 0 ? 0 : -1;
    fx d = axis >= 0 ? fx_abs((&q.x)[axis]) : (fx)isqrt64(d2);
    if(d > 2 && dist > 0){
        if(axis >= 0){
            out->normal = V3(0, 0, 0);
            fx component = (&q.x)[axis], m = fx_abs(component);

            (&out->normal.x)[axis] = component < 0 ? ((m & (m - 1)) ? N14_ONE - 1 : N14_ONE) : -N14_ONE;
        } else out->normal = v3_direction14(v3_neg(q), n);
    } else out->normal = n;
    out->depth = dist >= 0 ? radius - d : radius + d;
    out->point = v3_add(centre, q);
    out->attr = t->attr;
    out->tri = (int16_t)tri;
    return true;
}

static bool collectible_is_pop(const Actor* a){
    return a->stage_index < 0 && (a->flags & DROP_POP) && (a->type == OBJ_COIN || a->type == OBJ_STARBIT);
}
static bool collectible_is_drop(const Actor* a){
    return collectible_is_pop(a) && !(a->flags & DROP_NO_FALL);
}
static bool collectible_drop_ready(const Actor* a){
    return !collectible_is_drop(a) || a->timer >= (a->type == OBJ_COIN ? 30 : 26);
}
static int32_t collectible_random_velocity(uint32_t* seed, int range){
    *seed = *seed * 1664525u + 1013904223u;
    return (int32_t)(((*seed >> 16) * (uint32_t)(2 * range + 1)) >> 16) - range;
}
static v3 collectible_rebound(v3 v, v3 normal, int vertical, int tangent){
    int32_t dot = v3_dot14(v, normal);
    if(dot >= 0) return v;
    return v3_sub(v3_scale14(v3_sub(v, v3_scale14(normal, dot)), tangent),
                  v3_scale14(normal, mul14(dot, vertical)));
}

__attribute__((noinline)) static void collectible_collide(Actor* a, v3 motion, fx radius, fx offset){
    v3 off = v3_scale14(a->up, offset), from = v3_add(a->position, off), to = v3_add(from, motion);
    CollisionCandidates candidates;
    collision_gather_around(from, to, radius + 1, &candidates);

    bool settled = !(a->up.x || a->up.y) && a->up.z == N14_ONE && a->type == OBJ_COIN && motion.x == 0 &&
                   motion.y == 0 && motion.z == -1;
    bool resting = settled;
    if(resting)
        for(int i = 0; i < candidates.count; i++)
            if(candidates.index[i] >= DYNCOL_BASE){
                resting = false;
                break;
            }
    if(resting && a->args[0] && a->value == DROP_GROUND && (uint32_t)a->target.x == g_col_parts_hidden &&
       a->front.x == a->position.x && a->front.y == a->position.y && a->front.z == a->position.z){
        a->args[3] = 0;
        return;
    }
    if(a->type == OBJ_COIN) a->args[0] = 0;
    v3 start = a->position;
    RayHit ray;
    bool crossed = collision_raycast(&candidates, from, to, &ray, 0);
    v3 reaction = V3(0, 0, 0), ground = V3(0, 0, 0), wall = V3(0, 0, 0), roof = V3(0, 0, 0);
    int mask = 0;
    if(crossed){
        v3 resolved = v3_add(ray.point, v3_scale14(ray.normal, radius));
        reaction = v3_sub(resolved, to);
        to = resolved;
        int32_t d = v3_dot14(ray.normal, a->up);
        mask = d > R14(.5f) ? DROP_GROUND : d < -R14(.5f) ? DROP_ROOF : DROP_WALL;
        if(mask == DROP_GROUND){
            ground = ray.normal;
            if(a->type == OBJ_COIN && (ray.attr & COL_BURN)) mask |= DROP_BURN;
        } else if(mask == DROP_WALL) wall = ray.normal;
        else roof = ray.normal;
    }
    for(int i = 0; i < candidates.count; i++){
        SphereContact hit;
        if(!collectible_sphere_triangle(candidates.index[i], to, radius, &hit)) continue;
        if(hit.depth < 0) continue;
        v3 fix = v3_scale14(hit.normal, hit.depth + 1);
        to = v3_add(to, fix);
        reaction = v3_add(reaction, fix);
        int32_t direction = v3_dot14(hit.normal, a->up);
        if(direction > R14(.5f)){
            mask |= DROP_GROUND;
            ground = hit.normal;
            if(a->type == OBJ_COIN && (hit.attr & COL_BURN)) mask |= DROP_BURN;
        } else if(direction < -R14(.5f)){
            mask |= DROP_ROOF;
            roof = hit.normal;
        } else {
            mask |= DROP_WALL;
            wall = hit.normal;
        }
    }
    a->position = v3_sub(to, off);
    if(a->type == OBJ_COIN){
        a->target = wall;
        a->front = roof;
    } else a->target = mask ? v3_direction14(reaction, a->up) : V3(0, 0, 0);
    a->home = ground;
    a->value = (int16_t)mask;
    if(resting && mask == DROP_GROUND && a->position.x == start.x && a->position.y == start.y &&
       a->position.z == start.z){
        a->args[0] = 1;
        a->front = start;
        a->target = V3((int32_t)g_col_parts_hidden, 0, 0);
    }
    if(mask){
        if(reaction.x == 0 && reaction.y == 0 && reaction.z != 0) a->args[3] = 0;
        else if(reaction.x == 0 && reaction.z == 0 && reaction.y != 0) a->args[2] = 0;
        else if(reaction.y == 0 && reaction.z == 0 && reaction.x != 0) a->args[1] = 0;
        else {
            v3 fraction = V3(a->args[1], a->args[2], a->args[3]);
            fraction = v3_reject14(fraction, v3_direction14(reaction, a->up));
            a->args[1] = (int16_t)fraction.x;
            a->args[2] = (int16_t)fraction.y;
            a->args[3] = (int16_t)fraction.z;
        }
    }
}

static bool collectible_in_death(v3 pos){
    const Stage* st = &g_game.stage;
    uint32_t n = st->header->area_count;
    const uint16_t* list =
        st->header->area_grid ? bound_grid_list(st->header, st->header->area_grid, pos, &n) : NULL;
    for(uint32_t i = 0; i < n; i++){
        const StageArea* ar = &st->areas[list ? list[i] : i];
        if(ar->cls == AREA_DEATH && (ar->scenarios & gravity_scenario_bit()) && ar->args[0] <= 0 &&
           gcam_area_contains(ar, pos))
            return true;
    }
    return false;
}
__attribute__((noinline)) static void collectible_drop_tick(Actor* a){
    if(!collectible_is_pop(a)) return;
    bool bit = a->type == OBJ_STARBIT;

    if(a->flags & DROP_NO_FALL){
        if(bit && a->timer == 1) a->args[0] = 0;
        if(bit && !cutscene_blocks(BLOCK_COLLECTIBLES)) a->args[0]++;
        if(bit ? a->args[0] > 600 : a->timer >= 600) collectible_dead(a);
        return;
    }
    if(a->state != DROP_MOVE){
        if(!(a->flags & DROP_CUSTOM) && !collectible_fall_enabled(a->value > 0 ? a->value : 1)){
            a->flags |= DROP_NO_FALL;
            a->args[0] = 0;
            if(bit && !cutscene_blocks(BLOCK_COLLECTIBLES)) a->args[0]++;
            return;
        }

        int range = (a->flags & DROP_CUSTOM) ? a->health : bit ? V16(10) : a->value > 1 ? V16(4) : 0;
        int speed = (a->flags & DROP_CUSTOM) ? a->args[0] : bit ? V16(40) : V16(25);
        uint32_t seed = g_game.tick ^ ((uint32_t)actor_index(a) << 16) ^
                        (uint32_t)a->position.x * 2654435761u ^ (uint32_t)a->position.y;
        a->velocity = v3_scale14(a->up, speed);
        if(range)
            a->velocity = v3_add(a->velocity, V3(collectible_random_velocity(&seed, range),
                                                 collectible_random_velocity(&seed, range),
                                                 collectible_random_velocity(&seed, range)));
        if(water_query(a->position, a->up, NULL)){
            a->flags |= DROP_WATER;
            if(bit) a->velocity = v3_scale14(a->velocity, R14(.5f));
        }
        a->state = DROP_MOVE;
        a->timer = 0;
        a->value = 0;
        a->target = a->home = V3(0, 0, 0);
        a->args[0] = a->args[1] = a->args[2] = a->args[3] = 0;
    }

    if(bit){
        if(!cutscene_blocks(BLOCK_COLLECTIBLES)) a->args[0]++;
    }
    if(bit ? a->args[0] > 600 : a->timer >= 600){
        collectible_dead(a);
        return;
    }
    if(!bit && ((a->timer > 30 && (a->value & DROP_BURN)) || collectible_in_death(a->position))){
        collectible_dead(a);
        return;
    }
    GravitySample gravity = gravity_sample(g_game.stage.header, a->position);
    if(gravity.index >= 0){
        a->up = gravity.up;
        if(bit){
            bool unchanged = false;
            if(a->up.x == 0 && a->up.y == 0 && a->up.z == N14_ONE && a->front.z == 0){
                int m = fx_max(fx_abs(a->front.x), fx_abs(a->front.y));
                if(m <= N14_ONE){
                    unsigned scale = m < N14_ONE ? 2 : 1, unit = N14_ONE * scale;
                    uint32_t length2 =
                        (uint32_t)(a->front.x * a->front.x + a->front.y * a->front.y) * scale * scale;
                    unchanged = length2 >= unit * unit && length2 < (unit + 1) * (unit + 1);
                }
            }
            if(!unchanged) a->front = v3_tangent14(a->front, a->up, V3(N14_ONE, 0, 0));
        }
    }
    bool water = (a->flags & DROP_WATER) != 0;
    int32_t acceleration = bit ? (water ? V16(.1f) : V16(1)) : (water ? V16(.3f) : V16(1));
    if(!bit && gravity.index < 0) acceleration = 0;
    v3 v = a->velocity;
    if(bit){
        v = v3_sub(v3_scale14(v, R14(.97f)), v3_scale14(a->up, acceleration));
        if(a->value && v3_dot14(v, a->target) < 0){
            v = collectible_rebound(v, a->target, R14(.99f), N14_ONE);
            if(a->value == DROP_GROUND) v = v3_scale14(a->up, v3_dot14(v, a->up));
            int32_t minimum = water ? V16(10) : V16(20);
            if(v3_length_squared64(v) < (uint64_t)((int64_t)minimum * minimum))
                v = v3_scale14(v3_direction14(v, a->up), minimum);
        }
    } else {
        if(a->value & DROP_ROOF) v = collectible_rebound(v, a->front, R14(.6f), R14(.5f));
        if(a->value & DROP_WALL) v = collectible_rebound(v, a->target, R14(.6f), R14(.5f));
        if(a->value & DROP_GROUND){
            int rebound = v3_dot14(v, a->home) <= -V16(2) ? R14(.75f) : 0;
            v = collectible_rebound(v, a->up, rebound, R14(.5f));
        }
        if((a->value & (DROP_GROUND | DROP_ROOF)) == (DROP_GROUND | DROP_ROOF)) a->flags |= DROP_SQUEEZED;
        if(a->flags & DROP_SQUEEZED){
            a->velocity = V3(0, 0, 0);
            return;
        }
        v = v3_sub(v, v3_scale14(a->up, acceleration));
        v = v3_scale14(v, water ? R14(.8f) : (a->value & DROP_GROUND) ? R14(.9f) : R14(.995f));
    }
    a->velocity = v;
    v3 motion = V3(0, 0, 0);
    for(int i = 0; i < 3; i++){
        int32_t part = (&v.x)[i] + a->args[i + 1];
        (&motion.x)[i] = part >> 8;
        a->args[i + 1] = (int16_t)(part & 255);
    }
    collectible_collide(a, motion, bit ? 40 : 55, bit ? 0 : 70);
}

static void drop_coin_figure(const Actor* a, Figure* f){
    if(!collectible_is_drop(a)) return;
    f->basis_front = a->up.x == 0 && a->up.y == 0 && a->up.z == N14_ONE
                         ? V3(0, N14_ONE, 0)
                         : v3_tangent14(V3(0, N14_ONE, 0), a->up, V3(N14_ONE, 0, 0));
}
