#!/usr/bin/env python3
"""Check guarded setup conversions and exact renderer equivalence in Docker."""
import ast
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
tree = ast.parse((ROOT / 'tools/test_raster.py').read_text())
prefix = next(ast.literal_eval(n.value) for n in tree.body if isinstance(n, ast.Assign)
              and any(isinstance(t, ast.Name) and t.id == 'prefix' for t in n.targets))
prefix += '#include <assert.h>\n#include <limits.h>\n#include <float.h>\n'
source = (ROOT / 'src/graphics/engine.c').read_text()
production = source[source.index('static int64_t raster_edge('):source.index('\nvoid engine_raster(const DrawEntry* entry)')]
suffix = r'''
static uint64_t seed=UINT64_C(0x253178ace045679b),fits,wide;
static uint64_t random64(void){seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;return seed;}
static void check(int64_t a,int64_t b,int64_t c){
 int64_t values[3]={a,b,c};bool narrow=raster_coords_fit_s32(a,b,c);fits+=narrow;wide+=!narrow;
 for(unsigned i=0;i<3;i++){
  double expected=(double)values[i],actual=narrow?(double)(int32_t)values[i]:(double)values[i];
  assert(!memcmp(&actual,&expected,sizeof actual));
 }
}
int main(void){
 _Static_assert(FLT_RADIX==2&&DBL_MANT_DIG>=32,"Signed32 integers must be exactly representable");
 const int64_t edges[]={INT64_MIN,INT64_MIN+1,INT32_MIN-INT64_C(1),INT32_MIN,INT32_MIN+INT64_C(1),-1,0,1,INT32_MAX-INT64_C(1),INT32_MAX,INT32_MAX+INT64_C(1),UINT32_MAX,INT64_MAX-1,INT64_MAX};
 for(unsigned a=0;a<sizeof edges/sizeof *edges;a++)for(unsigned b=0;b<sizeof edges/sizeof *edges;b++)for(unsigned c=0;c<sizeof edges/sizeof *edges;c++)check(edges[a],edges[b],edges[c]);
 for(unsigned test=0;test<1000000;test++){
  int64_t values[3];
  for(unsigned i=0;i<3;i++){
   uint64_t bits=random64();memcpy(&values[i],&bits,8);
   if(test%3==0)values[i]=(int32_t)bits;
   if(test%3==1)values[i]=(int64_t)(bits&UINT64_C(0x1ffffffff))*(i&1?-1:1);
  }
  check(values[0],values[1],values[2]);
 }
 assert(fits>300000&&wide>300000);
 static uint8_t a[FRAME_STRIDE*FRAME_HEIGHT+64],b[sizeof a],tex[1024],mip[1024],add[65536];
 static uint16_t depths[FRAME_WIDTH*FRAME_HEIGHT],rows[256];
 for(unsigned i=0;i<1024;i++){tex[i]=next();mip[i]=next();}
 for(unsigned i=0;i<65536;i++){add[i]=next();shade_quarter[i]=next();}
 for(unsigned i=0;i<256;i++){rows[i]=next()&255;shade_darken[i]=next();g_shade_water[i]=next();}
 TextureReference texture={{(uintptr_t)tex,(uintptr_t)mip}};g_additive_rows=rows;g_additive_table=add;
 for(unsigned group=0;group<2500;group++){
  DrawEntry entries[4]={0};uint32_t z[4][3];
  g_perspective_active=group&1;g_depth_active=(group/2)&1;
  g_opaque_raster_enabled=(group/4)&1;g_raster_setup_recip_enabled=(group/8)&1;
  g_perspective_precheck_enabled=(group/16)&1;g_perspective_reuse_enabled=(group/32)&1;
  texture.mip[1]=group%7?(uintptr_t)mip:(uintptr_t)tex;
  for(unsigned j=0;j<4;j++){
   DrawEntry*e=&entries[j];e->mode=j<2?RM_TEX:(group+j)%8;e->colour=next();e->texture=group%23?&texture:NULL;
   for(unsigned k=0;k<3;k++){
    int x=(int)(next()%(FRAME_WIDTH*24))-FRAME_WIDTH*4,y=(int)(next()%(FRAME_HEIGHT*24))-FRAME_HEIGHT*4;
    if(group%5==0){x=(int16_t)next();y=(int16_t)next();}
    if(group%29==0)y=123;
    e->s[k]=(uint16_t)x|((uint32_t)(uint16_t)y<<16);e->uv[k]=next();
    z[j][k]=group%17?1+next()%8388608:123456;
    if(group%31==0&&k==2)z[j][k]=0;
   }
  }
  for(unsigned enabled=0;enabled<2;enabled++){
   uint8_t*fb=enabled?b:a;memset(fb,group&255,sizeof a);g_engine_fb=fb+32;
   memset(g_depth_buffer,group%5?0:41,sizeof g_depth_buffer);g_raster_convert_narrow_enabled=enabled;
   for(unsigned j=0;j<4;j++){g_raster_depth=group%13?z[j]:NULL;engine_raster_inner(&entries[j]);}
   if(!enabled)memcpy(depths,g_depth_buffer,sizeof depths);
  }
  if(memcmp(a,b,sizeof a)||memcmp(depths,g_depth_buffer,sizeof depths)){
   fprintf(stderr,"conversion differs in scene%u depth%d perspective%d opaque%d\n",group,g_depth_active,g_perspective_active,g_opaque_raster_enabled);abort();
  }
 }
 printf("1002744 signed64 triples: %llu fast/%llu wide, bit-exact conversions incl boundaries/full64;2500 four-triangle scenes exact pixels/depth at %dx%d across all controls/materials/extremes\n",(unsigned long long)fits,(unsigned long long)wide,FRAME_WIDTH,FRAME_HEIGHT);
}
'''
(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='raster-convert-', dir=ROOT / 'tmp') as directory:
    work = Path(directory)
    (work / 'test.c').write_text(prefix + production + suffix)
    try:
        subprocess.run(['docker', 'run', '--rm', '--name', work.name, '--user', '1000:1000',
                        '-v', f'{work}:/test', '-w', '/test', 'smg-runtime-tests', 'sh', '-c',
                        'cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie test.c -o test && ./test && '
                        'cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 test.c -o native && ./native'], check=True, timeout=120)
    finally:
        subprocess.run(['docker', 'rm', '-f', work.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
