static bool chip_group_has_chip(const StageObject* y, const StageObject* o, uint32_t c, uint8_t bit){
    bool shell = y->type == OBJ_SHELLFISH && y->args[3] == 1;
    if((y->type != OBJ_YELLOWCHIP && !shell) || y->zone != o->zone || y->args[0] != o->args[0]) return false;
    if(y->scenarios != 0xFF && !(y->scenarios & bit)) return false;
    return shell ? stream_part_state(c) == 0 : !(g_object_state[c] & (OBJECT_STATE_GONE | OBJECT_STATE_DEAD));
}
static EWRAM_BSS struct {
    uint8_t stage;
    uint16_t group, chip;
} g_chip_hint[16];

static EWRAM_BSS struct {
    const StageObject* objects;
    uint16_t count, index[14];
} g_switch_objects;
static inline bool switch_object(const StageObject* o){
    return o->type == OBJ_GROUPSWITCHWATCHER || o->type == OBJ_SWITCHSYNCREV ||
           o->type == OBJ_YELLOWCHIPGROUP;
}
/* Candidate membership is immutable stage data; scenario and object/switch
 * state are still checked each tick. Preserve source order for dependencies. */
static bool switch_objects_prepare(const Stage* st){
    uint32_t total = st->header->object_count;
    if(g_switch_objects.objects != st->objects){
        uint32_t n = 0;
        for(uint32_t i = 0; i < total; i++)
            if(switch_object(&st->objects[i])){
                if(n < sizeof g_switch_objects.index / sizeof g_switch_objects.index[0])
                    g_switch_objects.index[n] = (uint16_t)i;
                n++;
            }
        g_switch_objects.objects = st->objects;
        g_switch_objects.count = (uint16_t)n;
    }
    return g_switch_objects.count <= sizeof g_switch_objects.index / sizeof g_switch_objects.index[0];
}
static void switch_objects_tick(void){
    const Stage* st = &g_game.stage;
    uint8_t bit = g_stream.scenario_bit;
    uint32_t total = st->header->object_count;
    bool listed = switch_objects_prepare(st);
    for(uint32_t j = 0; j < (listed ? g_switch_objects.count : total); j++){
        uint32_t i = listed ? g_switch_objects.index[j] : j;
        const StageObject* o = &st->objects[i];
        if(!switch_object(o)) continue;
        if((o->scenarios != 0xFF && !(o->scenarios & bit)) || (g_object_state[i] & OBJECT_STATE_GONE))
            continue;
        if(o->type == OBJ_SWITCHSYNCREV){
            if(o->sw[SWITCH_A] >= 0 && o->sw[SWITCH_B] >= 0)
                switch_set(o->sw[SWITCH_A], !switch_on(o->sw[SWITCH_B]));
        } else if(o->type == OBJ_GROUPSWITCHWATCHER){
            bool result = group_switch_fires(o->sw[SWITCH_B], o->args[0], o->args[1]);
            if(o->args[2] == -1){
                if(result){
                    switch_set(o->sw[SWITCH_A], true);
                    g_object_state[i] |= OBJECT_STATE_GONE;
                }
            } else {
                switch_set(o->sw[SWITCH_A], result);
            }
        } else {
            int left = 0;
            unsigned h = i % 16;
            if(g_chip_hint[h].stage == g_game.stage_index + 1 && g_chip_hint[h].group == i){
                uint32_t c = g_chip_hint[h].chip;
                const StageObject* y = &st->objects[c];
                if(c < st->header->object_count && chip_group_has_chip(y, o, c, bit)) left = 1;
            }
            for(uint32_t c = 0; c < st->header->object_count && !left; c++)
                if(chip_group_has_chip(&st->objects[c], o, c, bit)){
                    left++;
                    g_chip_hint[h].stage = (uint8_t)(g_game.stage_index + 1);
                    g_chip_hint[h].group = (uint16_t)i;
                    g_chip_hint[h].chip = (uint16_t)c;
                }
            if(!left){
                switch_set(o->sw[SWITCH_A], true);
                g_object_state[i] |= OBJECT_STATE_GONE;
            }
        }
    }
}

static void group_switch_objects_tick(void){
    const Stage* stage = &g_game.stage;
    bool listed = switch_objects_prepare(stage);
    uint32_t total = listed ? g_switch_objects.count : stage->header->object_count;
    for(uint32_t j = 0; j < total; j++){
        uint32_t i = listed ? g_switch_objects.index[j] : j;
        const StageObject* o = &stage->objects[i];
        if(o->type != OBJ_GROUPSWITCHWATCHER || (g_object_state[i] & OBJECT_STATE_GONE)) continue;
        if(o->scenarios != 0xFF && !(o->scenarios & g_stream.scenario_bit)) continue;
        int output = o->sw[SWITCH_A], first = o->sw[SWITCH_B];
        if(output < 0 || first < 0) continue;
        if(o->args[2] == -1 && switch_on(output)) continue;
        bool activate = group_switch_fires(first, o->args[0], o->args[1]);
        if(o->args[2] == -1){
            if(activate){
                switch_set(output, true);
                g_object_state[i] |= OBJECT_STATE_GONE;
            }
        } else switch_set(output, activate);
    }
}
