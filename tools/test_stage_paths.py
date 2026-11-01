#!/usr/bin/env python3
"""Compare fast stage dispatch against the unchanged production general emitter.

Builds ASan fixtures in Docker; checks arena contents, bin order, reciprocal
depth and saturation across randomized near/far and special-material jobs.
This proves dispatch equivalence, not the correctness of the original assets.
"""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
engine = (ROOT / 'src/graphics/engine.c').read_text()
header = (ROOT / 'src/graphics/engine.h').read_text()
frame = (ROOT / 'src/graphics/frame.c').read_text()
math = (ROOT / 'src/math/vector_math.c').read_text()

def section(text, begin, end):
    at = text.index(begin)
    return text[at:text.index(end, at)]

prefix = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "src/assets/asset_format.h"
#define __GNW__ 1
#define EWRAM_BSS
#define IWRAM_CODE
#define IWRAM_FN __attribute__((noinline))
#define FRAME_WIDTH 160
#define FRAME_HEIGHT 120
#define FX_ONE 256
#define FXC(f) ((int32_t)((f)*256))
#define CLIP_SIDES 15
#define CLIP_GUARD 16
#define CLIP_NEAR_Z 160
#define MAX_TRIANGLE_DEPTH_SPAN FXC(4.0f)
#define BIN_COUNT 128
#define STAGE_MAX_TEXTURES 1024
#define SMG_COUNT(field) (counters.field++)
#define SMG_PEAK(field,v) do {if(counters.field<(unsigned)(v))counters.field=(v);}while(0)
typedef int32_t fx;
typedef struct {fx x,y;} v2;
typedef struct {fx x,y,z;} v3;
typedef struct {v3 position;uint8_t clip,mark;uint16_t pad;} FramePoint;
static v2 V2(fx x,fx y){return (v2){x,y};}
static v3 V3(fx x,fx y,fx z){return (v3){x,y,z};}
static fx fx_mul(fx a,fx b){return a*b>>8;}
static fx fx_min(fx a,fx b){return a<b?a:b;}
static fx fx_max(fx a,fx b){return a>b?a:b;}
static int32_t lut_project[65536];
static uint8_t lut_zbin[2048];
static uint16_t g_triangle_bin_heads[2*BIN_COUNT];
static TextureReference g_texture_cache_reference[4],raw_textures[4];
static bool raw_present[4];
static struct {unsigned emitted_fast,emitted_slow,arena_attempts,arena_dropped,arena_peak,subdivisions;} counters;
static struct {StageHeader header; int16_t scroll[8];} stage_storage;
#define stage_header stage_storage.header
static StageHeader far_header;
static struct {struct {const StageHeader* header;} stage;int camera_underwater;fx time;} g_game;
static struct {const StageHeader* header;} g_far_stage;
static const TextureReference* stage_far_raw_texture(unsigned texture){return raw_present[texture]?&raw_textures[texture]:NULL;}
static void engine_depth_end(void);
'''
# Host pointers differ from the target ABI; the SDK build retains layout asserts.
body = section(engine, '#ifndef SMG_ARENA_CAPACITY', 'static int64_t raster_edge(') + '\n#endif\n'
body = re.sub(r'_Static_assert\(offsetof\(EmitJob, far\).*?"raster.s assumes the EmitJob layout"\);', '', body, flags=re.S)
types = section(header, 'enum { RM_FLAT', 'extern const uint16_t engine_recip_table')
types += section(header, 'typedef struct {\n    const StageTriangle* tris;', '} EmitJob;') + '} EmitJob;\n'
types += 'void engine_emit_stage(const EmitJob*, const uint16_t*, int);\n'
projection = frame[:frame.index('static inline uint8_t frame_get')]
mask = section(math, 'static inline uint32_t view_clip_mask', '\nstatic v2 fx_sin_cos')
bin_fn = section((ROOT/'src/graphics/visibility.c').read_text(), 'IWRAM_CODE static int depth_bin_for_z', '\n#if defined(__GNW__)')
stub = 'static void engine_raster(const DrawEntry* entry){(void)entry;abort();}\n'
suffix = r'''
static uint32_t seed=0xc0ffee42;
static uint32_t rnd(void){seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;return seed;}
static void reset(unsigned initial){
 memset(g_arena,0,sizeof g_arena);memset(g_arena_depth,0,sizeof g_arena_depth);
 memset(g_triangle_bin_heads,255,sizeof g_triangle_bin_heads);memset(&counters,0,sizeof counters);
 g_arena_count=initial;g_engine_bin_offset=0;g_depth_active=g_depth_enabled;
}
static void check_depth_collection(void){
 for(unsigned mode=0;mode<4;mode++){
  g_depth_enabled=mode&1;g_perspective_active=mode&2;reset(0);
  engine_set_depth(0,2000,4000);
  engine_push(1,0,0,0,0,0,0,RM_FLAT,7,NULL);
  uint32_t expected[3]={(1u<<30)/CLIP_NEAR_Z,(1u<<30)/2000,(1u<<30)/4000};
  for(unsigned i=0;i<3;i++)if(g_arena_depth[0][i]!=(mode?expected[i]:0))abort();
 }
 g_depth_enabled=false;g_perspective_active=false;
 puts("Reciprocal depth collection: near clamp and exact values, depth-only/perspective-only/both/neither pass");
}
static DrawEntry reference[ENGINE_ARENA_MAX];
static uint32_t reference_depth[ENGINE_ARENA_MAX][3];
static void check_packed_materials(void){
 FramePoint points[3]={{{0,0,2000},0,0,0},{{100,0,2000},0,0,0},{{0,100,2000},0,0,0}};
 uint32_t screens[3]={0,160,160u<<16};uint16_t visible[1]={0};
 TextureReference textures[1]={0};uint8_t average[1];
 StageTriangle triangle={.v={{.point=0},{.point=1},{.point=2}}};
 EmitJob j={.tris=&triangle,.points=points,.screen=screens,.textures=textures,
   .average=average,.far=1,.cut_bin=127,.slow=emit_stage_slow};
 g_emit_job=&j;g_stage_fast_enabled=true;far_header.surface_features=STAGE_PACKED_FAR_TEXTURES;
 for(unsigned alpha=0;alpha<2;alpha++)for(unsigned avg=0;avg<2;avg++)for(unsigned raw=0;raw<2;raw++){
  triangle.flags=alpha?TRI_ALPHA:0;average[0]=avg?7:0;raw_present[0]=raw;
  reset(0);engine_emit_stage(&j,visible,1);
  unsigned expected=raw||(!alpha&&avg);
  if(g_arena_count!=(int)expected||counters.emitted_fast!=1)abort();
  if(expected){
   unsigned mode=alpha?RM_ALPHA:avg?RM_FLAT:RM_TEX;
   if(g_arena[0].mode!=mode||g_arena[0].texture!=(raw?&raw_textures[0]:NULL))abort();
  }
 }
 puts("Packed far material contract: raw/average/alpha combinations preserve flat fallback and cutout rejection");
}
static void check_subdivision_uv(void){
 /* Constant UVs must remain constant after splitting. These coordinates are
  * within the asset ABI; adding two of them need not fit signed16. Comparing
  * fast/slow dispatch alone cannot detect their shared midpoint bug. */
 const int values[]={30000,-30000,32767,-32768};
 TextureReference texture={0};
 EmitJob j={.cut_bin=0,.flat_bin=-1};
 for(unsigned test=0;test<4;test++){
  int uv=values[test]; SplitVertex tri[3];
  emit_split_vertex(&tri[0],V3(-100,-100,1000),uv,uv);
  emit_split_vertex(&tri[1],V3(1000,-500,10000),uv,uv);
  emit_split_vertex(&tri[2],V3(0,1000,1000),uv,uv);
  reset(0);emit_subdivided(&j,tri,false,0,&texture);
  if(!counters.subdivisions || !g_arena_count)abort();
  for(int face=0;face<g_arena_count;face++)for(unsigned k=0;k<3;k++){
   uint32_t packed=g_arena[face].uv[k];
   if((int16_t)packed!=uv || (int16_t)(packed>>16)!=uv){
    fprintf(stderr,"Subdivision changed constant UV %d into %d,%d\n",uv,(int16_t)packed,(int16_t)(packed>>16));abort();
   }
  }
 }
 puts("Independent subdivision UV contract: positive/negative large constant coordinates and signed16 endpoints remain constant");
}
int main(void){
 check_depth_collection();
 for(unsigned i=0;i<65536;i++)lut_project[i]=(1u<<20)/(i?i:1);
 for(unsigned i=0;i<2048;i++)lut_zbin[i]=i*127/2047;
 check_subdivision_uv();
 g_game.stage.header=&stage_header;g_far_stage.header=&far_header;
 check_packed_materials();
 stage_header.texture_scroll=sizeof(StageHeader);stage_header.water_level=0;
 stage_header.water_radius_squared=10000;stage_header.water_min_z=-2000;
 for(unsigned i=0;i<8;i++)stage_storage.scroll[i]=(i&1)?-19:37;
 FramePoint points[12];uint32_t screens[12];StageTriangle triangles[4];
 int16_t world[12][3];uint16_t home[12];int16_t delta[2]={0,3};
 TextureReference textures[4]={0};uint8_t average[4],cache[4];uint16_t water[4]={3,2,1,0};
 uint16_t visible[4]={2,0,3,1};unsigned long fast=0,slow=0,emitted=0,saturated=0;
 for(unsigned n=0;n<40000;n++){
  memset(triangles,0,sizeof triangles);memset(points,0,sizeof points);
  for(unsigned i=0;i<12;i++){
   int z=160+rnd()%24000,x=(int)(rnd()%(8*z))-4*z,y=(int)(rnd()%(8*z))-4*z;
   if(n%3){z=1500+rnd()%3000;x=(int)(rnd()%6000)-3000;y=(int)(rnd()%6000)-3000;}
   if(n%23==0)z=CLIP_NEAR_Z+(int)(n%3)-1;
   points[i].position=V3(x,y,z);points[i].clip=view_clip_mask(points[i].position);
   screens[i]=points[i].clip&CLIP_GUARD?0:screen_pack_position(project_to_screen(points[i].position));
   for(unsigned k=0;k<3;k++)world[i][k]=(int16_t)(rnd()%8000-4000);
   home[i]=1;
  }
  for(unsigned i=0;i<4;i++){
   for(unsigned k=0;k<3;k++)triangles[i].v[k]=(ModelVertex){i*3+k,(int16_t)rnd(),(int16_t)rnd()};
   triangles[i].texture=i|(rnd()&0xfc00);triangles[i].flags=(n%5?0:SURF_ANIM_UV)|(rnd()%2?TRI_ALPHA:0)|(n%7?0:TRI_WATER_TINT)|(n%11?0:TRI_BLEND_QUARTER|TRI_ALPHA);
   average[i]=rnd()%3?rnd():0;cache[i]=rnd()%2?i+1:0;raw_present[i]=rnd()%2;
  }
  /* Homed jobs exercise positive and negative slot deltas without aliasing. */
  if(n%2){
   for(unsigned k=0;k<3;k++)triangles[3].v[k].point=k;
   delta[1]=(n&2)?-3:3;
   if(delta[1]<0)for(unsigned i=0;i<4;i++)for(unsigned k=0;k<3;k++)triangles[i].v[k].point+=3;
  }
  EmitJob j={.tris=triangles,.points=points,.screen=screens,.textures=textures,.average=average,
   .cache_slots=n%4?cache:NULL,.home=n%2?home:NULL,.delta=delta,.stage_points=world,
   .far=n%3==0,.cut_bin=n%128,.flat_bin=(int)(n%129)-1,.flat_small=n%2,.slow=emit_stage_slow,
   .water=n%11==0?1+n%3:0,.water_textures=n%13?water:NULL,.under_bins=n%7==0};
  far_header.surface_features=n%4==0?STAGE_PACKED_FAR_TEXTURES:0;
  g_game.camera_underwater=n%2;g_game.time=n*10;g_emit_job=&j;g_depth_enabled=n%2;g_perspective_active=n%3!=0;
  unsigned initial=n%31==0?ENGINE_ARENA_MAX-1:0;
  reset(initial);for(unsigned i=0;i<4;i++)emit_stage_face(&j,visible[i]);
  unsigned ref_count=g_arena_count,attempts=counters.arena_attempts,dropped=counters.arena_dropped,splits=counters.subdivisions;
  uint16_t bins[2*BIN_COUNT];memcpy(bins,g_triangle_bin_heads,sizeof bins);
  memcpy(reference,g_arena,sizeof reference);memcpy(reference_depth,g_arena_depth,sizeof reference_depth);
  reset(initial);g_stage_fast_enabled=n%17!=0;engine_emit_stage(&j,visible,4);
  if(g_arena_count!=ref_count||memcmp(reference,g_arena,sizeof reference)||
     memcmp(reference_depth,g_arena_depth,sizeof reference_depth)||memcmp(bins,g_triangle_bin_heads,sizeof bins)||
     counters.emitted_fast+counters.emitted_slow!=4||counters.arena_attempts!=attempts||counters.arena_dropped!=dropped||counters.subdivisions!=splits){
   fprintf(stderr,"stage job %u diverged (entries %d vs %u)\n",n,g_arena_count,ref_count);return 1;
  }
  fast+=counters.emitted_fast;slow+=counters.emitted_slow;emitted+=ref_count-initial;saturated+=dropped;
 }
 if(!fast||!slow||!emitted||!saturated)abort();
 printf("40000 stage jobs: %lu fast / %lu slow, %lu emitted, %lu saturation drops; exact arena, bin order, depth, material and subdivision equivalence\n",fast,slow,emitted,saturated);
 return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='stage-paths-',dir=ROOT/'tmp') as directory:
    path=Path(directory)
    (path/'test.c').write_text(prefix+types+stub+projection+mask+bin_fn+body+suffix)
    try:
        subprocess.run(['docker','run','--rm','--name',path.name,'--user','1000:1000',
            '-v',f'{ROOT}:/work:ro','-v',f'{path}:/test','-w','/test',
            '-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c',
            'cc -I/work -O3 -fno-strict-aliasing -no-pie -fsanitize=address test.c -o test && ./test'],check=True,timeout=60)
    finally:
        subprocess.run(['docker','rm','-f',path.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
