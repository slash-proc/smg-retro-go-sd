#if defined(__GNW__)
static void audio_clear_effect_queue(void){}
static void audio_init(void){}
static void audio_play_music(int track){ (void)track; }
static void audio_stop_music(void){}
static void audio_play_sub_music(int track, int lead_in){ (void)track; (void)lead_in; }
static void audio_stop_sub_music(void){}
static void audio_play_stage_music(int track){ (void)track; }
static void audio_play_effect(int effect, int priority){ (void)effect; (void)priority; }
static void audio_play_effect_soft(int effect, int priority){ (void)effect; (void)priority; }
static void audio_queue_effect(int effect, int priority){ (void)effect; (void)priority; }
static void audio_stop_effects(void){}
static void audio_level_effect(int effect){ (void)effect; }
static void audio_stop_level_effect(int effect){ (void)effect; }
void audio_vblank(void){}
#else
#define REG_SOUNDCNT_L REG16(0x04000080)
#define REG_SOUNDCNT_H REG16(0x04000082)
#define REG_SOUNDCNT_X REG16(0x04000084)
#define REG_FIFO_A     0x040000A0
#define REG_FIFO_B     0x040000A4
#define REG_DMA1SAD    REG32(0x040000BC)
#define REG_DMA1DAD    REG32(0x040000C0)
#define REG_DMA1CNT_H  REG16(0x040000C6)
#define REG_DMA2SAD    REG32(0x040000C8)
#define REG_DMA2DAD    REG32(0x040000CC)
#define REG_DMA2CNT_H  REG16(0x040000D2)
#define REG_TM0CNT_L   REG16(0x04000100)
#define REG_TM0CNT_H   REG16(0x04000102)
#define REG_TM1CNT_L   REG16(0x04000104)
#define REG_TM1CNT_H   REG16(0x04000106)

#define AUDIO_MUSIC_TIMER_PERIOD  1463
#define AUDIO_EFFECT_TIMER_PERIOD 1232
_Static_assert(AUDIO_MUSIC_TIMER_PERIOD * SOUND_MUSIC_SAMPLES_PER_REFRESH == 280896,
               "music timer must match the frame length");
_Static_assert(AUDIO_EFFECT_TIMER_PERIOD * SOUND_EFFECT_SAMPLES_PER_REFRESH == 280896,
               "effect timer must match the frame length");

#define DMA_SOUND      (0x8000 | 0x2000 | 0x1000 | 0x0400 | 0x0200 | 0x0040)
#define AUDIO_IRQ_DMA1 0x0200
#define AUDIO_IRQ_DMA2 0x0400

#define AUDIO_MUSIC_BUFFER  256
#define AUDIO_EFFECT_BUFFER 288
_Static_assert(AUDIO_MUSIC_BUFFER % SOUND_BLOCK == 0 && AUDIO_MUSIC_BUFFER % 16 == 0,
               "music buffer must be a multiple of the block size and 16");
_Static_assert(AUDIO_EFFECT_BUFFER % SOUND_BLOCK == 0 && AUDIO_EFFECT_BUFFER % 16 == 0,
               "effect buffer must be a multiple of the block size and 16");

#define SOUNDCNT_A_FULL   0x0004
#define SOUNDCNT_B_FULL   0x0008
#define SOUNDCNT_A_BOTH   0x0300
#define SOUNDCNT_A_TIMER0 0x0000
#define SOUNDCNT_A_RESET  0x0800
#define SOUNDCNT_B_BOTH   0x3000
#define SOUNDCNT_B_TIMER1 0x4000
#define SOUNDCNT_B_RESET  0x8000

volatile AudioState g_audio EWRAM_BSS;

uint32_t g_vblank_stack[128] EWRAM_BSS __attribute__((used, aligned(8)));
_Static_assert(sizeof(AudioState) == 44, "AudioState must be 44 bytes");
_Static_assert(offsetof(AudioState, music_dma_position) == 36, "startup.s reads music_dma_position at 36");
_Static_assert(offsetof(AudioState, effect_dma_position) == 42, "startup.s reads effect_dma_position at 42");

