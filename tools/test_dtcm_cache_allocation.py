#!/usr/bin/env python3
"""Exercise production startup DTCM policy with bounded/failing pool stubs."""
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
s=(ROOT/'retro-go-sd/src/smg_homebrew.c').read_text()
a=s.index('static void smg_allocate_dtcm_caches(')
production=s[a:s.index('/* End DTCM cache allocation. */',a)]
prefix=r'''
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifndef SMG_TEXTURE_CACHE_SLOTS
#define SMG_TEXTURE_CACHE_SLOTS 54
#endif
#define SMG_TEXTURE_BYTES (SMG_TEXTURE_CACHE_SLOTS * 1024u)
static _Alignas(8) uint8_t pool[131072];
static uint32_t available,used,fail,requests[4],nrequest,texture_bytes,vertex_bytes;
static void *texture_ptr,*vertex_ptr,*zbin_ptr;
static uint32_t smg_game_texture_cache_bytes(void){return SMG_TEXTURE_BYTES;}
static uint32_t smg_game_vertex_cache_bytes(void){return 32768;}
static uint32_t smg_game_vertex_cache_min_bytes(void){return 16384;}
static uint32_t dtc_get_free_size(void){return available-used;}
static void* dtc_malloc(uint32_t bytes){
 if(nrequest>=4||bytes+16384>available-used)abort();requests[nrequest++]=bytes;
 unsigned bit=bytes==SMG_TEXTURE_BYTES?1:bytes==32768?2:bytes==16384?4:bytes==2048?8:0;if(!bit)abort();
 if(fail&bit)return NULL;
 void* result=pool+used;used+=bytes;return result;
}
static void smg_game_set_texture_cache(void* p,uint32_t bytes){texture_ptr=p;texture_bytes=bytes;}
static void smg_game_set_vertex_cache(void* p,uint32_t bytes){vertex_ptr=p;vertex_bytes=bytes;}
#define SMG_ZBIN_DTCM 1
static int smg_visibility_set_zbin_storage(void* p,uint32_t bytes){zbin_ptr=p;return p&&bytes==2048;}
#define printf(...) ((void)0)
'''
suffix=r'''
#undef printf
int main(void){
 unsigned checks=0;
 for(uint32_t capacity=0;capacity<=131072;capacity++)for(unsigned failure=0;failure<16;failure++){
  available=capacity;fail=failure;used=nrequest=0;texture_ptr=vertex_ptr=zbin_ptr=(void*)1;
  smg_allocate_dtcm_caches();
  // Independent policy invariants: textures first when feasible, then the
  // largest admissible source-cache tier. A failed request consumes no span.
  unsigned tex=(capacity>=SMG_TEXTURE_BYTES+16384)&&!(failure&1);
  uint32_t after_texture=capacity-(tex?SMG_TEXTURE_BYTES:0),chosen=0;
  const uint32_t candidates[]={32768,16384};
  for(unsigned i=0;i<2;i++){
   unsigned failure_bit=candidates[i]==32768?2:4;
   if(after_texture>=candidates[i]+16384 && !(failure&failure_bit)){chosen=candidates[i];break;}
  }
  if((texture_ptr!=NULL)!=tex||(vertex_ptr!=NULL)!=(chosen!=0)||texture_bytes!=SMG_TEXTURE_BYTES)abort();
  if(chosen&&vertex_bytes!=chosen)abort();
  if(tex&&texture_ptr!=pool)abort();
  if(chosen&&vertex_ptr!=pool+(tex?SMG_TEXTURE_BYTES:0))abort();
  unsigned zbin=(capacity-(tex?SMG_TEXTURE_BYTES:0)-chosen)>=2048+16384&&!(failure&8);
  if(used!=(tex?SMG_TEXTURE_BYTES:0)+chosen+(zbin?2048:0)||used>capacity||((tex||chosen||zbin)&&dtc_get_free_size()<16384))abort();
  if((zbin_ptr!=NULL)!=zbin)abort();
  if(zbin&&zbin_ptr!=pool+(tex?SMG_TEXTURE_BYTES:0)+chosen)abort();
  if(zbin&&dtc_get_free_size()<16384)abort();
  for(unsigned i=1;i<nrequest;i++)if(requests[i]>=requests[i-1])abort();
  if(capacity>=SMG_TEXTURE_BYTES+16384 && (!nrequest||requests[0]!=SMG_TEXTURE_BYTES))abort();
  if(after_texture>=32768+16384){
   unsigned found=0;for(unsigned i=0;i<nrequest;i++)found+=requests[i]==32768;
   if(found!=1)abort();
  }
  checks++;
 }
 fprintf(stderr,"%u exact startup allocation cases: texture priority,32/16KiB source cache,2KiB DTCM LUT fallback,16KiB reserve, capacity boundaries and independent allocation failures pass\n",checks);
 return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='dtcm-allocation-',dir=ROOT/'tmp') as directory:
    path=Path(directory);(path/'test.c').write_text(prefix+production+suffix)
    try:
        for slots in (54,64):
            subprocess.run(['docker','run','--rm','--name',path.name,'--user','1000:1000','-v',f'{path}:/test','-w','/test','smg-runtime-tests','sh','-c',
                f'cc -O3 -no-pie -fsanitize=address,undefined -fno-sanitize-recover=all -DSMG_TEXTURE_CACHE_SLOTS={slots} test.c -o test && ./test'],check=True,timeout=60)
    finally:subprocess.run(['docker','rm','-f',path.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
