#!/usr/bin/env python3
"""Compare emitted triangles from the cached-projection path and old clip path.

Uses production projection/clipping functions and randomized transformed poses;
the reference always calls the original clipped-face callback.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
engine = (ROOT / "src/graphics/engine.c").read_text()
header = (ROOT / "src/graphics/engine.h").read_text()
frame = (ROOT / "src/graphics/frame.c").read_text()
math = (ROOT / "src/math/vector_math.c").read_text()
types = (ROOT / "src/assets/asset_format.h").read_text()
start = engine.index("IWRAM_FN static int32_t engine_ratio16")
end = engine.index("static inline const uint16_t* stage_water_table", start)
clipping = engine[start:end]
start = engine.index("static const FigureTriangle* g_figure_draw_triangles;", end)
end = engine.index("static const TextureReference g_shadow_texture", start)
figures = engine[start:end]
area = engine[engine.index("IWRAM_CODE static inline int32_t screen_area"):
              engine.index("IWRAM_CODE static inline bool screen_small")]
job_start = header.index("typedef struct {\n    const FramePoint* points;")
job_end = header.index("} FigureRasterJob;", job_start) + len("} FigureRasterJob;")
prefix = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#define __GNW__ 1
#define IWRAM_CODE
#define IWRAM_FN __attribute__((noinline))
#define EWRAM_BSS
#define FRAME_WIDTH 160
#define FRAME_HEIGHT 120
#define FRAME_STRIDE 320
#define FX_ONE 256
#define FXC(x) ((int32_t)((x)*256))
#define CLIP_GUARD 16
#define CLIP_NEAR_Z 160
#define TRI_ALPHA 0x8000
typedef int32_t fx;
typedef struct {fx x,y;} v2;
typedef struct {fx x,y,z;} v3;
static v2 V2(fx x,fx y){return (v2){x,y};}
static int32_t lut_project[65536];
typedef struct {uintptr_t mip[2];} TextureReference;
typedef struct {v3 position;uint8_t clip,mark;uint16_t pad;} FramePoint;
typedef struct {uint16_t point;int16_t u,v;} ModelVertex;
typedef struct {ModelVertex v[3];uint16_t texture,flags;} FigureTriangle;
typedef struct {v3 p;int32_t u,v;} ClipVertex;
enum {RM_FLAT,RM_TEX,RM_ALPHA,RM_ADD,RM_DARKEN,RM_TINT};
extern const uint16_t* g_additive_rows;
typedef struct {uint16_t next;uint8_t mode,colour;uint32_t s[3],uv[3];const TextureReference* texture;} DrawEntry;
static DrawEntry emitted[16];
static int32_t emitted_depth[16][3], pending_depth[3];
static void engine_set_depth(fx a,fx b,fx c){pending_depth[0]=a;pending_depth[1]=b;pending_depth[2]=c;}
static unsigned count;
static void engine_out(int bin,uint32_t a,uint32_t b,uint32_t c,uint32_t ua,uint32_t ub,uint32_t uc,unsigned mode,unsigned colour,const TextureReference* texture){
    if(count>=16)abort();
    memcpy(emitted_depth[count],pending_depth,sizeof pending_depth);
    DrawEntry* e=&emitted[count++];
    e->next=bin;e->mode=mode;e->colour=colour;
    e->s[0]=a;e->s[1]=b;e->s[2]=c;e->uv[0]=ua;e->uv[1]=ub;e->uv[2]=uc;e->texture=texture;
}
static uint32_t rng=0xdecafbad;
static uint32_t next(void){rng^=rng<<13;rng^=rng>>17;rng^=rng<<5;return rng;}
'''
suffix = r'''
static FramePoint points[3];
static void old_face(int face){engine_figure_clipped(points,face);}
int main(void){
    /* Both paths use the same nontrivial perspective table and production
     * projection; its absolute scale does not affect path equivalence. */
    for(unsigned i=0;i<65536;i++)lut_project[i]=(1u<<20)/(i?i:1);
    TextureReference textures[2]={0};
    FigureTriangle tri={0};
    uint32_t screens[3];uint16_t order[1]={0};uint16_t additive[256]={0};
    FigureRasterJob job={.points=points,.screen=screens,.order=order,.count=1,
                        .step=2,.tris=&tri,.textures=textures,.clipped=old_face};
    g_figure_draw_triangles=&tri;g_figure_draw_textures=textures;
    unsigned fast=0,slow=0;
    for(unsigned n=0;n<40000;n++){
        for(int i=0;i<3;i++){
            int z=next()%12000-400;
            int x=(int)(next()%50000)-25000,y=(int)(next()%50000)-25000;
            if(n%2){z=160+next()%12000;x=(int)(next()%(z*8+1))-z*4;y=(int)(next()%(z*8+1))-z*4;}
            if(n%19==0)z=CLIP_NEAR_Z+(int)(n%3)-1;
            points[i].position=(v3){x,y,z};
            points[i].clip=view_clip_mask(points[i].position);
            screens[i]=points[i].clip&CLIP_GUARD?0:screen_pack_position(project_to_screen(points[i].position));
            tri.v[i]=(ModelVertex){i,(int16_t)next(),(int16_t)next()};
        }
        if((points[0].clip|points[1].clip|points[2].clip)&CLIP_GUARD)slow++;else fast++;
        tri.texture=n%2;tri.flags=n%3?0:TRI_ALPHA;
        job.textured=n%5?RM_TEX:RM_ADD;g_additive_rows=job.textured==RM_ADD?additive:NULL;
        DrawEntry reference[16];
        int32_t reference_depth[16][3];
        count=0;memset(emitted,0,sizeof emitted);old_face(0);
        unsigned ref_count=count;memcpy(reference,emitted,sizeof reference);
        memcpy(reference_depth,emitted_depth,sizeof reference_depth);
        count=0;memset(emitted,0,sizeof emitted);engine_draw_figure_triangles(&job);
        if(count!=ref_count||memcmp(reference,emitted,count*sizeof(DrawEntry))||
           memcmp(reference_depth,emitted_depth,count*sizeof pending_depth)){
            fprintf(stderr,"figure %u: emitted %u vs reference %u triangles\n",n,count,ref_count);return 1;
        }
    }
    printf("40000 figure triangles match the original clip path (%u cached, %u clipped); winding, UVs, alpha/additive, near/guard planes\n",fast,slow);
    return 0;
}
'''
# Use the project's actual alpha flag so this remains meaningful if it changes.
import re
alpha = re.search(r"TRI_ALPHA\s*=\s*([^,\n]+)", types)
if alpha:
    prefix = prefix.replace("#define TRI_ALPHA 0x8000", "#define TRI_ALPHA " + alpha.group(1))
projection = frame[:frame.index("static inline uint8_t frame_get")]
mask_start = math.index("static inline uint32_t view_clip_mask")
mask_end = math.index("\nstatic v2 fx_sin_cos", mask_start)
(ROOT / "tmp").mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix="figure-test-", dir=ROOT / "tmp") as directory:
    path = Path(directory)
    (path / "test.c").write_text(prefix + header[job_start:job_end] + "\n" + projection + area +
                                  math[mask_start:mask_end] + clipping + figures + suffix)
    try:
        subprocess.run(["docker", "run", "--rm", "--name", path.name, "--user", "1000:1000", "-v", f"{path}:/test",
                        "-w", "/test", "-e", "ASAN_OPTIONS=abort_on_error=1:handle_segv=0", "smg-runtime-tests", "sh", "-c",
                        "cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address test.c -o test && ./test"], check=True, timeout=60)
    finally:
        subprocess.run(["docker", "rm", "-f", path.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
