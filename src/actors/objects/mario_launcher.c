enum { SD_WAIT = 64, SD_MOVE, SD_DONE };
#if defined(__GNW__)
/* Temporary probes for the first GWemu HardFault; remove after diagnosis. */
volatile uint32_t g_smg_launcher_probe_action;
volatile uint32_t g_smg_launcher_probe_count;
volatile uint32_t g_smg_launcher_probe_index;
volatile uint32_t g_smg_launcher_probe_actor;
#endif

static Actor* mario_launcher_owner(void){
#if defined(__GNW__)
    g_smg_launcher_probe_action = (uint32_t)g_game.player.action;
    g_smg_launcher_probe_count = (uint32_t)g_game.actor_count;
    g_smg_launcher_probe_index = UINT32_MAX;
    g_smg_launcher_probe_actor = 0;
#endif
    if(g_game.player.action != MA_LAUNCHER_BIND) return NULL;
    for(int i = 0; i < g_game.actor_count; i++){
        Actor* a = &g_game.actors[i];
#if defined(__GNW__)
        g_smg_launcher_probe_index = (uint32_t)i;
        g_smg_launcher_probe_actor = (uint32_t)(uintptr_t)a;
#endif
        if(a->active && a->type == OBJ_MARIOLAUNCHER && a->state != ML_WAIT && a->state != ML_LAND) return a;
    }
    return NULL;
}
static bool mario_launcher_bound(void){ return mario_launcher_owner() != NULL; }
static bool mario_launcher_aiming(void){
    Actor* a = mario_launcher_owner();
    return a && a->state == ML_AIM;
}
static v3 launcher_local(const Actor* a, v3 local){
    return v3_add(v3_add(v3_scale14(v3_cross14(a->up, a->front), local.x), v3_scale14(a->up, local.y)),
                  v3_scale14(a->front, local.z));
}
static void launcher_sound(int cue){
    if(cue >= 0) audio_play_effect(cue, 2);
}
static void launcher_event_sound(int event){
    static const int16_t cues[] = {
        SFX_MARIO_LAUNCHER_PREP, SFX_MARIO_LAUNCHER_CATCH,  SFX_MARIO_LAUNCHER_IN,
        SFX_MARIO_LAUNCHER_BEND, SFX_MARIO_LAUNCHER_AIM_IN, SFX_MARIO_LAUNCHER_SHOOT,
        SFX_MARIO_LAUNCHER_JOY,  SFX_MARIO_LAUNCHER_REJECT, SFX_MARIO_LAUNCHER_REJECT_JUMP};
    if((unsigned)event < sizeof cues / sizeof *cues) launcher_sound(cues[event]);
}
static void shutter_place(Actor* a){
    int frame = a->state == SD_WAIT   ? -1
                : a->state == SD_DONE ? SHUTTERDOORE_MOVE_FRAMES
                                      : fx_min(a->timer - 1, SHUTTERDOORE_MOVE_FRAMES);
    v3 local = frame < 0 ? V3(0, -1, 0) : v3_from_i16(shutterdoore_move[frame][0].pos);
    int scale = OBJECT_SCALE_PERCENT(&g_game.stage.objects[a->stage_index]) * N14_ONE / 100;
    a->position = v3_add(a->home, v3_scale14(launcher_local(a, local), scale));
    MoverFrame f = mover_frame_of(a->position, a->up, a->front, scale);
    int set = mappart_set(a, kcl_shutterdoore, sizeof kcl_shutterdoore / sizeof *kcl_shutterdoore);
    if(set < 0) return;
    movers_place(set, &f);
    for(int i = 0; i < g_col_dyn_sets[set].count; i++){
        CollisionTriangle* t = &g_col_dyn[g_col_dyn_sets[set].first + i];
        t->floor_code = shutterdoore_pa[i][0];
        t->wall_code = shutterdoore_pa[i][1];
    }
    movers_enable(set, true);
}
static void mario_launcher_gate_init(Actor* a, const StageObject* o){
    a->value = -1;
    a->args[0] = a->args[1] = 0;
    a->health = 0;
    int scale = 64 * OBJECT_SCALE_PERCENT(o) / 100;
    a->figure.scale = V3(scale, scale, scale);
    if(a->type == OBJ_SHUTTERDOORE){
        actor_set_state(a, stream_part_state(a->stage_index) ? SD_DONE : SD_WAIT, -1);
        shutter_place(a);
    } else {
        actor_set_state(a, ML_WAIT, -1);
        int set = mappart_set(a, kcl_mariolauncher, sizeof kcl_mariolauncher / sizeof *kcl_mariolauncher);
        MoverFrame f = mover_frame_of(a->home, a->up, a->front, N14_ONE);
        movers_place(set, &f);
        if(set >= 0)
            for(int i = 0; i < g_col_dyn_sets[set].count; i++){
                CollisionTriangle* t = &g_col_dyn[g_col_dyn_sets[set].first + i];
                t->floor_code = mariolauncher_pa[i][0];
                t->wall_code = mariolauncher_pa[i][1];
            }
        movers_enable(set, true);
    }
}
static void shutter_think(Actor* a){
    if(a->state == SD_WAIT){
        int key = actor_switch_key(a, SWITCH_A);
        if(key < 0 || switch_on(key)){
            actor_set_state(a, SD_MOVE, -1);
            launcher_sound(SFX_SHUTTER_E_START);
        }
    } else if(a->state == SD_MOVE){
        if(a->timer - 1 == 173){
            launcher_sound(SFX_SHUTTER_E_END_RIDDLE);
        }
        if(a->timer - 1 >= SHUTTERDOORE_MOVE_FRAMES){
            actor_set_state(a, SD_DONE, -1);
            stream_part_state_set(a->stage_index, 1);
        }
    }
    shutter_place(a);
}
static v3 launcher_to_q8(v3 p){ return V3(p.x * 256, p.y * 256, p.z * 256); }
static v3 launcher_from_q8(v3 p){ return V3(p.x >> 8, p.y >> 8, p.z >> 8); }
static void launcher_player_position(v3 at, v3 up, v3 front){
    Player* p = &g_game.player;
    p->figure.position = at;
    p->up = up;
    p->front = front;
    g_mario.position16 = launcher_to_q8(at);
    g_mario.jump_vec = V3(0, 0, 0);
    mario_stop_walk();
    p->on_ground = false;
}
static v3 launcher_target(const Actor* a){
    return v3_add(a->home, v3_add(v3_scale14(a->up, 150), v3_scale14(a->front, 200)));
}
static void launcher_release(Actor* a, v3 rebound){
    Player* p = &g_game.player;
    mario_set_hidden(false);
    int key = actor_switch_key(a, SWITCH_B);
    if(key >= 0) switch_set(key, false);
    a->flags &= ~ACTOR_HIDDEN;
    mario_set_action(MA_FALL);
    mario_update_gravity();
    g_mario.jump_vec = launcher_to_q8(rebound);
    g_mario.jumping = true;
    p->on_ground = false;
    p->up = p->gravity_up;
    p->front = v3_tangent14(a->front, p->up, p->front);
    if(a->state != ML_REJECT) actor_set_state(a, ML_LAND, -1);
}

