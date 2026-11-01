#!/usr/bin/env python3
"""Verify production stage texture cache placement, fallback and migration.

Docker ASan/UBSan. Checks full 1024-byte texels and descriptor identities,
including both mips, stage changes, restore, invalid buffers and hidden glass.
"""
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
s=(ROOT/'src/world/stage.c').read_text();production=s[s.index('#define TEXCACHE_LOW_SLOTS'):s.index('static const uint32_t* stage_far_records(')]
water=s[s.index('static inline int stage_water_texture('):s.index('\nstatic EWRAM_BSS Stage g_far_stage;')]
prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <assert.h>
#include "assets/asset_format.h"
#define __GNW__ 1
#define EWRAM_BSS
#ifndef SMG_TEXTURE_CACHE_SLOTS
#define SMG_TEXTURE_CACHE_SLOTS 54
#endif
#define TEXCACHE_SLOTS SMG_TEXTURE_CACHE_SLOTS
#define STAGE_MAX_TEXTURES 80
static uint8_t g_texture_average[STAGE_MAX_TEXTURES];
typedef struct {const StageHeader*header;const int16_t(*points)[3];const StageTriangle*triangles;const TextureReference*textures;} Stage;
static struct {Stage stage;uint32_t actors[STAGE_MAX_TEXTURES];} g_game;
static void dma3_copy32(void*d,const void*s,uint32_t n){memcpy(d,s,n*4);}
static void dma3_fill32(void*d,const volatile uint32_t*v,uint32_t n){uint32_t*out=d;while(n--)*out++=*v;}
'''
suffix=r'''
static uint32_t dtcm[32+TEXCACHE_SLOTS*TEX_TEXELS/4];
static uint8_t texels[STAGE_MAX_TEXTURES][2][TEX_TEXELS];
static TextureReference textures[STAGE_MAX_TEXTURES];
static StageTriangle triangles[STAGE_MAX_TEXTURES];
static int16_t points[STAGE_MAX_TEXTURES*3][3];
static StageHeader header;
static unsigned checks;
static uint8_t*fast(void){return (uint8_t*)&dtcm[16];}
static void guards(void){for(unsigned i=0;i<16;i++)assert(dtcm[i]==0xa7a7a7a7&&dtcm[16+TEXCACHE_SLOTS*TEX_TEXELS/4+i]==0xa7a7a7a7);}
static void check_cache(bool use_fast,int hidden){
 assert(g_texture_dtcm_active==use_fast);
 for(unsigned t=0;t<STAGE_MAX_TEXTURES;t++){
  unsigned slot=g_texture_cache_slot[t];if(!slot)continue;slot--;
  uint8_t*dest=use_fast?fast()+slot*TEX_TEXELS:g_texture_cache[slot];
  assert(g_texture_cache_reference[slot].mip[0]==(uintptr_t)dest);
  if((int)slot==hidden){
   assert(g_texture_cache_reference[slot].mip[1]==(uintptr_t)dest);
   for(unsigned i=0;i<TEX_TEXELS;i++)assert(dest[i]==0);
  }else{
   assert(g_texture_cache_reference[slot].mip[1]==textures[t].mip[1]);
   assert(!memcmp(dest,texels[t][0],TEX_TEXELS));
  }
  checks++;
 }
 guards();
}
int main(void){
 assert((uintptr_t)dtcm<=UINT32_MAX&&(uintptr_t)texels<=UINT32_MAX);
 header.texture_count=STAGE_MAX_TEXTURES;header.triangle_count=STAGE_MAX_TEXTURES;
 for(unsigned t=0;t<STAGE_MAX_TEXTURES;t++){
  for(unsigned m=0;m<2;m++)for(unsigned i=0;i<TEX_TEXELS;i++)texels[t][m][i]=t*37+i*13+m*17;
  textures[t].mip[0]=(uintptr_t)texels[t][0];textures[t].mip[1]=(uintptr_t)texels[t][t%11?1:0];
  points[t*3+1][0]=16*(t+1);points[t*3+2][1]=16;
  triangles[t].texture=t;for(unsigned v=0;v<3;v++)triangles[t].v[v].point=t*3+v;
 }
 g_game.stage=(Stage){&header,points,triangles,textures};
 memset(dtcm,0xa7,sizeof dtcm);assert(smg_game_texture_cache_bytes()==TEXCACHE_SLOTS*TEX_TEXELS);
 for(unsigned bad=0;bad<3;bad++){
  smg_game_set_texture_cache(bad==0?NULL:bad==1?fast()+1:fast(),bad==2?smg_game_texture_cache_bytes()-1:smg_game_texture_cache_bytes());
  stage_cache_textures(&g_game.stage);check_cache(false,-1);assert(!g_texture_cache_dtcm);
 }
 smg_game_set_texture_cache(fast(),smg_game_texture_cache_bytes());stage_cache_textures(&g_game.stage);check_cache(true,-1);
 unsigned first_cached=STAGE_MAX_TEXTURES-TEXCACHE_SLOTS;
 for(unsigned t=0;t<STAGE_MAX_TEXTURES;t++)assert(g_texture_cache_slot[t]==(t<first_cached?0:STAGE_MAX_TEXTURES-t));
 for(unsigned n=0;n<20;n++){
  g_texture_dtcm_enabled=n%2;stage_sync_texture_cache();check_cache(n%2,-1);
  // Simulate loss of cache contents during full-screen UI/sleep, then restore.
  for(unsigned t=0;t<STAGE_MAX_TEXTURES;t++)if(g_texture_cache_slot[t])memset(texture_cache_address(g_texture_cache_slot[t]-1),0xcd,TEX_TEXELS);
  smg_game_restore_texture_cache();check_cache(n%2,-1);
 }
 // Marble hides glass by assigning a shared zero texture at the final slot.
 // Exercise the actual restore/rebinding branch on both migration directions.
 unsigned hidden=TEXCACHE_SLOTS-1;g_texture_cache_slot[26]=0;g_texture_cache_slot[7]=hidden+1;
 memset(texture_cache_address(hidden),0,TEX_TEXELS);
 g_texture_cache_reference[hidden].mip[0]=g_texture_cache_reference[hidden].mip[1]=(uintptr_t)texture_cache_address(hidden);
 for(unsigned n=0;n<20;n++){g_texture_dtcm_enabled=n%2;stage_sync_texture_cache();check_cache(n%2,hidden);smg_game_restore_texture_cache();check_cache(n%2,hidden);}
 // Loading another stage must replace old dynamic bindings and texel bytes.
 for(unsigned t=0;t<STAGE_MAX_TEXTURES;t++)for(unsigned i=0;i<TEX_TEXELS;i++)texels[t][0][i]^=0x9d;
 stage_cache_textures(&g_game.stage);check_cache(true,-1);
 assert(g_texture_cache_slot[7]==0&&g_texture_cache_slot[first_cached]==TEXCACHE_SLOTS);
 g_game.stage.header=NULL;g_texture_dtcm_enabled=false;stage_sync_texture_cache();smg_game_restore_texture_cache();guards();
 printf("%u full-texture/descriptor checks: all%u weighted slots, exact both-mip identity, DTCM/AXI migration, fallback/alignment/size, restores, hidden glass, stage replacement and guards pass\n",checks,TEXCACHE_SLOTS);return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='texture-cache-',dir=ROOT/'tmp') as directory:
 p=Path(directory);(p/'test.c').write_text(prefix+water+production+suffix)
 try:
  for slots in (54,64):
   subprocess.run(['docker','run','--rm','--name',p.name,'--user','1000:1000','-v',f'{ROOT}:/project','-w',f'/project/tmp/{p.name}','-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c',f'cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined -DSMG_TEXTURE_CACHE_SLOTS={slots} -I/project/src test.c -o test && ./test'],check=True,timeout=60)
 finally:subprocess.run(['docker','rm','-f',p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