#define AUDIO_EFFECT_NONE UINT16_MAX
_Static_assert(MUSIC_COUNT <= 127, "music IDs and stop sentinel must fit int8_t");
_Static_assert(SFX_COUNT < UINT16_MAX, "effect IDs and stop sentinel must fit uint16_t");
static const SoundBlob* const music_tracks[MUSIC_COUNT] = MUSIC_BLOBS;
static const SoundBlob* const sound_effects[SFX_COUNT] = SFX_BLOBS;

#define AUDIO_EFFECT_QUEUE_SIZE 4
typedef struct {
    struct {
        uint16_t id;
        uint8_t priority;
    } entry[AUDIO_EFFECT_QUEUE_SIZE];
    uint8_t read, write;
    uint16_t overflow;
} EffectQueue;
static volatile EffectQueue g_effect_queue EWRAM_BSS;

static volatile uint16_t g_effect_half_id EWRAM_BSS;

static void audio_clear_effect_queue(void){
    uint16_t enabled = REG_IME;
    REG_IME = 0;
    g_effect_queue.read = g_effect_queue.write;
    REG_IME = enabled;
}

typedef struct {
    const SoundBlob* blob;
    const uint8_t* next;
    const uint8_t* end;
    int32_t s1, s2;
} SoundStream;
static EWRAM_BSS SoundStream g_music_stream, g_effect_stream;
static EWRAM_BSS int8_t g_music_buffer[2][AUDIO_MUSIC_BUFFER] __attribute__((aligned(4)));
static EWRAM_BSS int8_t g_effect_buffer[2][AUDIO_EFFECT_BUFFER] __attribute__((aligned(4)));
static const int32_t sound_filters[16][2] = SOUND_FILTERS;

static void sound_stream_start(SoundStream* st, const SoundBlob* blob){
    st->blob = blob;
    st->next = blob->blocks;
    st->end = blob->blocks + blob->length / SOUND_BLOCK * SOUND_BLOCK_BYTES;
    st->s1 = st->s2 = 0;
}

IWRAM_CODE static void sound_fill(SoundStream* st, int8_t* out, unsigned blocks){
    const uint8_t* p = st->next;
    int32_t s1 = st->s1, s2 = st->s2;
    uint16_t* o = (uint16_t*)(void*)out;
    while(blocks--){
        if(p == st->end){
            const SoundBlob* blob = st->blob;
            if(blob->loop_start >= blob->length){
                for(int i = 0; i < SOUND_BLOCK / 2; i++) *o++ = 0;
                continue;
            }
            p = blob->blocks + blob->loop_start / SOUND_BLOCK * SOUND_BLOCK_BYTES;
            s1 = blob->loop_s1;
            s2 = blob->loop_s2;
        }
        unsigned h = *p++;
        int32_t fa = sound_filters[h >> 4][0], fb = sound_filters[h >> 4][1];
        unsigned rs = 28 - (h & 15);
        for(int i = 0; i < SOUND_BLOCK / 2; i++){
            uint32_t v = *p++;
            int32_t a = ((fa * s1 + fb * s2) >> 8) + ((int32_t)(v << 28) >> rs);
            int32_t b = ((fa * a + fb * s1) >> 8) + ((int32_t)((v & 0xF0) << 24) >> rs);
            *o++ = (uint16_t)(((uint32_t)a >> 8 & 0xFF) | ((uint32_t)b & 0xFF00));
            s2 = a;
            s1 = b;
        }
    }
    st->next = p;
    st->s1 = s1;
    st->s2 = s2;
}