__attribute__((noinline)) static bool launcher_flight_move(Actor* a, v3* rebound){
    v3 old = a->target, next = v3_add(old, a->velocity);
    v3 direction = v3_direction14(a->velocity, a->front),
       from = v3_add(launcher_from_q8(old), v3_scale14(direction, 180)),
       to = v3_add(launcher_from_q8(next), v3_scale14(direction, 180));
    CollisionCandidates candidates;
    collision_gather_around(from, to, 80, &candidates);
    bool collided = false;
    int set = mappart_set(a, kcl_mariolauncher, sizeof kcl_mariolauncher / sizeof *kcl_mariolauncher);
    for(int pass = 0; pass < 2; pass++)
        for(int i = 0; i < candidates.count; i++){
            int tri = candidates.index[i];
            if(set >= 0 && tri >= COL_DYN_BASE + g_col_dyn_sets[set].first &&
               tri < COL_DYN_BASE + g_col_dyn_sets[set].first + g_col_dyn_sets[set].count)
                continue;
            SphereContact contact;
            if(!collision_sphere_triangle(tri, to, 80, &contact)) continue;
            v3 displacement = v3_scale14(contact.normal, contact.depth);
            to = v3_add(to, displacement);
            next = v3_add(next, launcher_to_q8(displacement));
            collided = true;
        }
    *rebound = launcher_from_q8(v3_sub(next, old));
    a->target = next;
    launcher_player_position(launcher_from_q8(next), direction,
                             v3_tangent14(v3_neg(a->up), direction, a->front));
    return collided || water_query(g_game.player.figure.position, g_game.player.gravity_up, NULL);
}
static void mario_launcher_think(Actor* a){
    Player* p = &g_game.player;
    int step = a->timer - 1;
    if(a->state == ML_WAIT){
        int key = actor_switch_key(a, SWITCH_A);
        if(key >= 0 && !switch_on(key)) return;
        if(p->action == MA_DEATH || p->action == MA_STAR_GET || p->action == MA_STAR_LAUNCH ||
           p->action == MA_TALK || mario_launcher_bound())
            return;
        v3 sensor = v3_add(a->home, v3_scale14(a->up, 20));
        if(v3_length_squared64(v3_sub(sensor, mario_centre())) <=
           (80u + C_BODY_RADIUS) * (80u + C_BODY_RADIUS)){
            kamella_damage_drop();
            a->target = p->figure.position;
            mario_set_action(MA_LAUNCHER_BIND);
            mario_restart_anim(ANIM_MARIO_EARTHENPIPEREADY);
            actor_set_state(a, ML_READY, -1);
            launcher_event_sound(0);
        }
        return;
    }
    if(a->state == ML_LAND){
        if(step >= 1) actor_set_state(a, ML_WAIT, -1);
        return;
    }
    if(a->state == ML_REJECT){
        if(step == 18) mario_set_hidden(false);
        if(step == 20 && !a->health){
            launcher_player_position(v3_add(a->home, v3_scale14(a->up, 150)), a->up, a->front);
            launcher_release(a, v3_sub(v3_scale14(a->up, 10), v3_scale14(a->front, 10)));
            launcher_event_sound(8);
            a->health = 1;
        }
        if(a->health && step >= 30) actor_set_state(a, ML_WAIT, -1);
        return;
    }
    if(p->action != MA_LAUNCHER_BIND){
        a->flags &= ~ACTOR_HIDDEN;
        mario_set_hidden(false);
        actor_set_state(a, ML_LAND, -1);
        return;
    }
    if(a->state == ML_READY){
        int t = fx_min(step, 24);
        v3 at = v3_add(a->target, v3_scale14(v3_sub(a->home, a->target), t * N14_ONE / 24));
        launcher_player_position(at, a->up, a->front);
        if(step >= 24){
            mario_restart_anim(ANIM_MARIO_EARTHENPIPEIN);
            p->anim_speed = 4 * FX_ONE;
            actor_set_state(a, ML_PLAYERIN, -1);
            launcher_event_sound(1);
        }
    } else if(a->state == ML_PLAYERIN){
        launcher_player_position(a->home, a->up, a->front);
        if(step >= 19){
            mario_set_hidden(true);
            p->anim_speed = FX_ONE;
            actor_set_state(a, ML_PREP, -1);
        }
    } else if(a->state == ML_PREP){
        if(step == 10) launcher_event_sound(2);
        if(step == 26) launcher_event_sound(3);
        if(step >= 10 + MARIOLAUNCHER_ACTIVE_FRAMES){
            a->flags |= ACTOR_HIDDEN;
            actor_set_state(a, ML_AIM, -1);
            launcher_event_sound(4);
            int key = actor_switch_key(a, SWITCH_B);
            if(key >= 0) switch_set(key, true);
        }
    } else if(a->state == ML_AIM){
        launcher_player_position(launcher_target(a), a->up, a->front);
        if(pointer_pressed()){
            v3 origin, direction;
            pointer_world_ray(&origin, &direction);
            a->target = launcher_to_q8(launcher_target(a));
            a->velocity = v3_scale14(direction, 40 * 256);
            mario_set_hidden(false);
            mario_restart_anim(ANIM_MARIO_CANNONFLYSTART);
            actor_set_state(a, ML_FLIGHT, -1);
            camera_shake(3);
            launcher_event_sound(5);
        } else if(pointer_shoot_pressed()){
            int key = actor_switch_key(a, SWITCH_B);
            if(key >= 0) switch_set(key, false);
            a->flags &= ~ACTOR_HIDDEN;
            a->health = 0;
            actor_set_state(a, ML_REJECT, -1);
            launcher_event_sound(7);
        }
    } else if(a->state == ML_FLIGHT){
        if(step == 65){
            mario_restart_anim(ANIM_MARIO_CANNONFLYLOOP);
            launcher_event_sound(6);
        }
        if(step == 90) a->flags &= ~ACTOR_HIDDEN;
        if(step == 300) mario_restart_anim(ANIM_MARIO_DIECANNONFLY);
        if(step >= 360){
            p->health = 0;
            mario_set_action(MA_DEATH);
            mario_restart_anim(ANIM_MARIO_DIEFALL);
            actor_set_state(a, ML_KILL, -1);
            return;
        }
        v3 rebound;
        if(launcher_flight_move(a, &rebound)) launcher_release(a, v3_scale14(rebound, R14(.4f)));
    }
}

