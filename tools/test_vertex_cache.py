#!/usr/bin/env python3
"""Validate cached source coordinates against production uncached transforms.

Exercises collisions, source changes, list/home remapping, camera changes,
mutable non-stage sources, missing/misaligned buffers and the empty-tag index.
Docker ASan checks the fixed cache bounds; cached hits are also tested with
mprotect(PROT_NONE) to prove that they do not read the source array.
"""
import ast
from pathlib import Path
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]

def section(source,start,end):
    at=source.index(start)
    return source[at:source.index(end,at)]

def literal(path,name):
    for node in ast.parse(path.read_text()).body:
        if isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id==name for t in node.targets):
            return ast.literal_eval(node.value)
    raise ValueError(name)

prefix=literal(ROOT/'tools/test_visibility.py','prefix')
prefix+='#include <sys/mman.h>\n#include <unistd.h>\n'
header=(ROOT/'src/graphics/visibility.h').read_text()
types=section(header,'typedef struct {\n    const int16_t (*src)[3];','} TransformJob;')+'} TransformJob;\n'
frame=(ROOT/'src/graphics/frame.c').read_text()
projection=frame[:frame.index('static inline uint8_t frame_get')]
production=section((ROOT/'src/graphics/visibility.c').read_text(),'#define STAGE_VERTEX_CACHE_ENTRIES','\n#endif /* __GNW__ transforms */')
suffix=r'''
#define SOURCE_POINTS 65536
#define OUTPUT_POINTS 1024
static int16_t sources[2][SOURCE_POINTS][3],mutable_points[SOURCE_POINTS][3];
static FramePoint a[OUTPUT_POINTS],b[OUTPUT_POINTS];
static uint32_t sa[OUTPUT_POINTS],sb[OUTPUT_POINTS];
static StageVertexCacheEntry entries[STAGE_VERTEX_CACHE_ENTRIES+1];
static unsigned comparisons;
static void check(TransformJob job){
 memset(a,0xa5,sizeof a);memcpy(b,a,sizeof a);memset(sa,0x75,sizeof sa);memcpy(sb,sa,sizeof sa);
 // Read the same source through the independent allocation-failure path.
 // Restore the cache descriptor afterward so the candidate retains warm data.
 StageVertexCacheEntry* memory=g_stage_vertex_cache;
 const int16_t (*source)[3]=g_stage_vertex_cache_source;
 bool dsp=g_transform_dsp_enabled;g_transform_dsp_enabled=false;
 g_stage_vertex_cache=NULL;job.dst=a;job.screen=sa;engine_transform_points(&job);
 g_stage_vertex_cache=memory;g_stage_vertex_cache_source=source;
 g_transform_dsp_enabled=dsp;
 job.dst=b;job.screen=sb;engine_transform_points(&job);
 if(memcmp(a,b,sizeof a)||memcmp(sa,sb,sizeof sa)){
  fprintf(stderr,"Source cache changes transform output at job %u\n",comparisons);exit(1);
 }
 if(memcmp(&entries[STAGE_VERTEX_CACHE_ENTRIES],"ZZZZZZZZ",8))abort();
 if(g_stage_vertex_cache_mask==2047 && memcmp(&entries[2048],"ZZZZZZZZ",8))abort();
 comparisons++;
}
int main(void){
 g_transform_dsp_enabled=true;
 for(unsigned i=0;i<65536;i++)lut_project[i]=(1u<<20)/(i?i:1);
 for(unsigned s=0;s<2;s++)for(unsigned i=0;i<SOURCE_POINTS;i++)for(unsigned k=0;k<3;k++)
  sources[s][i][k]=(int16_t)next();
 memcpy(mutable_points,sources[0],sizeof mutable_points);
 memset(entries,0,sizeof entries);memcpy(&entries[STAGE_VERTEX_CACHE_ENTRIES],"ZZZZZZZZ",8);
 if(smg_game_vertex_cache_bytes()!=32768||smg_game_vertex_cache_min_bytes()!=16384)abort();
 smg_game_set_vertex_cache(NULL,16384);if(g_stage_vertex_cache)abort();
 smg_game_set_vertex_cache(entries,16383);if(g_stage_vertex_cache)abort();
 smg_game_set_vertex_cache((char*)entries+1,16384);if(g_stage_vertex_cache)abort();
 smg_game_set_vertex_cache(entries,32767);if(!g_stage_vertex_cache||g_stage_vertex_cache_mask!=2047)abort();
 smg_game_set_vertex_cache(entries,32769);if(g_stage_vertex_cache_mask!=4095)abort();
 smg_game_set_vertex_cache(entries,16384);if(!g_stage_vertex_cache||g_stage_vertex_cache_mask!=2047)abort();
 memcpy(&entries[2048],"ZZZZZZZZ",8);
 uint16_t list[256],home[SOURCE_POINTS];int16_t delta[2]={0,0};
 for(unsigned n=0;n<12000;n++){
  if(n%23==0){
   uint32_t bytes=(n/23)%2?16384:32768;smg_game_set_vertex_cache(entries,bytes);
   if(bytes==16384)memcpy(&entries[2048],"ZZZZZZZZ",8);
  }
  unsigned source=(n/13)%2;g_game.stage.points=sources[source];
  bool animated=n%11==0;const int16_t (*input)[3]=animated?mutable_points:sources[source];
  // Collision pairs share low 11 bits but have different coordinates. Every
  // input maps independently into a bounded destination slot through home/delta.
  for(unsigned i=0;i<256;i++){
   unsigned point=(n%3==0)?i:(i+(n%32)*2048);
   if(n%97==0 && i==255)point=UINT16_MAX;
   list[i]=point;home[point]=1;
  }
  // Home maps a contiguous source run back to output 0..255; the special
  // empty-tag index uses its own leaf to map back to slot 255.
  unsigned offset=n%3==0?0:(n%32)*2048;delta[1]=-(int)offset;
  // Signed 16-bit home deltas cannot represent the upper half. Use direct
  // low-index jobs or a fixed high-source list with a matching positive wrap
  // avoided: source >32767 jobs use sequential output list below instead.
  if(offset>32767)for(unsigned i=0;i<256;i++){list[i]=i;home[i]=1;} 
  if(offset>32767)delta[1]=0;
  // Test index 65535 separately below (home delta range cannot map it here).
  if(n%97==0)list[255]=255,home[255]=0;
  home[255]=list[255]==255?0:home[255];
  TransformJob job={.src=input,.dst=b,.screen=sb,.list=n%5?list:NULL,.n=256,
     .home=n%5?home:NULL,.delta=delta};
  for(unsigned row=0;row<3;row++){
   for(unsigned k=0;k<3;k++)job.m[row][k]=(int)(next()%65)-32;
   job.m[row][3]=(int)(next()%4096)-2048;
  }
  if(animated)for(unsigned i=0;i<256;i++)for(unsigned k=0;k<3;k++)mutable_points[list[i]][k]=(int16_t)next();
  if(n%17==0)smg_game_invalidate_vertex_cache();
  g_stage_vertex_cache_enabled=n%19!=0;check(job);
 }
 // Same visible source set, several changing matrices: all warm points hit.
 g_stage_vertex_cache_enabled=true;g_game.stage.points=sources[0];smg_game_invalidate_vertex_cache();
 TransformJob warm={.src=sources[0],.n=256};for(unsigned row=0;row<3;row++)warm.m[row][row]=256;
 check(warm);uint32_t hits=smg_vertex_cache_stats.hits,misses=smg_vertex_cache_stats.misses;
 warm.m[0][3]=1234;warm.m[2][3]=-197;check(warm);
 if(smg_vertex_cache_stats.hits-hits!=256||smg_vertex_cache_stats.misses!=misses)abort();
 // Destination slot remapping must never become a cache key.
 for(unsigned i=0;i<512;i++)home[i]=1;
 delta[1]=32;warm.home=home;warm.delta=delta;check(warm);
 for(unsigned i=0;i<256;i++)list[i]=32+i;
 delta[1]=-32;warm.list=list;check(warm);
 warm.home=NULL;warm.delta=NULL;warm.list=NULL;
 // No read from the immutable source on a warm hit. An OS protection fault
 // catches stale tests that count hits while still loading external vertices.
 size_t page=(size_t)sysconf(_SC_PAGESIZE),guard_bytes=((2304*6+page-1)/page)*page;
 int16_t (*guarded)[3]=mmap(NULL,guard_bytes,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);
 if(guarded==MAP_FAILED)abort();memcpy(guarded,sources[0],2304*6);
 for(unsigned i=0;i<256;i++){list[i]=2048+i;home[2048+i]=1;}
 delta[1]=-2048;warm.home=home;warm.delta=delta;warm.list=list;
 warm.src=guarded;g_game.stage.points=guarded;warm.dst=b;warm.screen=sb;
 for(unsigned bytes=16384;bytes<=32768;bytes*=2){
  smg_game_set_vertex_cache(entries,bytes);engine_transform_points(&warm);
  memcpy(a,b,sizeof a);memcpy(sa,sb,sizeof sa);
  if(mprotect(guarded,guard_bytes,PROT_NONE))abort();engine_transform_points(&warm);
  if(memcmp(a,b,sizeof a)||memcmp(sa,sb,sizeof sa))abort();
  if(mprotect(guarded,guard_bytes,PROT_READ|PROT_WRITE))abort();
 }
 munmap(guarded,guard_bytes);warm.home=NULL;warm.delta=NULL;warm.list=NULL;
 // Full uint16 source index is never confused with an empty cache tag.
 FramePoint* large=calloc(SOURCE_POINTS,sizeof *large);uint32_t* screens=calloc(SOURCE_POINTS,4);
 uint16_t edge_list[]={UINT16_MAX};warm.src=sources[0];warm.dst=large;warm.screen=screens;warm.list=edge_list;warm.n=1;
 g_game.stage.points=sources[0];smg_game_invalidate_vertex_cache();uint32_t bypassed=smg_vertex_cache_stats.bypassed;
 engine_transform_points(&warm);if(smg_vertex_cache_stats.bypassed!=bypassed+1)abort();
 int32_t expected_x=sources[0][UINT16_MAX][0]+warm.m[0][3];if(large[UINT16_MAX].position.x!=expected_x)abort();
 free(large);free(screens);
 printf("%u exact cached/uncached transform comparisons; warm-hit protected source, both16/32KiB bounds, resize, collisions, invalidation, mutable bypass, home/list remapping and allocation fallback pass\n",comparisons);
 return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='vertex-cache-',dir=ROOT/'tmp') as directory:
    path=Path(directory);(path/'test.c').write_text(prefix+types+projection+production+suffix)
    try:
        subprocess.run(['docker','run','--rm','--name',path.name,'--user','1000:1000',
            '-v',f'{ROOT}:/source:ro','-v',f'{path}:/test','-w','/test',
            '-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c',
            'cc -I/source/src -O3 -fno-strict-aliasing -no-pie -fsanitize=address test.c -o test && ./test'],check=True,timeout=60)
    finally:
        subprocess.run(['docker','rm','-f',path.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