void audio_dma(unsigned channel){
    volatile AudioState* a = &g_audio;
    if(!channel){
        REG_DMA1CNT_H &= (uint16_t)~0x8000u;
        unsigned old = a->music_buffer;
        a->music_buffer = (uint8_t)(old ^ 1u);
        a->music_dirty |= (uint8_t)(1u << old);
        REG_DMA1SAD = (uint32_t)g_music_buffer[old ^ 1u];
        REG_DMA1CNT_H = DMA_SOUND | 0x4000;
        a->music_dma_position = 0;
    } else {
        REG_DMA2CNT_H &= (uint16_t)~0x8000u;
        unsigned old = a->effect_buffer;
        a->effect_buffer = (uint8_t)(old ^ 1u);
        a->effect_dirty |= (uint8_t)(1u << old);
        REG_DMA2SAD = (uint32_t)g_effect_buffer[old ^ 1u];
        REG_DMA2CNT_H = DMA_SOUND | 0x4000;
        a->effect_dma_position = 0;
    }
}

typedef struct {
    int8_t sub_pending;
    int8_t sub_music;
    uint8_t sub_playing;
    uint8_t shadow_playing;
    uint8_t lead_pending;
    uint8_t lead;
    int8_t sub_track;
    uint8_t reserved;
} BgmLayers;
static volatile BgmLayers g_bgm EWRAM_BSS;
static EWRAM_BSS SoundStream g_stage_shadow;

static void audio_init(void){
    g_effect_half_id = AUDIO_EFFECT_NONE;
    audio_clear_effect_queue();
    REG_SOUNDCNT_X = 0x0080;
    REG_SOUNDCNT_L = 0;
    REG_SOUNDCNT_H = SOUNDCNT_A_FULL | SOUNDCNT_B_FULL | SOUNDCNT_A_BOTH | SOUNDCNT_A_TIMER0 |
                     SOUNDCNT_A_RESET | SOUNDCNT_B_BOTH | SOUNDCNT_B_TIMER1 | SOUNDCNT_B_RESET;
    REG_DMA1DAD = REG_FIFO_A;
    REG_DMA2DAD = REG_FIFO_B;
    REG_TM0CNT_H = 0;
    REG_TM0CNT_L = (uint16_t)(65536 - AUDIO_MUSIC_TIMER_PERIOD);
    REG_TM1CNT_H = 0;
    REG_TM1CNT_L = (uint16_t)(65536 - AUDIO_EFFECT_TIMER_PERIOD);
    g_audio.music_pending = -1;
    g_audio.effect_pending = AUDIO_EFFECT_NONE;
    g_audio.current_music = -1;
    g_audio.current_effect = AUDIO_EFFECT_NONE;
    g_bgm.sub_pending = -1;
    g_bgm.sub_music = -1;
    g_bgm.sub_playing = 0;
    g_bgm.shadow_playing = 0;
    g_bgm.lead = 0;
}

static void audio_play_music(int track){
    if(!(g_debug_options & DEBUG_MUSIC)) return;
    if(track == g_audio.current_music) return;
    g_audio.current_music = (int8_t)track;
    g_audio.music_pending = (int8_t)track;
}

static void audio_stop_music(void){
    g_audio.current_music = -1;
    g_audio.music_pending = MUSIC_COUNT;
}

static void audio_play_sub_music(int track, int lead_in){
    if(track < 0 || track >= MUSIC_COUNT) return;
    if(!(g_debug_options & DEBUG_MUSIC)) return;
    g_bgm.sub_music = (int8_t)track;
    g_bgm.lead_pending = (uint8_t)(lead_in < 0 ? 0 : lead_in > 255 ? 255 : lead_in);
    g_bgm.sub_pending = (int8_t)track;
}

static void audio_stop_sub_music(void){
    if(g_bgm.sub_music < 0) return;
    g_bgm.sub_music = -1;
    g_bgm.sub_pending = MUSIC_COUNT;
}

static void audio_play_stage_music(int track){
    if(track >= 0 && track < MUSIC_COUNT) audio_play_music(track);
    else audio_stop_music();
}