static bool mario_launcher_camera(fx dt, const KeyState* keys){
    Actor* a = mario_launcher_owner();
    if(!a || (a->state != ML_AIM && a->state != ML_FLIGHT)) return false;
    Camera* c = &g_game.camera;
    v3 target, up, front;
    if(a->state == ML_AIM){
        int x, y;
        pointer_screen(&x, &y);
        fx yaw = (x - 120) * FXC(.8f) / 120, pitch = FXC(.62f) - (y - 80) * FXC(.3f) / 80;
        front = v3_rotate_axis14(a->front, a->up, -yaw);
        v3 side = v3_cross14(a->up, front);
        front = v3_rotate_axis14(front, side, -pitch);
        target = launcher_target(a);
        c->position = v3_add(target, v3_scale14(a->up, 120));
        up = v3_tangent14(a->up, front, a->up);
        c->target = v3_add(c->position, v3_scale14(front, 1000));
    } else {
        target = v3_add(g_game.player.figure.position, v3_scale14(g_game.player.up, 100));
        v3 offset = v3_rotate_axis14(g_game.player.front, g_game.player.up, FX_PI),
           side = v3_cross14(g_game.player.up, offset);
        offset = v3_rotate_axis14(offset, side, FXC(1.4f));
        c->position = v3_add(target, v3_scale14(offset, 900));
        front = v3_direction14(v3_sub(target, c->position), g_game.player.front);
        up = v3_tangent14(g_game.player.up, front, g_game.player.up);
        c->target = target;
    }
    c->front = front;
    c->up = up;
    c->smooth_up = up;
    c->heading = front;
    c->distance = 900;
    return true;
}

