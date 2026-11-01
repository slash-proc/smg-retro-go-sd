#!/usr/bin/env python3
"""Check real-stage chunk-cache copies, lifetime and flash-read avoidance.

Builds production binding/culling in Docker with ASan/UBSan. Cached culls run
while canonical source chunk pages are PROT_NONE. Uses all linked main/far
stage pairs, exact capacity edges, toggles, reload/restore and allocation failure.
"""
from pathlib import Path
import struct
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
s=(ROOT/'src/world/stage.c').read_text()
cache=s[s.index('static uint8_t* g_visibility_cache_memory;'):s.index('\n#endif',s.index('static uint8_t* g_visibility_cache_memory;'))]
v=(ROOT/'src/graphics/visibility.c').read_text()
cull=v[v.index('static volatile bool g_cull_narrow_enabled'):v.index('\n#endif',v.index('static volatile bool g_cull_narrow_enabled'))]
h=(ROOT/'src/graphics/visibility.h').read_text();start=h.index('typedef struct {\n    const StageChunk* chunks;');types=h[start:h.index('} CullJob;',start)]+'} CullJob;\n'
prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include "assets/asset_format.h"
#define SMG_COUNT(field) ((void)0)
typedef struct {const StageHeader* header;const StageChunk* chunks;} Stage;
static struct {Stage stage;} g_game;
static Stage g_far_stage;
static bool g_has_far_stage;
'''
suffix=r'''
typedef struct {uint8_t* blob;size_t bytes;uint32_t count;} Blob;
static unsigned checks;
static uint8_t buffer[128*1024+32] __attribute__((aligned(4)));
static Blob load(const char*path){
 if(!strcmp(path,"-"))return (Blob){0};
 FILE*f=fopen(path,"rb");if(!f)abort();StageHeader h;if(fread(&h,sizeof h,1,f)!=1)abort();
 uint32_t count=h.chunk_count;size_t page=sysconf(_SC_PAGESIZE),payload=(count*32+page-1)&~(page-1);
 uint8_t*blob=mmap(NULL,page+payload,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);if(blob==MAP_FAILED)abort();
 h.chunks=page;memcpy(blob,&h,sizeof h);if(fread(blob+page,32,count,f)!=count)abort();fclose(f);
 return (Blob){blob,page+payload,count};
}
static Stage stage(Blob b){return (Stage){(StageHeader*)b.blob,b.blob?(StageChunk*)(b.blob+((StageHeader*)b.blob)->chunks):NULL};}
static void protect(Blob b,int mode){if(b.blob&&mprotect(b.blob+((StageHeader*)b.blob)->chunks,b.bytes-((StageHeader*)b.blob)->chunks,mode))abort();}
static unsigned culled(Stage s,uint16_t*leaves){
 CullJob j={.chunks=s.chunks,.n=s.header->chunk_count,.z_drop=1000000000,.leaves=leaves,.cam={200001,200001,200001}};
 return geometry_cull_chunks(&j);
}
static void check_bytes(Blob main,Blob far){
 size_t m=main.count*32,f=far.count*32;
 if(memcmp(buffer,stage(main).chunks,m)||(f&&memcmp(buffer+m,stage(far).chunks,f)))abort();
 if(g_game.stage.chunks!=(StageChunk*)buffer||(far.blob&&g_far_stage.chunks!=(StageChunk*)(buffer+m)))abort();
 for(unsigned i=128*1024;i<sizeof buffer;i++)if(buffer[i]!=0x5a)abort();
 checks++;
}
int main(int argc,char**argv){
 if(argc<3||argc%2!=1)abort();memset(buffer,0x5a,sizeof buffer);
 if(smg_game_visibility_cache_bytes()!=128*1024)abort();
 for(int pair=1;pair<argc;pair+=2){
  Blob main=load(argv[pair]),far=load(argv[pair+1]);g_game.stage=stage(main);g_far_stage=stage(far);g_has_far_stage=far.blob!=NULL;
  uint16_t*expected=calloc(main.count+far.count,2),*actual=calloc(main.count+far.count,2);if(!expected||!actual)abort();
  unsigned n=culled(g_game.stage,expected),nf=far.blob?culled(g_far_stage,expected+main.count):0;
  g_visibility_cache_enabled=true;smg_game_set_visibility_cache(buffer,128*1024);stage_bind_visibility_cache();
  if(!smg_visibility_cache_stats.active)abort();check_bytes(main,far);
  uint32_t fills=smg_visibility_cache_stats.fills;
  protect(main,PROT_NONE);protect(far,PROT_NONE);
  for(unsigned i=0;i<5;i++){
   stage_sync_visibility_cache();
   if(culled(g_game.stage,actual)!=n||memcmp(actual,expected,n*2))abort();
   if(far.blob&&(culled(g_far_stage,actual+main.count)!=nf||memcmp(actual+main.count,expected+main.count,nf*2)))abort();
  }
  if(smg_visibility_cache_stats.fills!=fills)abort();
  // Disabling must restore original descriptors without touching source data.
  g_visibility_cache_enabled=false;stage_sync_visibility_cache();
  if(g_game.stage.chunks!=stage(main).chunks||(far.blob&&g_far_stage.chunks!=stage(far).chunks))abort();
  protect(main,PROT_READ|PROT_WRITE);protect(far,PROT_READ|PROT_WRITE);
  g_visibility_cache_enabled=true;stage_sync_visibility_cache();check_bytes(main,far);
  // Stage reload can reuse the same header address; force canonical rebinding.
  g_game.stage=stage(main);g_far_stage=stage(far);stage_bind_visibility_cache();check_bytes(main,far);
  memset(buffer,0xcc,(main.count+far.count)*32);smg_game_restore_visibility_cache();check_bytes(main,far);
  // Capacity one byte short must preserve both canonical pointers, not a prefix.
  smg_game_set_visibility_cache(buffer,(main.count+far.count)*32-1);stage_sync_visibility_cache();
  if(smg_visibility_cache_stats.active||g_game.stage.chunks!=stage(main).chunks||(far.blob&&g_far_stage.chunks!=stage(far).chunks))abort();
  uint32_t bypass=smg_visibility_cache_stats.bypasses;
  for(unsigned i=0;i<20;i++)stage_sync_visibility_cache();if(smg_visibility_cache_stats.bypasses!=bypass)abort();
  smg_game_set_visibility_cache(buffer,(main.count+far.count)*32);stage_sync_visibility_cache();check_bytes(main,far);
  smg_game_set_visibility_cache(buffer+1,128*1024);stage_sync_visibility_cache();if(smg_visibility_cache_stats.active||smg_visibility_cache_stats.capacity)abort();
  smg_game_set_visibility_cache(NULL,128*1024);stage_sync_visibility_cache();if(smg_visibility_cache_stats.active||g_game.stage.chunks!=stage(main).chunks)abort();
  smg_game_set_visibility_cache(buffer,128*1024);
  ((StageHeader*)main.blob)->chunk_count=UINT32_MAX;stage_sync_visibility_cache();if(smg_visibility_cache_stats.active)abort();
  ((StageHeader*)main.blob)->chunk_count=main.count;stage_bind_visibility_cache();check_bytes(main,far);
  if(far.blob){
   ((StageHeader*)far.blob)->chunk_count=UINT32_MAX;stage_bind_visibility_cache();if(smg_visibility_cache_stats.active)abort();
   ((StageHeader*)far.blob)->chunk_count=far.count;stage_bind_visibility_cache();check_bytes(main,far);
  }
  smg_game_set_visibility_cache(NULL,0);stage_sync_visibility_cache();
  free(expected);free(actual);munmap(main.blob,main.bytes);if(far.blob)munmap(far.blob,far.bytes);
 }
 g_game.stage=(Stage){0};g_has_far_stage=false;stage_sync_visibility_cache();if(smg_visibility_cache_stats.active)abort();
 printf("%u real-stage cache byte/binding checks; all linked pairs, protected-source cached culls, exact/short capacity, no repeated bypass, reload/restore, null/misaligned allocation and overflowing counts pass\n",checks);
}
'''
with tempfile.TemporaryDirectory(prefix='visibility-cache-',dir=ROOT/'tmp') as directory:
 p=Path(directory);(p/'test.c').write_text(prefix+types+cache+cull+suffix);args=[]
 stages=[x for x in sorted((ROOT/'build/assets').glob('stage_*.bin')) if 'shared' not in x.name and not x.stem.endswith('_far')]
 for i,main in enumerate(stages):
  if len(main.read_bytes())<204:continue
  for kind,source in [('main',main),('far',main.with_name(main.stem+'_far.bin'))]:
   if not source.exists():args.append('-');continue
   b=source.read_bytes();count=struct.unpack_from('<I',b,12)[0];offset=struct.unpack_from('<I',b,40)[0]
   assert offset+count*32<=len(b)
   name=f'{i}-{kind}.bin';(p/name).write_bytes(b[:204]+b[offset:offset+count*32]);args.append(name)
 try:
  command='cc -I/source/src -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined test.c -o test && ./test '+' '.join(args)
  subprocess.run(['docker','run','--rm','--name',p.name,'--user','1000:1000','-v',f'{ROOT}:/source:ro','-v',f'{p}:/test','-w','/test','-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c',command],check=True,timeout=60)
 finally:subprocess.run(['docker','rm','-f',p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
