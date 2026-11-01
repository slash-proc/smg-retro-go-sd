#!/usr/bin/env python3
"""Depth regression using the captured Honeyhive wall/ground overlap."""
import ast
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
tree = ast.parse((ROOT / "tools/test_raster.py").read_text())
prefix = next(ast.literal_eval(node.value) for node in tree.body
              if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == "prefix" for t in node.targets))
for declaration in ("static bool g_depth_active;", "static bool g_perspective_active;", "static const uint32_t* g_raster_depth;",
                    "static uint16_t g_depth_buffer[FRAME_WIDTH*FRAME_HEIGHT];"):
    prefix = prefix.replace(declaration, "")
prefix += r'''
#include <assert.h>
#include <time.h>
typedef int32_t fx;
#define CLIP_NEAR_Z 160
#define ENGINE_ARENA_MAX 2048
#define BIN_END 65535
#define EWRAM_BSS
#define SMG_PERF_RASTER 0
static unsigned smg_perf_now(void){return 0;}
static void smg_perf_add(unsigned stage,unsigned start){(void)stage;(void)start;}
static DrawEntry g_arena[ENGINE_ARENA_MAX];
'''
source = (ROOT / "src/graphics/engine.c").read_text()
control = source[source.index("static bool g_depth_enabled;"):source.index("#else\n#define engine_set_depth")]
start = source.index("static int64_t raster_edge(")
end = source.index("\n#endif\n\nstatic const FigureTriangle*", start)
production = source[start:end]
suffix = r'''
static uint8_t framebuffer[FRAME_STRIDE*FRAME_HEIGHT];
static TextureReference texture;
static uint8_t texels[1024];
static DrawEntry ground={.mode=RM_FLAT,.colour=51,.s={65339837,30476490,98042461}};
static DrawEntry wall={.mode=RM_FLAT,.colour=89,.s={91423785,45417463,45679837}};

static void submit(const DrawEntry* e,int a,int b,int c){
    engine_set_depth(a,b,c);
    g_raster_depth=g_pending_depth;
    engine_raster(e);
}
static void begin(bool depth){
    memset(framebuffer,7,sizeof framebuffer);
    g_depth_enabled=depth;g_engine_fb=framebuffer;engine_depth_begin();
}
static double now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec*1e-9;}

static void compare_setup_modes(void){
    static uint8_t reference_pixels[sizeof framebuffer],mip[1024],add[65536];
    static uint16_t reference_depth[FRAME_WIDTH*FRAME_HEIGHT],rows[256];
    for(unsigned i=0;i<1024;i++){texels[i]=(i%7)?(i*17+31)%255+1:0;mip[i]=(i%5)?(i*23+11)%255+1:0;}
    for(unsigned i=0;i<65536;i++)add[i]=(i*13+(i>>8)*7)&255;
    for(unsigned i=0;i<256;i++){rows[i]=i;shade_darken[i]=(i*7)&255;g_shade_water[i]=(i*19)&255;}
    g_additive_rows=rows;g_additive_table=add;
    for(unsigned n=0;n<600;n++){
        DrawEntry entries[4]={0};int z[4][3];
        texture.mip[0]=(uintptr_t)texels;texture.mip[1]=(uintptr_t)(n%3?mip:texels);
        for(unsigned t=0;t<4;t++){
            DrawEntry* e=&entries[t];e->mode=(n+t)%6;e->colour=next();e->texture=&texture;
            for(unsigned i=0;i<3;i++){
                int x=(int)(next()%(240*16))-40*16,y=(int)(next()%(200*16))-40*16;
                e->s[i]=(uint16_t)x|((uint32_t)(uint16_t)y<<16);e->uv[i]=next();
                z[t][i]=160+next()%65536;
                if(n%13==0)z[t][i]=1000; // constant-depth zero gradients
                if(n%17==0)e->uv[i]=17u|(31u<<16); // zero UV gradients
            }
        }
        for(unsigned enabled=0;enabled<2;enabled++){
            g_raster_setup_recip_enabled=enabled;begin(true);
            for(unsigned t=0;t<4;t++){
                engine_set_depth(z[t][0],z[t][1],z[t][2]);g_raster_depth=g_pending_depth;
                engine_raster_inner(&entries[t]);
            }
            if(!enabled){memcpy(reference_pixels,framebuffer,sizeof framebuffer);memcpy(reference_depth,g_depth_buffer,sizeof reference_depth);}
            else if(memcmp(reference_pixels,framebuffer,sizeof framebuffer)||memcmp(reference_depth,g_depth_buffer,sizeof reference_depth)){
                fprintf(stderr,"Reciprocal setup changes pixels/depth in group %u\n",n);abort();
            }
        }
    }
    g_raster_setup_recip_enabled=true;
    puts("600 four-triangle groups match pixels and depth in both setup modes across all materials, winding, mipmaps and zero gradients.");
}

static int check_depth(void){
    ground.mode=wall.mode=RM_FLAT;memset(texels,0,sizeof texels);
    /* Recorded entries 106 and 119. At (70,66) the ground is at z=4329,
     * wall z=6372, but the old painter list submits the wall last. */
    begin(false);submit(&ground,5802,8486,3041);submit(&wall,6793,6051,5938);
    assert(framebuffer[66*FRAME_STRIDE+70]==89);engine_depth_end();
    for(unsigned reverse=0;reverse<2;reverse++){
        begin(true);
        if(reverse)submit(&wall,6793,6051,5938);
        submit(&ground,5802,8486,3041);
        if(!reverse)submit(&wall,6793,6051,5938);
        assert(framebuffer[66*FRAME_STRIDE+70]==51);
        assert(g_depth_buffer[66*FRAME_WIDTH+70]>=1930&&g_depth_buffer[66*FRAME_WIDTH+70]<=1940);
        engine_depth_end();
    }
    /* Transparent foreground must not hide the background or update depth. */
    DrawEntry cutout=ground;cutout.mode=RM_ALPHA;cutout.texture=&texture;
    texture.mip[0]=texture.mip[1]=(uintptr_t)texels;
    begin(true);submit(&wall,6793,6051,5938);
    uint16_t old_depth=g_depth_buffer[66*FRAME_WIDTH+70];
    submit(&cutout,1000,1000,1000);
    assert(framebuffer[66*FRAME_STRIDE+70]==89);
    assert(g_depth_buffer[66*FRAME_WIDTH+70]==old_depth);
    memset(texels,123,sizeof texels);submit(&cutout,1000,1000,1000);
    assert(framebuffer[66*FRAME_STRIDE+70]==123);engine_depth_end();
    /* Deferred shadow must shade the final ground, even when queued before it.
     * Copy the texture descriptor: later figures may replace the original. */
    DrawEntry shadow=ground;shadow.mode=RM_DARKEN;shadow.texture=&texture;
    for(int i=0;i<256;i++)shade_darken[i]=(uint8_t)(i/2);
    begin(true);submit(&shadow,5800,8484,3039);
    assert(g_depth_effect_count==1&&framebuffer[66*FRAME_STRIDE+70]==7);
    texture.mip[0]=texture.mip[1]=0;
    submit(&ground,5802,8486,3041);engine_depth_end();
    assert(framebuffer[66*FRAME_STRIDE+70]==25);
    /* Behind-surface shadow fails depth and leaves existing depth untouched. */
    texture.mip[0]=texture.mip[1]=(uintptr_t)texels;
    begin(true);submit(&shadow,12000,12000,12000);submit(&ground,5802,8486,3041);
    old_depth=g_depth_buffer[66*FRAME_WIDTH+70];engine_depth_end();
    assert(framebuffer[66*FRAME_STRIDE+70]==51&&g_depth_buffer[66*FRAME_WIDTH+70]==old_depth);
    puts("Captured wall/ground overlap is order independent; cutout holes preserve depth; deferred shadows target the final visible surface.");

    compare_setup_modes();

    /* Host-only comparison. It is not an STM32 or whole-game FPS prediction. */
    ground.mode=RM_TEX;ground.texture=&texture;wall.mode=RM_TEX;wall.texture=&texture;
    for(int depth=0;depth<2;depth++){
        double start=now();
        for(int n=0;n<10000;n++){
            begin(depth);submit(&ground,5802,8486,3041);submit(&wall,6793,6051,5938);engine_depth_end();
        }
        printf("Host fixture: depth %s %.3f microseconds/frame (two captured triangles plus clear)\n",depth?"on":"off",(now()-start)*100);
    }
    return 0;
}
int main(void){
    g_perspective_enabled=false;check_depth();
    g_perspective_enabled=true;check_depth();
    puts("Depth, cutout and deferred effects pass with perspective collection disabled and enabled.");
    return 0;
}

'''
with tempfile.TemporaryDirectory(prefix="depth-test-", dir=ROOT / "tmp") as directory:
    path = Path(directory)
    (path / "test.c").write_text(prefix + control + production + suffix)
    try:
        subprocess.run([
            "docker", "run", "--rm", "--name", path.name, "--user", "1000:1000",
            "-v", f"{path}:/test", "-w", "/test", "smg-runtime-tests", "sh", "-c",
            "cc -O3 -D_POSIX_C_SOURCE=200809L -fno-strict-aliasing -no-pie "
            "-fsanitize=address,undefined test.c -o test && ASAN_OPTIONS=handle_segv=0 ./test && "
            "cc -O3 -D_POSIX_C_SOURCE=200809L -fno-strict-aliasing test.c -o benchmark && ./benchmark",
        ], check=True, timeout=60)
    finally:
        subprocess.run(["docker", "rm", "-f", path.name],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