static void audio_play_effect(int effect, int priority){
    if(effect < 0 || effect >= SFX_COUNT) return;
    if(!(g_debug_options & DEBUG_SOUND)) return;
    if(g_audio.effect_playing && priority < g_audio.effect_priority) return;
    g_audio.current_effect = (uint16_t)effect;
    g_audio.effect_priority = (uint8_t)priority;
    g_audio.effect_pending = (uint16_t)effect;
}

static void audio_play_effect_soft(int effect, int priority){
    audio_play_effect(effect, priority);
    if(g_audio.effect_pending == (uint16_t)effect) g_effect_half_id = (uint16_t)effect;
}

static void audio_queue_effect(int effect, int priority){
    if(effect != SFX_CRYSTAL_CAGE_S_BREAK && effect != SFX_BEE_FLOWER_BEE_ON &&
       effect != SFX_BEE_FLOWER_DISAPPEAR && effect != SFX_BEE_FLOWER_RECOVER &&
       effect != SFX_BOSS_BUG_EXPLODE && effect != SFX_BOSS_BUG_CLOSE_FEET &&
       effect != SFX_POWER_STAR_APPEAR && effect != SFX_POWER_STAR_MOVE_END &&
       effect != SFX_POWER_STAR_MOVE_END_WATER && effect != SFX_BRIDGE_RIDDLE && effect != SFX_BRIDGE_LAND &&
       effect != SFX_BRIDGE_FALL && effect != SFX_LIFE_PLATE_DEC && effect != SFX_FF_PV_TWIST_START_0 &&
       effect != SFX_FF_PV_TWIST_START_1 && effect != SFX_FF_PV_TWIST_START_2 &&
       effect != SFX_FF_PV_TWIST_START_3 && effect != SFX_FF_PM_SPIN_ATTACK_0 &&
       effect != SFX_FF_OJ_VALVE_SWITCH_ON_0 && effect != SFX_FF_OJ_MTL_SHUTTER_S_ST_0 &&
       effect != SFX_FF_OJ_MTL_SHUTTER_S_ED_0 && effect != SFX_FF_OJ_FENCE_SHUTTER_START_0 &&
       effect != SFX_FF_OJ_FENCE_SHUTTER_STOP_0)
        return;
    if(!(g_debug_options & DEBUG_SOUND)) return;
    unsigned write = g_effect_queue.write;
    if((uint8_t)(write - g_effect_queue.read) >= AUDIO_EFFECT_QUEUE_SIZE){
        if(g_effect_queue.overflow != UINT16_MAX) g_effect_queue.overflow++;
        return;
    }
    unsigned slot = write & (AUDIO_EFFECT_QUEUE_SIZE - 1);
    g_effect_queue.entry[slot].id = (uint16_t)effect;
    g_effect_queue.entry[slot].priority = (uint8_t)priority;
    g_effect_queue.write = (uint8_t)(write + 1);
}

static void audio_stop_effects(void){
    audio_clear_effect_queue();
    g_audio.effect_pending = SFX_COUNT;
    g_audio.current_effect = AUDIO_EFFECT_NONE;
}

static void audio_level_effect(int effect){
    if(!g_audio.effect_playing && g_audio.effect_pending == AUDIO_EFFECT_NONE) audio_play_effect(effect, 1);
}
static void audio_stop_level_effect(int effect){
    if(effect < 0 || effect >= SFX_COUNT) return;
    if(g_audio.current_effect == effect){
        g_audio.effect_pending = SFX_COUNT;
        g_audio.current_effect = AUDIO_EFFECT_NONE;
    }
}