__attribute__((noinline)) static void launcher_prepare_figure(
    Figure* part, const Actor* a, const uint8_t* model, const LauncherJointPose* pose){
    part->model = (const FigureHeader*)model;
    part->pose = model_rest_pose(part->model);
    int scale = OBJECT_SCALE_PERCENT(&g_game.stage.objects[a->stage_index]) * N14_ONE / 100;
    part->position = v3_add(a->home, v3_scale14(launcher_local(a, v3_from_i16(pose->pos)), scale));
    part->basis_up = launcher_local(a, v3_from_i16(pose->up));
    part->basis_front = launcher_local(a, v3_from_i16(pose->front));
    for(int k = 0; k < 3; k++)
        ((int32_t*)&part->scale)[k] = (int32_t)((int64_t)64 * scale * pose->scale[k] >> 28);
    part->transform_dirty = true;
    figure_update_transform(part);
}
__attribute__((noinline)) static bool mario_launcher_gate_draw(
    uint8_t* framebuffer, Figure* figure, const CameraView* cv){
    bool cannon = figure->model == (const FigureHeader*)mariolauncher_models[0],
         door = figure->model == (const FigureHeader*)shutterdoore_models[0];
    if(!cannon && !door) return false;
    const Actor* a = (const Actor*)((const uint8_t*)figure - __builtin_offsetof(Actor, figure));
    const uint8_t* const* models = cannon ? mariolauncher_models : shutterdoore_models;
    const uint8_t* joints = cannon ? mariolauncher_joints : shutterdoore_joints;
    int count = cannon ? sizeof mariolauncher_joints : sizeof shutterdoore_joints;
    const LauncherJointPose* poses;
    if(cannon){
        int frame = a->state == ML_PREP ? fx_clamp(a->timer - 11, 0, MARIOLAUNCHER_ACTIVE_FRAMES)
                    : a->state >= ML_AIM && a->state <= ML_FLIGHT ? MARIOLAUNCHER_ACTIVE_FRAMES
                                                                  : 0;
        poses = frame ? mariolauncher_active[frame] : mariolauncher_wait[0];
    } else {
        int frame = a->state == SD_WAIT   ? 0
                    : a->state == SD_DONE ? SHUTTERDOORE_MOVE_FRAMES
                                          : fx_min(a->timer - 1, SHUTTERDOORE_MOVE_FRAMES);
        poses = shutterdoore_move[frame];
    }
    Figure* part = figure;
    const FigureHeader* saved_model = part->model;
    const void* saved_pose = part->pose;
    v3 saved_position = part->position, saved_scale = part->scale, saved_up = part->basis_up,
       saved_front = part->basis_front;
    for(int i = 0; i < count; i++){
        launcher_prepare_figure(part, a, models[i], &poses[joints[i]]);
        draw_figure(framebuffer, part, cv);
    }
    part->model = saved_model;
    part->pose = saved_pose;
    part->position = saved_position;
    part->scale = saved_scale;
    part->basis_up = saved_up;
    part->basis_front = saved_front;
    part->transform_dirty = true;
    figure_update_transform(part);
    return true;
}

