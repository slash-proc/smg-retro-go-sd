#!/usr/bin/env python3
"""Independent quarter-alpha, edge ownership and opaque-depth contracts."""
import ast
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
tree = ast.parse((ROOT / "tools/test_raster.py").read_text())
prefix = next(ast.literal_eval(n.value) for n in tree.body if isinstance(n, ast.Assign)
              and any(isinstance(t, ast.Name) and t.id == "prefix" for t in n.targets))
for declaration in ("static bool g_depth_active;", "static bool g_perspective_active;",
                    "static const uint32_t* g_raster_depth;",
                    "static uint16_t g_depth_buffer[FRAME_WIDTH*FRAME_HEIGHT];"):
    prefix = prefix.replace(declaration, "")
prefix += r'''
#include <assert.h>
#include <math.h>
#include "tools/asset_compiler/palette.h"
typedef int32_t fx;
#define CLIP_NEAR_Z 160
#define ENGINE_ARENA_MAX 2048
#define BIN_END 65535
#define EWRAM_BSS
#define SMG_PERF_RASTER 0
static unsigned smg_perf_now(void){return 0;}
static void smg_perf_add(unsigned a,unsigned b){(void)a;(void)b;}
static DrawEntry g_arena[ENGINE_ARENA_MAX];
'''
source = (ROOT / "src/graphics/engine.c").read_text()
control = source[source.index("static bool g_depth_enabled;"):source.index("#else\n#define engine_set_depth")]
production = source[source.index("static int64_t raster_edge("):source.index("\n#endif\n\nstatic const FigureTriangle*")]
suffix = r'''
static uint8_t fb[FRAME_STRIDE*FRAME_HEIGHT],tex[1024],mip[1024];
static TextureReference texture={{(uintptr_t)tex,(uintptr_t)mip}};
static unsigned expected(unsigned bg,unsigned fg){return fg?((3*bg+fg+1)/4?:1):bg;}
static uint32_t screen(int x,int y){return (uint16_t)x|((uint32_t)(uint16_t)y<<16);}
static void submit(DrawEntry e,int z){engine_set_depth(z,z,z);g_raster_depth=g_pending_depth;engine_raster(&e);}
static DrawEntry triangle(unsigned mode,unsigned color,int ax,int ay,int bx,int by,int cx,int cy){
 return (DrawEntry){.mode=mode,.colour=color,.s={screen(ax,ay),screen(bx,by),screen(cx,cy)},.texture=&texture};
}
static void quad(unsigned mode,unsigned color,int lo,int hi,int z,bool reverse){
 DrawEntry a=triangle(mode,color,lo,lo,hi,lo,hi,hi),b=triangle(mode,color,lo,lo,hi,hi,lo,hi);
 if(reverse){uint32_t t=a.s[1];a.s[1]=a.s[2];a.s[2]=t;t=b.s[1];b.s[1]=b.s[2];b.s[2]=t;}
 if(reverse){submit(b,z);submit(a,z);}else{submit(a,z);submit(b,z);}
}
int main(void){
 Rgba palette[256];for(unsigned i=0;i<256;i++)palette[i]=(Rgba){i,i,i,255};
 palette_build_quarter_blend(shade_quarter,palette);
 for(unsigned bg=0;bg<256;bg++)for(unsigned fg=0;fg<256;fg++)assert(shade_quarter[bg*256+fg]==expected(bg,fg));
 /* A10x10 quad has exactly100 fragments even when pixel centres lie on the
  * shared diagonal. Double blending would produce a visible diagonal seam. */
 for(unsigned depth=0;depth<2;depth++)for(unsigned perspective=0;perspective<2;perspective++)
 for(unsigned reverse=0;reverse<2;reverse++)for(unsigned transparent=0;transparent<2;transparent++){
  memset(fb,40,sizeof fb);memset(tex,transparent?0:200,sizeof tex);memset(mip,transparent?0:200,sizeof mip);
  g_engine_fb=fb;g_depth_enabled=depth;g_perspective_enabled=perspective;engine_depth_begin();
  quad(RM_BLEND_QUARTER,0,8,168,2000,reverse);
  if(depth){assert(g_depth_effect_count==2);for(unsigned i=0;i<sizeof fb;i++)assert(fb[i]==40);}
  engine_depth_end();unsigned changed=0;
  for(unsigned y=0;y<FRAME_HEIGHT;y++)for(unsigned x=0;x<FRAME_WIDTH;x++){
   unsigned want=!transparent&&x<10&&y<10?expected(40,200):40;
   assert(fb[y*FRAME_STRIDE+x]==want);changed+=fb[y*FRAME_STRIDE+x]!=40;
   if(depth)assert(g_depth_buffer[y*FRAME_WIDTH+x]==0);
  }
  assert(changed==(transparent?0:100));
 }
 /* Defer water until opaque actors/world finish. It must not cover a nearer
  * surface or write depth, regardless of their submission order. */
 for(unsigned reverse=0;reverse<2;reverse++){
  memset(fb,7,sizeof fb);memset(tex,200,sizeof tex);memset(mip,200,sizeof mip);
  g_engine_fb=fb;g_depth_enabled=true;engine_depth_begin();
  if(reverse)quad(RM_FLAT,220,88,168,1000,false);
  quad(RM_BLEND_QUARTER,0,8,168,2000,false);
  quad(RM_FLAT,40,8,168,3000,false);
  if(!reverse)quad(RM_FLAT,220,88,168,1000,false);
  uint16_t background=g_depth_buffer[2*FRAME_WIDTH+2],foreground=g_depth_buffer[7*FRAME_WIDTH+7];
  engine_depth_end();assert(fb[2*FRAME_STRIDE+2]==expected(40,200));assert(fb[7*FRAME_STRIDE+7]==220);
  assert(g_depth_buffer[2*FRAME_WIDTH+2]==background&&g_depth_buffer[7*FRAME_WIDTH+7]==foreground);
 }
 /* Variable-depth texture mapping must remain perspective-correct in the
  * blended mode too. Check samples against independent rational barycentrics,
  * allowing only the renderer's existing1-model-UV-unit span error. */
 memset(fb,40,sizeof fb);
 for(unsigned y=0;y<32;y++)for(unsigned x=0;x<32;x++)tex[y*32+x]=1+(x*7)%253;
 g_engine_fb=fb;g_depth_enabled=true;g_perspective_enabled=true;engine_depth_begin();
 DrawEntry e=triangle(RM_BLEND_QUARTER,0,8,8,1608,8,8,1608);e.uv[1]=512;e.uv[2]=512u<<16;
 engine_set_depth(2000,4000,8000);g_raster_depth=g_pending_depth;
 uint32_t depths[3];memcpy(depths,g_pending_depth,sizeof depths);engine_raster(&e);engine_depth_end();
 for(int y=0;y<FRAME_HEIGHT;y++)for(int x=0;x<FRAME_WIDTH;x++){
  if(x+y>=100){assert(fb[y*FRAME_STRIDE+x]==40);continue;}
  long double b=x/100.0L,c=y/100.0L,a=1-b-c;
  long double u=b*depths[1]*512/(a*depths[0]+b*depths[1]+c*depths[2]);
  long low=floorl((u-1.001L)/16),high=floorl((u+1.001L)/16);bool match=false;
  for(long sample=low;sample<=high;sample++)match|=fb[y*FRAME_STRIDE+x]==expected(40,1+(((unsigned)sample&31)*7)%253);
  assert(match);
 }
 puts("65536 palette pairs exact; water is blended once per shared-edge pixel, transparent texels preserve background, opaque foreground occludes water, no water depth writes; all depth/perspective/winding controls pass");
}
'''

(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix="water-blend-", dir=ROOT / "tmp") as directory:
    path = Path(directory)
    (path / "test.c").write_text(prefix + control + production + suffix)
    try:
        subprocess.run([
            "docker", "run", "--rm", "--name", path.name, "--user", "1000:1000",
            "-v", f"{ROOT}:/work:ro", "-v", f"{path}:/test", "-w", "/test",
            "smg-runtime-tests", "sh", "-c",
            "cc -O2 -fno-strict-aliasing -fsanitize=address,undefined -no-pie -I/work test.c "
            "/work/tools/asset_compiler/palette.c /work/tools/asset_compiler/common.c -lm -o test && ./test && "
            "cc -O2 -fno-strict-aliasing -fsanitize=address,undefined -no-pie -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 -I/work test.c "
            "/work/tools/asset_compiler/palette.c /work/tools/asset_compiler/common.c -lm -o native && ./native",
        ], check=True, timeout=60)
    finally:
        subprocess.run(["docker", "rm", "-f", path.name],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
