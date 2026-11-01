#!/usr/bin/env python3
"""Compare opaque specialization with the complete same-build raster path.

Checks complete pixels/depth and diagnostic work, preserving equal-depth,
cutout and overlay ordering across multi-triangle scenes. Independent GBA,
rational perspective and water contracts remain the correctness oracles.
"""
import ast
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
tree = ast.parse((ROOT / 'tools/test_raster.py').read_text())
prefix = next(ast.literal_eval(n.value) for n in tree.body if isinstance(n, ast.Assign)
              and any(isinstance(t, ast.Name) and t.id == 'prefix' for t in n.targets))
prefix += '''
#include <assert.h>
#include "src/graphics/perf.h"
#ifndef SMG_RASTER_WORK_STATS
#define SMG_RASTER_WORK_STATS 1
#endif
#if !SMG_RASTER_WORK_STATS
static SmgRasterWorkStats smg_raster_work_stats; /* Unused comparison dummy. */
#endif
'''
source = (ROOT / 'src/graphics/engine.c').read_text()
production = source[source.index('static int64_t raster_edge('):source.index('\nvoid engine_raster(const DrawEntry* entry)')]
suffix = r'''
int main(void){
 static uint8_t initial[FRAME_STRIDE*FRAME_HEIGHT+64],a[sizeof initial],b[sizeof initial];
 static uint16_t initial_depth[FRAME_WIDTH*FRAME_HEIGHT],depth[FRAME_WIDTH*FRAME_HEIGHT];
 static uint8_t tex[1024],mip[1024],add[65536];static uint16_t rows[256];
 for(unsigned i=0;i<1024;i++){tex[i]=next();mip[i]=next();}
 for(unsigned i=0;i<65536;i++){add[i]=next();shade_quarter[i]=next();}
 for(unsigned i=0;i<256;i++){rows[i]=next()&255;shade_darken[i]=next();g_shade_water[i]=next();}
 TextureReference texture={{(uintptr_t)tex,(uintptr_t)mip}};g_additive_rows=rows;g_additive_table=add;
 unsigned opaque=0;
 for(unsigned group=0;group<3000;group++){
  DrawEntry entries[4]={0};uint32_t z[4][3];SmgRasterWorkStats work={0};
  g_perspective_active=group&1;g_depth_active=(group/2)&1;
  g_raster_setup_recip_enabled=(group/4)&1;
  g_perspective_reuse_enabled=(group/8)&1;g_perspective_precheck_enabled=(group/16)&1;
  texture.mip[1]=group%7?(uintptr_t)mip:(uintptr_t)tex;
  for(unsigned tri=0;tri<4;tri++){
   DrawEntry*e=&entries[tri];e->mode=tri<2?RM_TEX:(group+tri)%8;
   e->colour=next();e->texture=group%23?&texture:NULL;
   opaque+=e->mode==RM_TEX;
   for(unsigned k=0;k<3;k++){
    int x=(int)(next()%(FRAME_WIDTH*24))-FRAME_WIDTH*4,y=(int)(next()%(FRAME_HEIGHT*24))-FRAME_HEIGHT*4;
    if(group%11==0){x=(int16_t)next();y=(int16_t)next();}
    if(group%29==0)y=123;
    e->s[k]=(uint16_t)x|((uint32_t)(uint16_t)y<<16);e->uv[k]=next();
    z[tri][k]=group%17?1+next()%8388608:123456;
    if(group%31==0&&k==2)z[tri][k]=0; /* affine fallback, never divide by zero */
   }
  }
  for(unsigned i=0;i<sizeof initial;i++)initial[i]=next();
  for(unsigned i=0;i<sizeof initial_depth/sizeof *initial_depth;i++)initial_depth[i]=group%5?0:next();
  for(unsigned candidate=0;candidate<2;candidate++){
   uint8_t* output=candidate?b:a;memcpy(output,initial,sizeof initial);g_engine_fb=output+32;
   memcpy(g_depth_buffer,initial_depth,sizeof initial_depth);memset(&smg_raster_work_stats,0,sizeof smg_raster_work_stats);
   g_opaque_raster_enabled=candidate;
   for(unsigned tri=0;tri<4;tri++){
    g_raster_depth=group%13?z[tri]:NULL;engine_raster_inner(&entries[tri]);
   }
   if(!candidate){memcpy(depth,g_depth_buffer,sizeof depth);work=smg_raster_work_stats;}
   else if(memcmp(a,b,sizeof a)||memcmp(depth,g_depth_buffer,sizeof depth)||memcmp(&work,&smg_raster_work_stats,sizeof work)){
    fprintf(stderr,"opaque specialization differs: group%u perspective%d depth%d\n",group,g_perspective_active,g_depth_active);abort();
   }
  }
 }
 printf("3000 four-triangle scenes (%u opaque triangles) match full pixels/depth/work at %dx%d across depth/mapping/reuse/precheck/setup controls, both mips, materials0..7, extremes, missing inputs and zero-depth fallback\n",opaque,FRAME_WIDTH,FRAME_HEIGHT);
}
'''

(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='opaque-raster-', dir=ROOT / 'tmp') as directory:
    path = Path(directory)
    (path / 'test.c').write_text(prefix + production + suffix)
    try:
        subprocess.run([
            'docker', 'run', '--rm', '--name', path.name, '--user', '1000:1000',
            '-v', f'{ROOT}:/work:ro', '-v', f'{path}:/test', '-w', '/test',
            'smg-runtime-tests', 'sh', '-c',
            'cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie -I/work test.c -o test && ./test && '
            'cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 -I/work test.c -o native && ./native && '
            'cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie -DSMG_RASTER_WORK_STATS=0 -I/work test.c -o test-no-stats && ./test-no-stats && '
            'cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie -DSMG_RASTER_WORK_STATS=0 -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 -I/work test.c -o native-no-stats && ./native-no-stats',
        ], check=True, timeout=120)
    finally:
        subprocess.run(['docker', 'rm', '-f', path.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
