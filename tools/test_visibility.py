#!/usr/bin/env python3
"""Differential checks of GNW visibility and transforms against their fallback."""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
s = (ROOT / "src/graphics/visibility.c").read_text()
h = (ROOT / "src/graphics/visibility.h").read_text()
frame = (ROOT / "src/graphics/frame.c").read_text()

def section(text, begin, end):
    a = text.index(begin)
    return text[a:text.index(end, a)]

types = section(h, "typedef struct {\n    const StageChunk*", "} CullJob;") + "} CullJob;\n"
types += section(h, "typedef struct {\n    const uint16_t* leaves;", "} CollectJob;") + "} CollectJob;\n"
types += section(h, "typedef struct {\n    const int16_t (*src)[3];", "} TransformJob;") + "} TransformJob;\n"
functions = "static volatile bool g_cull_narrow_enabled = true;\n"
functions += "struct {uint32_t version,narrow_jobs,wide_jobs;} smg_cull_math_stats;\n"
functions += section(s, "static bool cull_narrow_planes", "\n_Static_assert(offsetof(CullJob")
functions = functions.rsplit("\n#endif", 1)[0]
functions += section(s, "static volatile bool g_collect_narrow_enabled", "\nvoid geometry_collect_leaves(")
functions += section(s, "#define STAGE_VERTEX_CACHE_ENTRIES", "\n#endif /* __GNW__ transforms */")
prefix = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "assets/asset_format.h"
#define FRAME_WIDTH 160
#define FRAME_HEIGHT 120
#define FRAME_STRIDE 320
#define IWRAM_CODE
#define __GNW__ 1
#define EWRAM_BSS
#define SMG_COUNT(field) ((void)0)
#define SMG_ADD(field,count) ((void)(count))
#define SMG_PEAK(field,value) ((void)0)
#define FX_ONE 256
#define FXC(x) ((int32_t)((x)*256))
typedef struct {int32_t x,y,z;} v3;
typedef struct {int32_t x,y;} v2;
static v2 V2(int32_t x,int32_t y){return (v2){x,y};}
typedef struct {v3 position;uint8_t clip,mark;uint16_t pad;} FramePoint;
static int32_t lut_project[65536];
static struct {struct {const int16_t (*points)[3];} stage;} g_game;
static uint32_t rng=0x12345678;
static uint32_t next(void){rng^=rng<<13;rng^=rng>>17;rng^=rng<<5;return rng;}
'''
suffix = r'''
static unsigned tree(StageChunk* chunks, unsigned at, int depth, const int16_t* lo, const int16_t* hi){
    StageChunk* c=&chunks[at];
    memset(c,0,sizeof *c);
    for(int k=0;k<3;k++){
        c->min[k]=lo[k];c->max[k]=hi[k];
        c->cone_normal[k]=(int8_t)next();c->cone_normal_extent[k]=(uint8_t)next();
        c->cone_center[k]=(lo[k]+hi[k])/2;
    }
    c->cone_plane_limit=next()%3?CONE_NONE:(int32_t)next();
    c->leaf=depth?UINT16_MAX:at;
    unsigned end=at+1;
    if(depth){
        int16_t middle[3],upper[3],lower[3];
        memcpy(upper,hi,6);memcpy(lower,lo,6);
        for(int k=0;k<3;k++)middle[k]=(lo[k]+hi[k])/2;
        unsigned axis=depth%3;upper[axis]=middle[axis];lower[axis]=middle[axis];
        end=tree(chunks,end,depth-1,lo,upper);
        end=tree(chunks,end,depth-1,lower,hi);
    }
    c->skip=end;
    return end;
}
int main(void){
    for(unsigned i=0;i<65536;i++)lut_project[i]=(1u<<20)/(i?i:1);
    for(unsigned test=0;test<4000;test++){
        StageChunk chunks[256]={0};uint16_t leaves1[256]={0},leaves2[256]={0};
        CullJob c={.chunks=chunks,.n=256,.leaves=leaves1,.z_drop=next()%65536,.z_inner=test%2?256:0};
        for(unsigned i=0;i<256;i++){
            for(int k=0;k<3;k++){chunks[i].min[k]=(int16_t)next();chunks[i].max[k]=(int16_t)next();}
            chunks[i].skip=i+1;chunks[i].leaf=i;chunks[i].cone_plane_limit=CONE_NONE;
        }
        if(test%2){
            int16_t lo[3]={-30000,-30000,-30000},hi[3]={30000,30000,30000};
            c.n=tree(chunks,0,7,lo,hi);
            for(int k=0;k<3;k++)c.cam[k]=(int)(next()%300001)-150000;
        }
        for(int plane=0;plane<5;plane++){
            for(int k=0;k<3;k++){
                c.planes[plane][k]=(int32_t)(next()%4097)-2048;
                if(test%7==0)c.planes[plane][k]=(int32_t)(next()&0x3fffffff)-0x20000000;
                c.planes[plane][k+3]=abs(c.planes[plane][k]);
            }
            c.planes[plane][6]=(int32_t)(next()%65536)-32768;
        }
        CullJob cc=c;cc.leaves=leaves2;
        int a=reference_cull(&c),b=geometry_cull_chunks(&cc);
        if(a!=b||c.leaf_count!=cc.leaf_count||memcmp(leaves1,leaves2,sizeof leaves1)){
            fprintf(stderr,"cull mismatch %u\n",test);return 1;
        }

        uint16_t leaves[8],offsets[9],ranks[128],plist1[256]={0},plist2[256]={0},home[128];
        uint32_t bits1[8]={0},bits2[8]={0};uint8_t marks1[128]={0},marks2[128]={0};
        StageTriangle triangles[128];LeafTriangle records[128];int16_t points[128][3];
        for(int i=0;i<8;i++)leaves[i]=next()%8;
        for(int i=0;i<9;i++)offsets[i]=i*16;
        for(int i=0;i<128;i++){
            home[i]=i%16;ranks[i]=next()%256;
            for(int k=0;k<3;k++){
                points[i][k]=(int)(next()%8192)-4096;
                triangles[i].v[k].point=records[i].points[k]=next()%128;
                triangles[i].normal[k]=(int8_t)next();records[i].normal[k]=(int16_t)next();
                records[i].a[k]=(int16_t)next();
            }
        }
        CollectJob j={.leaves=leaves,.leaf_count=8,.leaf_triangle_offsets=offsets,.recs=records,
            .ranks=ranks,.limit=(int)(next()%8192)-4096,.cam={321,-450,78},.marks=marks1,
            .stamp=1,.bits=bits1,.lo=UINT32_MAX,.hi=0,.plist=plist1,.plist_end=plist1+1+test%128,
            .home=test%3?NULL:home,.points=points,.triangles=triangles,
            .axis_coord=test%6,.axis_sign=test%2};
        CollectJob jj=j;jj.marks=marks2;jj.bits=bits2;jj.plist=plist2;jj.plist_end=plist2+(j.plist_end-plist1);
        static const int32_t cameras[]={-1000000,-32769,-32768,-32767,0,32766,32767,32768,1000000};
        for(unsigned k=0;k<3;k++)j.cam[k]=jj.cam[k]=cameras[(test+k)%9];
        g_collect_narrow_enabled=test%5!=0;
        reference_collect(&j,test%2);geometry_collect_leaves_c(&jj,test%2);
        if(j.plist-plist1!=jj.plist-plist2||j.lo!=jj.lo||j.hi!=jj.hi||memcmp(marks1,marks2,sizeof marks1)||
           memcmp(bits1,bits2,sizeof bits1)||memcmp(plist1,plist2,sizeof plist1)){
            fprintf(stderr,"collect mismatch %u\n",test);return 1;
        }

        FramePoint transformed1[256],transformed2[256];uint32_t screen1[256],screen2[256];
        uint16_t list[128];int16_t delta[16];
        for(int i=0;i<128;i++)list[i]=next()%128;
        for(int i=0;i<16;i++)delta[i]=next()%16;
        memset(transformed1,0xa5,sizeof transformed1);memcpy(transformed2,transformed1,sizeof transformed1);
        memset(screen1,0x75,sizeof screen1);memcpy(screen2,screen1,sizeof screen1);
        TransformJob t={.src=points,.dst=transformed1,.screen=screen1,.list=test%2?list:NULL,.n=128,
                        .home=test%3?home:NULL,.delta=delta};
        for(int row=0;row<3;row++)for(int k=0;k<4;k++)t.m[row][k]=(int)(next()%1025)-512;
        // Exercise both packed-coefficient and safe-sum guard fallbacks.
        if(test%7==0)t.m[0][1]=40000;
        if(test%11==0)for(int k=0;k<3;k++)t.m[1][k]=-32768;
        TransformJob tt=t;tt.dst=transformed2;tt.screen=screen2;
        g_transform_dsp_enabled=true;
        reference_transform(&t);engine_transform_points(&tt);
        if(memcmp(transformed1,transformed2,sizeof transformed1)||memcmp(screen1,screen2,sizeof screen1)){
            fprintf(stderr,"transform mismatch %u\n",test);return 1;
        }
    }
    /* Exercise the cone fast-math guard at both excluded endpoints, the
     * adjacent included deltas, all signed8 normal extremes, uint8 extents,
     * and cone limits spanning int32. The reference keeps 64-bit products. */
    static const int32_t deltas[]={-131073,-131072,-131071,-1,0,1,131070,131071,131072};
    static const int32_t limits[]={INT32_MIN,INT32_MIN+1,INT32_MIN/2,-769,-768,-767,0,INT32_MAX};
    unsigned cone_cases=0;
    for(unsigned d=0;d<sizeof deltas/sizeof deltas[0];d++)
        for(unsigned l=0;l<sizeof limits/sizeof limits[0];l++){
            StageChunk chunk={.skip=1,.leaf=0,.cone_plane_limit=limits[l]};
            uint16_t wide_leaf[1]={0},narrow_leaf[1]={0};
            CullJob wide={.chunks=&chunk,.n=1,.z_drop=10000,.leaves=wide_leaf};
            CullJob narrow_job=wide;narrow_job.leaves=narrow_leaf;
            for(unsigned axis=0;axis<3;axis++){
                chunk.cone_normal[axis]=axis==d%3?INT8_MIN:INT8_MAX;
                chunk.cone_normal_extent[axis]=UINT8_MAX;
                wide.cam[axis]=deltas[(d+axis*3)% (sizeof deltas/sizeof deltas[0])];
                narrow_job.cam[axis]=wide.cam[axis];
            }
            int expected=reference_cull(&wide),actual=geometry_cull_chunks(&narrow_job);
            if(expected!=actual||wide.leaf_count!=narrow_job.leaf_count||
               memcmp(wide_leaf,narrow_leaf,sizeof wide_leaf)){
                fprintf(stderr,"cone boundary mismatch delta=%u limit=%u expected=%d actual=%d counts=%u/%u leaves=%u/%u planes=%u z=%d\n",
                    d,l,expected,actual,wide.leaf_count,narrow_job.leaf_count,wide_leaf[0],narrow_leaf[0],
                    wide.planes[0][0],wide.z_drop);return 1;
            }
            cone_cases++;
        }
    /* A whole job can skip cone arithmetic only when no signed16 center can
     * yield an accepted delta. Check both exact reachability endpoints and
     * the adjacent cameras on every axis against the wide oracle. */
    static const int32_t camera_edges[]={-163840,-163839,163838,163839};
    for(unsigned axis=0;axis<3;axis++)
        for(unsigned edge=0;edge<sizeof camera_edges/sizeof camera_edges[0];edge++){
            StageChunk chunk={.skip=1,.leaf=0,.cone_plane_limit=0};
            uint16_t wide_leaf[1]={0},narrow_leaf[1]={0};
            CullJob wide={.chunks=&chunk,.n=1,.z_drop=10000,.leaves=wide_leaf};
            CullJob narrow_job=wide;narrow_job.leaves=narrow_leaf;
            for(unsigned k=0;k<3;k++){
                chunk.cone_center[k]=wide.cam[k]=narrow_job.cam[k]=0;
                chunk.cone_normal[k]=0;
                chunk.cone_normal_extent[k]=0;
            }
            int32_t camera=camera_edges[edge];
            int16_t center=(camera<0)?INT16_MIN:INT16_MAX;
            chunk.cone_center[axis]=center;
            wide.cam[axis]=narrow_job.cam[axis]=camera;
            chunk.cone_normal[axis]=(camera<0)?INT8_MAX:INT8_MIN;
            int expected=reference_cull(&wide),actual=geometry_cull_chunks(&narrow_job);
            if(expected!=actual||wide.leaf_count!=narrow_job.leaf_count||
               memcmp(wide_leaf,narrow_leaf,sizeof wide_leaf)){
                fprintf(stderr,"cone camera reachability mismatch axis=%u camera=%d center=%d expected=%d actual=%d\n",
                    axis,camera,center,expected,actual);return 1;
            }
            cone_cases++;
        }
    if(!smg_collect_math_stats.narrow_jobs || !smg_collect_math_stats.wide_jobs)abort();
    printf("4000 culling/collection/transform jobs and %u cone-boundary cases match the wide reference exactly\n",cone_cases);
    return 0;
}
'''
(ROOT / "tmp").mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix="visibility-test-", dir=ROOT / "tmp") as directory:
    path = Path(directory)
    (path / "test.c").write_text(prefix + types + frame[:frame.index("static inline uint8_t frame_get")] +
        (ROOT / "tools/tests/visibility_reference.inc").read_text() + functions + suffix)
    try:
        subprocess.run(["docker", "run", "--rm", "--name", path.name, "--user", f"{os.getuid()}:{os.getgid()}",
            "-v", f"{path}:/test", "-v", f"{ROOT}:/source:ro", "-w", "/test",
            "-e", "ASAN_OPTIONS=abort_on_error=1:handle_segv=0", "smg-runtime-tests", "sh", "-c",
            "cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address -I/source/src test.c -o test && ./test"],
            check=True, timeout=60)
    finally:
        subprocess.run(["docker", "rm", "-f", path.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
