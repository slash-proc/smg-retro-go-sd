#!/usr/bin/env python3
"""Check texture layout independently and compare complete multi-triangle output.

The reference shader substitutes the previous row/stride calculation; all
other raster work stays shared so this isolates the addressing change.
"""
import ast
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
tree = ast.parse((ROOT / 'tools/test_raster.py').read_text())
prefix = next(ast.literal_eval(n.value) for n in tree.body if isinstance(n, ast.Assign)
              and any(isinstance(t, ast.Name) and t.id == 'prefix' for t in n.targets))
prefix += '#include <assert.h>\n'
source = (ROOT / 'src/graphics/engine.c').read_text()
production = source[source.index('static int64_t raster_edge('):source.index('\nvoid engine_raster(const DrawEntry* entry)')]
begin = production.index('__attribute__((always_inline)) static inline void engine_raster_inner_impl(')
end = production.index('__attribute__((noinline)) static void engine_raster_inner_affine(', begin)
reference = production[begin:end].replace('engine_raster_inner_impl(', 'reference_impl(', 1)
reference = '#define RASTER_ZERO_TEXEL(source) ((void)(source))\n' + reference
lookup = 'texture[raster_texture_offset(uw, vw, texture_row_mask)]'
assert reference.count(lookup) == 1
reference = reference.replace(lookup, 'texture[(mip1 ? ((vw >> 21) & 15) * 64 : ((vw >> 20) & 31) * 32) + ((uw >> 20) & 31)]')
suffix = r'''
static unsigned oracle(uint32_t u,uint32_t v,bool mip){
 /* Asset layout: V rows, U columns; mip1 stores half as many rows, with padding. */
 unsigned column=(u/1048576)%32;
 unsigned row=(v/(mip?2097152:1048576))%(mip?16:32);
 return row*(mip?64:32)+column;
}
static void address(uint32_t u,uint32_t v,bool mip){
 unsigned actual=raster_texture_offset(u,v,mip?0x3c0:0x3e0);
 assert(actual==oracle(u,v,mip)&&actual<1024);
}
int main(void){
 for(unsigned mip=0;mip<2;mip++)for(unsigned row=0;row<32;row++)for(unsigned col=0;col<32;col++){
  uint32_t u=col<<20,v=row<<20;address(u,v,mip);
  /* Toggle every irrelevant input bit independently, including wrap/sign bits. */
  for(unsigned bit=0;bit<32;bit++){
   if(bit<20||bit>=25)address(u^(1u<<bit),v,mip);
   if(bit<(mip?21:20)||bit>=25)address(u,v^(1u<<bit),mip);
  }
 }
 for(unsigned i=0;i<1000000;i++){uint32_t u=next(),v=next();address(u,v,i&1);}
 static uint8_t tex[1024],mip[1024],a[FRAME_STRIDE*FRAME_HEIGHT+64],b[sizeof a],add[65536];
 static uint16_t depths[FRAME_WIDTH*FRAME_HEIGHT],rows[256];
 for(unsigned i=0;i<1024;i++){tex[i]=next();mip[i]=next();}
 for(unsigned i=0;i<65536;i++){add[i]=next();shade_quarter[i]=next();}
 for(unsigned i=0;i<256;i++){rows[i]=next()&255;shade_darken[i]=next();g_shade_water[i]=next();}
 /* Named independent GBA/asset contract: U=2,V=7 must not be transposed. */
 assert(raster_texture_offset(2u<<20,7u<<20,0x3e0)==7*32+2);
 assert(raster_texture_offset(2u<<20,7u<<20,0x3c0)==3*64+2);
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
  for(unsigned candidate=0;candidate<2;candidate++){
   uint8_t*fb=candidate?b:a;memset(fb,group&255,sizeof a);g_engine_fb=fb+32;
   memset(g_depth_buffer,group%5?0:41,sizeof g_depth_buffer);
   for(unsigned j=0;j<4;j++){
    const DrawEntry* e=&entries[j];g_raster_depth=group%13?z[j]:NULL;
    bool perspective=g_perspective_active&&g_raster_depth&&
      (e->mode==RM_TEX||e->mode==RM_ALPHA||e->mode==RM_BLEND_QUARTER)&&
      g_raster_depth[0]&&g_raster_depth[1]&&g_raster_depth[2];
    if(candidate)engine_raster_inner(e);
    else reference_impl(e,perspective,e->mode==RM_TEX&&g_opaque_raster_enabled,-1);
   }
   if(!candidate)memcpy(depths,g_depth_buffer,sizeof depths);
  }
  if(memcmp(a,b,sizeof a)||memcmp(depths,g_depth_buffer,sizeof depths)){
   fprintf(stderr,"texture addressing differs in scene%u depth%d perspective%d opaque%d\n",group,g_depth_active,g_perspective_active,g_opaque_raster_enabled);abort();
  }
 }
 printf("Both mip layouts, every row/column and irrelevant bit, 1000000 full32 UV pairs exact; 2500 four-triangle scenes preserve all pixels/depth at %dx%d across materials/controls/wrapping/extremes\n",FRAME_WIDTH,FRAME_HEIGHT);
}
'''
(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='texture-address-', dir=ROOT / 'tmp') as directory:
    work = Path(directory)
    (work / 'test.c').write_text(prefix + production + reference + suffix)
    try:
        subprocess.run(['docker', 'run', '--rm', '--name', work.name, '--user', '1000:1000',
                        '-v', f'{work}:/test', '-w', '/test', 'smg-runtime-tests', 'sh', '-c',
                        'cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie test.c -o test && ./test && '
                        'cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 test.c -o native && ./native'], check=True, timeout=120)
    finally:
        subprocess.run(['docker', 'rm', '-f', work.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