static void audio_prime_fifo(unsigned channel){
    volatile AudioState* a = &g_audio;
    SoundStream* st = channel ? &g_effect_stream : &g_music_stream;
    int8_t* b0 = channel ? g_effect_buffer[0] : g_music_buffer[0];
    int8_t* b1 = channel ? g_effect_buffer[1] : g_music_buffer[1];
    unsigned blocks = (channel ? AUDIO_EFFECT_BUFFER : AUDIO_MUSIC_BUFFER) / SOUND_BLOCK;
    sound_fill(st, b0, blocks);
    sound_fill(st, b1, blocks);
    REG_SOUNDCNT_H |= channel ? SOUNDCNT_B_RESET : SOUNDCNT_A_RESET;
    const uint32_t* first = (const uint32_t*)(const void*)b0;
    for(int i = 0; i < 8; i++) REG32(channel ? REG_FIFO_B : REG_FIFO_A) = first[i];
    if(channel){
        a->effect_buffer = 0;
        a->effect_dirty = 0;
        a->effect_dma_position = 32;
        REG_IF = AUDIO_IRQ_DMA2;
        REG_DMA2SAD = (uint32_t)(b0 + 32);
        REG_IE |= AUDIO_IRQ_DMA2;
        REG_DMA2CNT_H = DMA_SOUND | 0x4000;
    } else {
        a->music_buffer = 0;
        a->music_dirty = 0;
        a->music_dma_position = 32;
        REG_IF = AUDIO_IRQ_DMA1;
        REG_DMA1SAD = (uint32_t)(b0 + 32);
        REG_IE |= AUDIO_IRQ_DMA1;
        REG_DMA1CNT_H = DMA_SOUND | 0x4000;
    }
}

static void audio_start_fifo(unsigned channel, const SoundBlob* blob){
    sound_stream_start(channel ? &g_effect_stream : &g_music_stream, blob);
    audio_prime_fifo(channel);
}

static void audio_music_fifo_stop(void){
    REG_DMA1CNT_H &= (uint16_t)~0x8000u;
    REG_IE &= ~AUDIO_IRQ_DMA1;
    REG_SOUNDCNT_H |= SOUNDCNT_A_RESET;
}

static void audio_sub_music_end(void){
    volatile AudioState* a = &g_audio;
    audio_music_fifo_stop();
    g_bgm.sub_playing = 0;
    g_bgm.lead = 0;
    g_music_stream = g_stage_shadow;
    a->music_playing = g_bgm.shadow_playing;
    if(a->music_playing) audio_prime_fifo(0);
}

static void audio_music_vblank(void){
    volatile AudioState* a = &g_audio;
    volatile BgmLayers* l = &g_bgm;
    bool restarted = false, shadow_set = false;

    if(a->music_pending >= 0){
        int track = a->music_pending;
        a->music_pending = -1;
        if(l->sub_playing){
            l->shadow_playing = track < MUSIC_COUNT;
            if(l->shadow_playing) sound_stream_start(&g_stage_shadow, music_tracks[track]);
            shadow_set = true;
        } else {
            audio_music_fifo_stop();
            a->music_playing = track < MUSIC_COUNT;
            if(a->music_playing) audio_start_fifo(0, music_tracks[track]);
            restarted = true;
        }
    }
    if(l->sub_pending >= 0){
        int track = l->sub_pending;
        l->sub_pending = -1;
        if(track < MUSIC_COUNT){
            if(!l->sub_playing){
                g_stage_shadow = g_music_stream;
                l->shadow_playing = a->music_playing;
                l->sub_playing = 1;
                shadow_set = true;
            }
            audio_music_fifo_stop();
            l->sub_track = (int8_t)track;
            l->lead = l->lead_pending;
            a->music_playing = !l->lead;
            if(a->music_playing) audio_start_fifo(0, music_tracks[track]);
            restarted = true;
        } else if(l->sub_playing){
            audio_sub_music_end();
            restarted = true;
        }
    }

    if(l->sub_playing && l->shadow_playing && !shadow_set){
        int8_t scratch[SOUND_BLOCK] __attribute__((aligned(4)));
        for(int i = 0; i < SOUND_MUSIC_SAMPLES_PER_REFRESH / SOUND_BLOCK; i++)
            sound_fill(&g_stage_shadow, scratch, 1);
    }
    if(restarted) return;
    if(l->sub_playing){
        if(l->lead && !--l->lead){
            audio_start_fifo(0, music_tracks[l->sub_track]);
            a->music_playing = 1;
            return;
        }

        const SoundBlob* blob = g_music_stream.blob;
        if(a->music_playing && blob->loop_start >= blob->length &&
           g_music_stream.next == g_music_stream.end){
            audio_sub_music_end();
            return;
        }
    }
    if(a->music_playing){
        unsigned dirty = a->music_dirty;
        if(dirty & 1) sound_fill(&g_music_stream, g_music_buffer[0], AUDIO_MUSIC_BUFFER / SOUND_BLOCK);
        if(dirty & 2) sound_fill(&g_music_stream, g_music_buffer[1], AUDIO_MUSIC_BUFFER / SOUND_BLOCK);
        a->music_dirty &= (uint8_t)~dirty;
    }
}

