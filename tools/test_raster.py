#!/usr/bin/env python3
"""Differentially test the production GNW raster against its frozen reference.

docker build -t smg-runtime-tests -f tools/tests/Dockerfile .
python3 tools/test_raster.py
"""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "src/graphics/engine.c").read_text()
start = source.index("static int64_t raster_edge(")
end = source.index("\nvoid engine_raster(const DrawEntry* entry)", start)
production = source[start:end]
reference = (ROOT / "tools/tests/raster_reference.inc").read_text()
prefix = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifndef FRAME_WIDTH
#define FRAME_WIDTH 160
#define FRAME_HEIGHT 120
#endif
#define FRAME_STRIDE 320
#define TEX_DIM 32
#define TEX_MIP1_STRIDE 64
#define SMG_COUNT(field) ((void)0)
#define SMG_PEAK(field,value) ((void)0)
enum {RM_FLAT,RM_TEX,RM_ALPHA,RM_ADD,RM_DARKEN,RM_TINT,RM_BLEND_QUARTER};
typedef struct {uintptr_t mip[2];} TextureReference;
typedef struct {uint16_t next; uint8_t mode,colour; uint32_t s[3],uv[3]; const TextureReference* texture;} DrawEntry;
static int32_t screen_x(uint32_t s){return (int16_t)s;}
static int32_t screen_y(uint32_t s){return (int16_t)(s>>16);}
uint8_t g_shade_water[256],shade_darken[256],*g_engine_fb;
static uint8_t shade_quarter[65536];
const uint16_t* g_additive_rows;
const uint8_t* g_additive_table;
static bool g_depth_active;
static bool g_perspective_active;
static const uint32_t* g_raster_depth;
static uint16_t g_depth_buffer[FRAME_WIDTH*FRAME_HEIGHT];
const uint16_t engine_recip_table[256] = {65535,65281,65028,64777,64528,64281,64035,63792,63550,63310,63072,62836,62602,62369,62138,61909,61681,61455,61231,61008,60787,60568,60350,60133,59919,59705,59494,59283,59075,58867,58662,58457,58254,58053,57852,57654,57456,57260,57065,56872,56680,56489,56299,56111,55924,55738,55554,55370,55188,55007,54828,54649,54471,54295,54120,53946,53773,53601,53431,53261,53092,52925,52759,52593,52429,52265,52103,51942,51782,51622,51464,51306,51150,50995,50840,50686,50534,50382,50231,50081,49932,49784,49637,49490,49345,49200,49056,48913,48771,48630,48489,48349,48210,48072,47935,47798,47663,47528,47393,47260,47127,46995,46864,46733,46603,46474,46346,46218,46091,45965,45839,45714,45590,45467,45344,45222,45100,44979,44859,44739,44620,44502,44384,44267,44151,44035,43919,43805,43691,43577,43464,43352,43240,43129,43019,42908,42799,42690,42582,42474,42367,42260,42154,42048,41943,41838,41734,41631,41528,41425,41323,41222,41121,41020,40920,40820,40721,40623,40525,40427,40330,40233,40137,40041,39946,39851,39756,39662,39569,39476,39383,39291,39199,39108,39017,38926,38836,38746,38657,38568,38480,38392,38304,38217,38130,38044,37958,37872,37787,37702,37617,37533,37449,37366,37283,37200,37118,37036,36954,36873,36792,36712,36631,36552,36472,36393,36314,36236,36158,36080,36003,35926,35849,35772,35696,35620,35545,35470,35395,35320,35246,35172,35099,35026,34953,34880,34808,34735,34664,34592,34521,34450,34380,34309,34239,34169,34100,34031,33962,33893,33825,33757,33689,33622,33554,33487,33421,33354,33288,33222,33157,33091,33026,32961,32897,32832};
static uint32_t rng=0xf00dcafe;
static uint32_t next(void){rng^=rng<<13;rng^=rng>>17;rng^=rng<<5;return rng;}
'''
suffix = r'''
int main(void){
    uint8_t tex[1024],mip[1024],add[65536]; uint16_t rows[256];
    uint8_t a[FRAME_STRIDE*FRAME_HEIGHT+64],b[sizeof a];
    for(int i=0;i<1024;i++)tex[i]=next();
    for(int i=0;i<1024;i++)mip[i]=next();
    for(int i=0;i<65536;i++)add[i]=next();
    for(int i=0;i<256;i++){rows[i]=next()&255;shade_darken[i]=next();g_shade_water[i]=next();}
    TextureReference t={{(uintptr_t)tex,(uintptr_t)mip}};
    g_additive_rows=rows;g_additive_table=add;
    for(int n=0;n<24000;n++){
        DrawEntry e={0};e.mode=n%7;if(e.mode==RM_BLEND_QUARTER)e.mode=7;e.colour=next();
        e.texture=(n%23)?&t:NULL;
        t.mip[1]=(n%11)?(uintptr_t)mip:(uintptr_t)tex;
        for(int i=0;i<3;i++){
            int x,y;
            if(n%5==0){x=(int16_t)next();y=(int16_t)next();}
            else {x=(int)(next()%(240*16))-40*16;y=(int)(next()%(200*16))-40*16;}
            if(n%13==0)y=320; /* degenerate triangles */
            e.s[i]=(uint16_t)x|((uint32_t)(uint16_t)y<<16);
            e.uv[i]=next();
        }
        if(n%37==0){ /* immediately below/at/above the mip threshold, both signs */
            e.s[0]=16u|(16u<<16);e.s[1]=176u|(16u<<16);e.s[2]=16u|(176u<<16);
            e.uv[0]=0;e.uv[1]=(uint16_t)((959+n%3)*(n%2?-1:1));e.uv[2]=0;
        }
        memset(a,n&255,sizeof a);memcpy(b,a,sizeof a);
        g_engine_fb=a+32;reference_raster(&e);
        g_engine_fb=b+32;engine_raster_inner(&e);
        if(memcmp(a,b,sizeof a)){
            for(unsigned i=0;i<sizeof a;i++)if(a[i]!=b[i]){
                fprintf(stderr,"triangle %d mode %u byte %u: reference=%u actual=%u\n",n,e.mode,i,a[i],b[i]);break;}
            return 1;
        }
    }
    puts("24000 triangles match byte-for-byte across all modes, winding, clipping, UV wrapping, mipmaps and degenerate inputs");
    return 0;
}
'''
scratch = ROOT / "tmp"
scratch.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix="raster-test-", dir=scratch) as directory:
    path = Path(directory)
    (path / "test.c").write_text(prefix + reference + production + suffix)
    try:
        subprocess.run(["docker", "run", "--rm", "--name", path.name, "--user", f"{os.getuid()}:{os.getgid()}", "-v", f"{path}:/test",
                        "-w", "/test", "-e", "ASAN_OPTIONS=abort_on_error=1:handle_segv=0", "smg-runtime-tests", "sh", "-c",
                        "cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address -fno-omit-frame-pointer test.c -o test && ./test && "
                        "cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address -fno-omit-frame-pointer -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 test.c -o test-native && ./test-native"], check=True, timeout=60)
    finally:
        subprocess.run(["docker", "rm", "-f", path.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
