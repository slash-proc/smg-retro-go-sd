#!/usr/bin/env python3
"""Check optional AHB reservation and rollback under heap overhead/fragmentation."""
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
s=(ROOT/'retro-go-sd/src/smg_homebrew.c').read_text();i=s.index('static void smg_allocate_home_cache(void)\n{');production=s[i:s.index('/* End AHB home-cache allocation. */',i)]
prefix=r'''
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
static _Alignas(8) uint8_t pool[65536];
static uint32_t capacity,metadata,loss,used,attempts,frees,fail,bytes_arg;
static void *bound,*smg_home_allocation;
static uint32_t smg_game_home_cache_bytes(void){return 65536;}
static size_t ahb_get_free_size(void){return used?(capacity>used+loss?capacity-used-loss:0):capacity;}
static void* ahb_malloc(size_t n){
 if(n!=65536||capacity<n+metadata||attempts++)abort();if(fail)return NULL;used=n+metadata;return pool;
}
static void fake_free(void*p){if(p!=pool||!used||frees++)abort();used=0;}
static void smg_game_set_home_cache(void*p,uint32_t n){bound=p;bytes_arg=n;}
#define free fake_free
'''
callbacks=s[s.index('static void Shutdown(void)\n{'):s.index('static void SramSave(void)')]
prefix+=r'''
#define SMG_GAME_RUNTIME 1
#define SAMPLE_RATE 16000
#define AUDIO_LENGTH 266
static unsigned beep_enabled,restores;
static void odroid_settings_app_int32_set(const char*k,int v){(void)k;(void)v;}
static void smg_game_invalidate_vertex_cache(void){}
static void smg_game_restore_texture_cache(void){}
static void smg_game_restore_visibility_cache(void){}
static void smg_game_restore_home_cache(void){restores++;}
static void odroid_audio_init(int rate){if(rate!=16000)abort();}
static void audio_clear_buffers(void){}
static void audio_start_playing(int n){if(n!=266)abort();}
'''
suffix=r'''
#undef free
int main(void){
 unsigned checks=0;
 for(capacity=0;capacity<=131072;capacity++)for(metadata=0;metadata<=64;metadata+=8)
 for(unsigned kind=0;kind<3;kind++)for(fail=0;fail<2;fail++){
  loss=kind==0?0:kind==1?64:8192;used=attempts=frees=0;bound=(void*)1;
  smg_allocate_home_cache();
  unsigned expected=capacity>=65536+16384+metadata+loss&&!fail;
  if((bound!=NULL)!=expected||bytes_arg!=65536||attempts!=(capacity>=81920))abort();
  if(expected&&(bound!=pool||ahb_get_free_size()<16384||frees))abort();
  if(!expected&&used)abort();
  if(frees&&(!attempts||fail))abort();
  checks++;
 }
 capacity=86580;metadata=16;loss=fail=used=attempts=frees=0;smg_home_allocation=NULL;
 smg_allocate_home_cache();if(!smg_home_allocation)abort();Shutdown();
 if(smg_home_allocation||bound||used||frees!=1)abort();Shutdown();if(frees!=1)abort();
 attempts=0;SleepWake();if(!smg_home_allocation||!bound||restores!=1)abort();
 frees=0;Shutdown();capacity=70000;attempts=0;SleepWake();if(smg_home_allocation||bound||restores!=2)abort();
 printf("%u AHB allocation/reserve cases: capacity edges, heap overhead, fragmented spans, failure, rollback, idempotent shutdown and wake reallocation pass\n",checks);
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='ahb-home-',dir=ROOT/'tmp') as directory:
 p=Path(directory);(p/'test.c').write_text(prefix+production+callbacks+suffix)
 try:
  subprocess.run(['docker','run','--rm','--name',p.name,'--user','1000:1000','-v',f'{p}:/test','-w','/test','smg-runtime-tests','sh','-c','cc -O3 -no-pie -fsanitize=address,undefined -fno-sanitize-recover=all test.c -o test && ./test'],check=True,timeout=60)
 finally:subprocess.run(['docker','rm','-f',p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