/* All three shutter materials use joint0 and the same transform. Its centre
 * can leave the view while a panel still spans the screen. Bound every part
 * before allowing the scene's centre-only cull to discard the whole gate. */
static fx shutter_gate_radius(const Figure* figure){
    uint32_t extent = 0;
    for(unsigned i = 0; i < sizeof shutterdoore_models / sizeof *shutterdoore_models; i++){
        const FigureHeader* part = (const FigureHeader*)shutterdoore_models[i];
        if(part->extent > extent) extent = part->extent;
    }
    fx scale = fx_max(fx_abs(figure->scale.x),
                      fx_max(fx_abs(figure->scale.y), fx_abs(figure->scale.z)));
    /* extent bounds each model axis. 7/4 exceeds sqrt(3); round up and keep
     * a small margin for the fixed-point basis transform. */
    return (fx)(((int64_t)extent * scale * 7 + 1023) >> 10) + FX(4);
}

static fx shutter_gate_draw_extent(const Actor* actor){
    return shutter_gate_radius(&actor->figure);
}

__attribute__((noinline)) static bool shutter_gate_view_bounds(
    const Figure* figure, const CameraView* view){
    if(figure->model != (const FigureHeader*)shutterdoore_models[0]) return false;
    return view_sphere_visible(view, figure->position, shutter_gate_radius(figure));
}

static const FigureOps mario_launcher_figure_ops = {
    .draw = mario_launcher_gate_draw,
    .view_bounds = shutter_gate_view_bounds,
};