void audio_vblank(void){
    volatile AudioState* a = &g_audio;
    if(!(REG_TM0CNT_H & TM_ENABLE)){
        REG_TM0CNT_H = TM_ENABLE;
        REG_TM1CNT_H = TM_ENABLE;
    }

    audio_music_vblank();

    if(a->effect_pending == AUDIO_EFFECT_NONE && a->effect_playing){
        a->effect_position += SOUND_EFFECT_SAMPLES_PER_REFRESH;
        if(a->effect_position >= a->effect_length + 64){
            REG_DMA2CNT_H &= (uint16_t)~0x8000u;
            REG_IE &= ~AUDIO_IRQ_DMA2;
            a->effect_playing = 0;
        } else {
            unsigned dirty = a->effect_dirty;
            if(dirty & 1) sound_fill(&g_effect_stream, g_effect_buffer[0], AUDIO_EFFECT_BUFFER / SOUND_BLOCK);
            if(dirty & 2) sound_fill(&g_effect_stream, g_effect_buffer[1], AUDIO_EFFECT_BUFFER / SOUND_BLOCK);
            a->effect_dirty &= (uint8_t)~dirty;
        }
    }
    if(a->effect_pending == AUDIO_EFFECT_NONE && !a->effect_playing &&
       g_effect_queue.read != g_effect_queue.write){
        unsigned read = g_effect_queue.read;
        unsigned slot = read & (AUDIO_EFFECT_QUEUE_SIZE - 1);
        a->current_effect = g_effect_queue.entry[slot].id;
        a->effect_priority = g_effect_queue.entry[slot].priority;
        a->effect_pending = g_effect_queue.entry[slot].id;
        g_effect_queue.read = (uint8_t)(read + 1);
    }
    if(a->effect_pending != AUDIO_EFFECT_NONE){
        REG_DMA2CNT_H &= (uint16_t)~0x8000u;
        REG_IE &= ~AUDIO_IRQ_DMA2;
        REG_SOUNDCNT_H |= SOUNDCNT_B_RESET;
        if(a->effect_pending >= SFX_COUNT){
            a->effect_playing = 0;
            a->current_effect = AUDIO_EFFECT_NONE;
            a->effect_pending = AUDIO_EFFECT_NONE;
            return;
        }
        const SoundBlob* blob = sound_effects[a->effect_pending];

        uint16_t h = (uint16_t)(REG_SOUNDCNT_H & ~(SOUNDCNT_A_RESET | SOUNDCNT_B_RESET));
        REG_SOUNDCNT_H = a->effect_pending == g_effect_half_id ? (uint16_t)(h & ~SOUNDCNT_B_FULL)
                                                               : (uint16_t)(h | SOUNDCNT_B_FULL);
        g_effect_half_id = AUDIO_EFFECT_NONE;
        a->effect_length = blob->length;
        a->effect_position = 0;
        audio_start_fifo(1, blob);
        a->effect_playing = 1;
        a->effect_pending = AUDIO_EFFECT_NONE;
    }
}
#endif
