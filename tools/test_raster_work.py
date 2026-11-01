#!/usr/bin/env python3
"""Check optional raster counters against independent pixel/coverage enumeration."""
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
#define SMG_RASTER_WORK_STATS 1
'''
source = (ROOT / 'src/graphics/engine.c').read_text()
production = source[source.index('static int64_t raster_edge('):source.index('\nvoid engine_raster(const DrawEntry* entry)')]
suffix = r'''
static int64_t edge(int ax,int ay,int bx,int by,int x,int y){
 return (int64_t)(bx-ax)*(y-ay)-(int64_t)(by-ay)*(x-ax);
}
static bool owned(int ax,int ay,int bx,int by,int direction){
 int dx=direction*(bx-ax),dy=direction*(by-ay);
 return dy<0||(dy==0&&dx>0);
}
int main(void){
 uint8_t tex[1024],mip[1024],fb[FRAME_STRIDE*FRAME_HEIGHT],add[65536];uint16_t rows[256];
 TextureReference texture={{(uintptr_t)tex,(uintptr_t)mip}};
 uint32_t z[3]={262144,262144,262144};g_raster_depth=z;g_engine_fb=fb;
 g_additive_rows=rows;g_additive_table=add;memset(rows,0,sizeof rows);memset(add,17,sizeof add);
 const uint16_t stored[]={0,2047,2048,2049,4096};
 for(unsigned trial=0;trial<2000;trial++){
  DrawEntry e={.mode=trial%7,.colour=45,.texture=&texture};int x[3],y[3];
  for(unsigned k=0;k<3;k++){
   x[k]=(int)(next()%((FRAME_WIDTH+40)*16))-320;
   y[k]=(int)(next()%((FRAME_HEIGHT+40)*16))-320;
   e.s[k]=(uint16_t)x[k]|((uint32_t)(uint16_t)y[k]<<16);
   e.uv[k]=next();
  }
  unsigned source=(trial&1)?0:123;memset(tex,source,sizeof tex);memset(mip,source,sizeof mip);
  g_depth_active=(trial/7)&1;g_perspective_active=(trial/14)&1;
  memset(fb,9,sizeof fb);for(unsigned k=0;k<FRAME_WIDTH*FRAME_HEIGHT;k++)g_depth_buffer[k]=stored[k%5];
  memset(&smg_raster_work_stats,0,sizeof smg_raster_work_stats);smg_raster_work_stats.version=1;
  engine_raster_inner(&e);
  SmgRasterModeWork expected={0};
  int64_t area=edge(x[0],y[0],x[1],y[1],x[2],y[2]);int sign=area<0?-1:1;
  bool textured=e.mode!=RM_FLAT&&e.mode!=RM_TINT;
  bool perspective=g_perspective_active&&(e.mode==RM_TEX||e.mode==RM_ALPHA||e.mode==RM_BLEND_QUARTER);
  if(area)for(int py=0;py<FRAME_HEIGHT;py++){
   unsigned row=0;
   for(int px=0;px<FRAME_WIDTH;px++){
    bool inside=true;
    for(unsigned k=0;k<3;k++){
     unsigned j=(k+1)%3;int64_t w=sign*edge(x[k],y[k],x[j],y[j],px*16+8,py*16+8);
     if(w<0||(w==0&&e.mode==RM_BLEND_QUARTER&&!owned(x[k],y[k],x[j],y[j],sign)))inside=false;
    }
    if(!inside)continue;
    expected.covered_pixels++;row++;
    unsigned old=stored[(py*FRAME_WIDTH+px)%5],bias=e.mode==RM_DARKEN?2:0;
    if(g_depth_active&&2048+bias<old){expected.depth_rejected_pixels++;continue;}
    if(textured){expected.texture_samples++;expected.zero_texels+=!source;}
    bool writes=!textured||e.mode==RM_TEX||source;
    expected.colour_writes+=writes;
    expected.depth_writes+=g_depth_active&&writes&&(e.mode==RM_FLAT||e.mode==RM_TEX||e.mode==RM_ALPHA);
   }
   if(row&&perspective){
    expected.perspective_pixels+=row;unsigned spans=(row+SMG_PERSPECTIVE_SPAN_MAX-1)/SMG_PERSPECTIVE_SPAN_MAX;
    expected.perspective_spans+=spans;
    // Default endpoint reuse is off: every span divides at its start and
    // again at its endpoint unless the span contains one pixel. Subdivision
    // retries are excluded because these deterministic rows stay at the cap.
    expected.perspective_divisions+=2*spans-(row%SMG_PERSPECTIVE_SPAN_MAX==1);
   }
  }
  SmgRasterModeWork actual=smg_raster_work_stats.mode[e.mode];
  /* Degenerate/offscreen triangles return before publishing a triangle. */
  expected.triangles=actual.triangles;assert(actual.triangles<=1);
  if(memcmp(&actual,&expected,sizeof actual)){
   uint32_t* a=(uint32_t*)&actual,*b=(uint32_t*)&expected;
   for(unsigned k=0;k<sizeof actual/4;k++)if(a[k]!=b[k])fprintf(stderr,"trial %u mode %u field %u: %u != %u\n",trial,e.mode,k,a[k],b[k]);
   abort();
  }
 }
 puts("2000 independent fragment/depth/texture/write/span/division counter cases pass across all material modes, winding, clipping, depth and mapping controls");
}
'''

(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='raster-work-', dir=ROOT / 'tmp') as directory:
    path = Path(directory)
    (path / 'test.c').write_text(prefix + production + suffix)
    try:
        subprocess.run([
            'docker', 'run', '--rm', '--name', path.name, '--user', '1000:1000',
            '-v', f'{ROOT}:/work:ro', '-v', f'{path}:/test', '-w', '/test',
            'smg-runtime-tests', 'sh', '-c',
            'cc -O2 -fno-strict-aliasing -fsanitize=address,undefined -no-pie -I/work test.c -o test && ./test && '
            'cc -O2 -fno-strict-aliasing -fsanitize=address,undefined -no-pie -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 -I/work test.c -o native && ./native',
        ], check=True, timeout=60)
    finally:
        subprocess.run(['docker', 'rm', '-f', path.name],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
