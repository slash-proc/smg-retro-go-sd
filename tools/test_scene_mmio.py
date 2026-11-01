#!/usr/bin/env python3
"""Exercise scene transition code with GBA MMIO deliberately unmapped on GNW."""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def function(path, name):
    source = (ROOT / path).read_text()
    start = source.index('static void ' + name + '(')
    body = source.index('{', start)
    depth = 1
    end = body + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end] + '\n'

prefix = r'''
#include <stdint.h>
#include <stdbool.h>
#include <assert.h>
#include <stdio.h>
#ifdef __GNW__
#define REG16(a) (*(volatile uint16_t*)(uintptr_t)(a))
#define REG32(a) (*(volatile uint32_t*)(uintptr_t)(a))
#else
static uint32_t registers[256];
#define REG16(a) (*(volatile uint16_t*)&registers[((a)-0x04000000)/2])
#define REG32(a) registers[((a)-0x04000000)/2]
#endif
#define REG_BLDCNT REG16(0x04000050)
#define REG_BLDALPHA REG16(0x04000052)
#define REG_BLDY REG16(0x04000054)
#define REG_DMA0CNT REG32(0x040000B8)
#define REG_DISPCNT REG16(0x04000000)
enum { SCENE_PLAY=1, SCENE_STAR_GET, SCENE_MISSION_SELECT };
enum { SCENARIO_SELECT_COMET_APPEAR=1, SCENARIO_SELECT_COMET_END=3 };
enum { WIPE_OFF, WIPE_CLOSING, WIPE_OPENING, WIPE_CLOSED };
#define SS_BAND_EVA 12
typedef struct { int scene, fade; } Game;
static Game g_game;
static struct { int dir; bool white; } g_screen_fade;
static struct { int comet, nerve, step, vram; } g_ss;
static struct { int mode, step, frames; } g_wipe;
static int g_star_get_banner_alpha, fade_level, palette_calls, close_calls;
static int screen_fade_level(void){return fade_level;}
static void ss_write_band_palette(void){palette_calls++;}
static int ss_band_level(void){return 16;}
static void ss_close(void){close_calls++;g_ss.vram=0;}
'''
source = prefix + function('src/ui/hud.c', 'hud_set_fade')
source += function('src/game/star_get.c', 'star_get_set_blend')
source += function('src/game/star_get.c', 'wipe_tick')
source += function('src/game/scenario_select.c', 'scenario_select_set_fade')
source += r'''
int main(void){
    g_game.scene=SCENE_MISSION_SELECT;
    for(int white=0;white<2;white++)
    for(int fade=0;fade<=16;fade++)
    for(int comet=0;comet<2;comet++)
    for(int nerve=0;nerve<=4;nerve++){
        g_screen_fade.white=white;fade_level=fade;g_ss.comet=comet;
        g_ss.nerve=nerve;g_ss.step=25;
        scenario_select_set_fade(&g_game);
    }
    assert(palette_calls==340);
    g_ss.vram=1;g_game.scene=SCENE_PLAY;scenario_select_set_fade(&g_game);
    assert(close_calls==1);
    g_game.scene=SCENE_STAR_GET;
    for(int alpha=0;alpha<=16;alpha++){
        g_star_get_banner_alpha=alpha;star_get_set_blend();
#ifndef __GNW__
        if(alpha<16)assert(REG_BLDALPHA==(alpha|((16-alpha)<<8)));
#endif
    }
    g_wipe.mode=WIPE_OPENING;g_wipe.frames=2;g_wipe.step=0;
#ifndef __GNW__
    REG_DMA0CNT=0x80000001;REG_DISPCNT=0x2404;
#endif
    wipe_tick();assert(g_wipe.mode==WIPE_OPENING);
    wipe_tick();assert(g_wipe.mode==WIPE_OFF);
#ifndef __GNW__
    assert(REG_DMA0CNT==0&&REG_DISPCNT==0x404);
#endif
    g_wipe.mode=WIPE_CLOSING;g_wipe.step=1;wipe_tick();assert(g_wipe.mode==WIPE_CLOSED);
    puts("Scene transitions passed: mission/comet fades, star banner, wipe completion");
}
'''
with tempfile.TemporaryDirectory(prefix='scene-mmio-', dir=ROOT / 'tmp') as directory:
    path = Path(directory)
    (path / 'test.c').write_text(source)
    subprocess.run(['docker','run','--rm','--user',f'{os.getuid()}:{os.getgid()}','-v',f'{path}:/test',
                    '-w','/test','smg-runtime-tests','sh','-c',
                    'cc -O2 -fsanitize=address,undefined -no-pie -D__GNW__ test.c -o gnw && ./gnw && '
                    'cc -O2 -fsanitize=address,undefined -no-pie test.c -o gba && ./gba'], check=True)
