#!/usr/bin/env python3
"""Verify constant-specialized paths match the current general raster path.

This regression checks an optimization, not original-GBA equivalence. The
independent rational, coverage, alpha/depth and GBA fixtures remain required.
"""
import ast
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
tree=ast.parse((ROOT/'tools/test_raster.py').read_text())
prefix=next(ast.literal_eval(n.value) for n in tree.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='prefix' for t in n.targets))
s=(ROOT/'src/graphics/engine.c').read_text();production=s[s.index('static int64_t raster_edge('):s.index('\nvoid engine_raster(const DrawEntry* entry)')]
suffix=r'''
int main(void){
 g_perspective_reuse_enabled=false; /* Compare the unchanged Y span policy. */
 static uint8_t a[FRAME_STRIDE*FRAME_HEIGHT+64],b[sizeof a],tex[1024],mip[1024],add[65536];
 static uint16_t depth[FRAME_WIDTH*FRAME_HEIGHT],rows[256];
 for(unsigned i=0;i<1024;i++){tex[i]=next();mip[i]=next();}
 for(unsigned i=0;i<65536;i++)add[i]=next();
 for(unsigned i=0;i<256;i++){rows[i]=next()&255;shade_darken[i]=next();g_shade_water[i]=next();}
 TextureReference texture={{(uintptr_t)tex,(uintptr_t)mip}};g_additive_rows=rows;g_additive_table=add;
 for(unsigned n=0;n<1500;n++){
  DrawEntry entries[4]={0};uint32_t depths[4][3];
  g_perspective_active=(n/2)%2;g_depth_active=(n/4)%2;
  g_raster_setup_recip_enabled=n%2;
  texture.mip[1]=n%7?(uintptr_t)mip:(uintptr_t)tex;
  for(unsigned t=0;t<4;t++){
   DrawEntry*e=&entries[t];e->mode=(n+t)%7;if(e->mode==RM_BLEND_QUARTER)e->mode=7;e->colour=next();e->texture=n%41?&texture:NULL;
   for(unsigned i=0;i<3;i++){
    int x=(int)(next()%(FRAME_WIDTH*24))-FRAME_WIDTH*4,y=(int)(next()%(FRAME_HEIGHT*24))-FRAME_HEIGHT*4;
    if(n%5==0){x=(int16_t)next();y=(int16_t)next();}
    if(n%19==0)y=123;
    e->s[i]=(uint16_t)x|((uint32_t)(uint16_t)y<<16);e->uv[i]=next();depths[t][i]=1+next()%8388608;
    if(n%17==0)depths[t][i]=123456;
   }
  }
  for(unsigned candidate=0;candidate<2;candidate++){
   uint8_t*fb=candidate?b:a;memset(fb,n&255,sizeof a);g_engine_fb=fb+32;memset(g_depth_buffer,n%9?0:19,sizeof g_depth_buffer);
   g_opaque_raster_enabled=candidate;
   for(unsigned t=0;t<4;t++){
    g_raster_depth=n%13?depths[t]:NULL;
    engine_raster_inner(&entries[t]);
   }
   if(!candidate)memcpy(depth,g_depth_buffer,sizeof depth);
  }
  if(memcmp(a,b,sizeof a)||memcmp(depth,g_depth_buffer,sizeof depth)){
   fprintf(stderr,"specialization differs in group%u perspective%d depth%d\n",n,g_perspective_active,g_depth_active);return 1;
  }
 }
 printf("1500 four-triangle groups match general and specialized pixels/depth at %dx%d: both perspective/depth/setup switches, all materials, winding, UV wrapping, mips, missing depths/textures and degenerates\n",FRAME_WIDTH,FRAME_HEIGHT);return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='raster-specialized-',dir=ROOT/'tmp') as directory:
 p=Path(directory);(p/'test.c').write_text(prefix+production+suffix)
 try:
  subprocess.run(['docker','run','--rm','--name',p.name,'--user','1000:1000','-v',f'{p}:/test','-w','/test','-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c','cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined,float-cast-overflow test.c -o test && ./test && cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined,float-cast-overflow -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 test.c -o test-native && ./test-native'],check=True,timeout=90)
 finally:subprocess.run(['docker','rm','-f',p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
