#!/usr/bin/env python3
"""Real-table binding/lifetime tests and protected canonical-home consumers."""
import ast
import struct
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
def section(s,a,z):
 i=s.index(a);return s[i:s.index(z,i)]
def literal(path,name):
 for n in ast.parse(path.read_text()).body:
  if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id==name for t in n.targets):return ast.literal_eval(n.value)
 raise ValueError(name)
prefix=literal(ROOT/'tools/test_visibility.py','prefix').replace('static struct {struct {const int16_t (*points)[3];} stage;} g_game;','typedef struct {const StageHeader* header;const int16_t (*points)[3];} Stage;\nstatic struct {Stage stage;} g_game;')
prefix+='#include <sys/mman.h>\n#include <unistd.h>\n_Static_assert(sizeof(StageHeader)==204 && offsetof(StageHeader,point_home)==176, "stage file layout");\n'
h=(ROOT/'src/graphics/visibility.h').read_text();types=section(h,'typedef struct {\n    const int16_t (*src)[3];','} TransformJob;')+'} TransformJob;\n'
f=(ROOT/'src/graphics/frame.c').read_text();projection=f[:f.index('static inline uint8_t frame_get')]
s=(ROOT/'src/world/stage.c').read_text();cache=section(s,'/* GNW home-table cache. */','/* End home-table cache. */')
v=(ROOT/'src/graphics/visibility.c').read_text();transform=section(v,'#define STAGE_VERTEX_CACHE_ENTRIES','\n#endif /* __GNW__ transforms */')
e=(ROOT/'src/graphics/engine.c').read_text();emit='typedef struct {const uint16_t* home;const int16_t* delta;} EmitJob;\n'+section(e,'static inline int emit_slot(','\n__attribute__((noinline)) static bool emit_through_water')
suffix=r'''
static _Alignas(4) uint8_t slab[65536+16];
static unsigned checks;
int main(int argc,char**argv){
 g_compact_points_enabled=false;
 for(unsigned i=0;i<65536;i++)lut_project[i]=(1u<<20)/(i?i:1);
 for(int file=1;file<argc;file++){
  FILE*f=fopen(argv[file],"rb");if(!f)abort();StageHeader h;if(fread(&h,sizeof h,1,f)!=1)abort();
  uint32_t bytes=h.point_home?h.point_count*2:0;size_t page=sysconf(_SC_PAGESIZE),payload=(bytes+page-1)&~(page-1);
  uint8_t* blob=mmap(NULL,page+payload,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);if(blob==MAP_FAILED)abort();
  if(bytes){h.point_home=page;if(fread(blob+page,1,bytes,f)!=bytes)abort();}fclose(f);memcpy(blob,&h,sizeof h);
  g_game.stage=(Stage){.header=(StageHeader*)blob};memset(slab,0x5a,sizeof slab);
  smg_game_set_home_cache(slab,65536);stage_sync_home_cache();
  const uint16_t* canonical=h.point_home?(uint16_t*)(blob+page):NULL;
  if(!bytes){if(stage_point_home(&g_game.stage)||smg_home_cache_stats.active)abort();checks++;munmap(blob,page+payload);continue;}
  if(!smg_home_cache_stats.active||memcmp(slab,canonical,bytes)||stage_point_home(&g_game.stage)!=(uint16_t*)slab)abort();
  uint32_t fills=smg_home_cache_stats.fills;for(unsigned i=0;i<10;i++)stage_sync_home_cache();if(smg_home_cache_stats.fills!=fills)abort();
  int16_t* deltas=calloc(h.leaf_count,2);int32_t* expected=calloc(h.point_count,4);
  for(unsigned i=0;i<h.leaf_count;i++)deltas[i]=(int16_t)(next()%2001)-1000;
  for(unsigned i=0;i<16;i++)if(canonical[i]!=UINT16_MAX)deltas[canonical[i]]=0;
  EmitJob job={.home=canonical,.delta=deltas};
  for(unsigned p=0;p<h.point_count;p++)if(canonical[p]!=UINT16_MAX)expected[p]=emit_slot(&job,p);
  int16_t source[16][3];FramePoint a[32],b[32];uint32_t sa[32],sb[32];
  for(unsigned i=0;i<16;i++){source[i][0]=i;source[i][1]=i*2;source[i][2]=400;}
  memset(a,0xa5,sizeof a);memcpy(b,a,sizeof a);memset(sa,0x75,sizeof sa);memcpy(sb,sa,sizeof sa);
  TransformJob t={.src=source,.dst=a,.screen=sa,.n=16,.home=canonical,.delta=deltas};for(unsigned k=0;k<3;k++)t.m[k][k]=256;
  engine_transform_points(&t);
  if(mprotect(blob+page,payload,PROT_NONE))abort();job.home=stage_point_home(&g_game.stage);
  // Use cached entries only; expected sentinel locations are identified by
  // the copied cache itself. All bytes were compared before protection.
  for(unsigned p=0;p<h.point_count;p++)if(job.home[p]!=UINT16_MAX&&emit_slot(&job,p)!=expected[p])abort();
  t.home=stage_point_home(&g_game.stage);t.dst=b;t.screen=sb;engine_transform_points(&t);
  if(memcmp(a,b,sizeof a)||memcmp(sa,sb,sizeof sa))abort();
  if(mprotect(blob+page,payload,PROT_READ|PROT_WRITE))abort();checks++;
  g_home_cache_enabled=false;stage_sync_home_cache();if(stage_point_home(&g_game.stage)!=canonical||smg_home_cache_stats.active)abort();
  g_home_cache_enabled=true;stage_sync_home_cache();if(memcmp(slab,canonical,bytes))abort();
  memset(slab,0xcc,bytes);smg_game_restore_home_cache();if(memcmp(slab,canonical,bytes))abort();
  stage_bind_home_cache();if(memcmp(slab,canonical,bytes))abort(); // same-header reload
  smg_game_set_home_cache(slab,bytes-1);stage_sync_home_cache();if(smg_home_cache_stats.active||stage_point_home(&g_game.stage)!=canonical)abort();
  uint32_t bypass=smg_home_cache_stats.bypasses;for(unsigned i=0;i<10;i++)stage_sync_home_cache();if(smg_home_cache_stats.bypasses!=bypass)abort();
  smg_game_set_home_cache(slab,bytes);stage_sync_home_cache();if(!smg_home_cache_stats.active||memcmp(slab,canonical,bytes))abort();
  smg_game_set_home_cache(slab+1,65536);stage_sync_home_cache();if(smg_home_cache_stats.active||smg_home_cache_stats.capacity)abort();
  smg_game_set_home_cache(NULL,65536);stage_sync_home_cache();if(smg_home_cache_stats.active)abort();
  smg_game_set_home_cache(slab,65536);((StageHeader*)blob)->point_count=UINT32_MAX;stage_sync_home_cache();if(smg_home_cache_stats.active)abort();
  ((StageHeader*)blob)->point_count=h.point_count;stage_bind_home_cache();if(memcmp(slab,canonical,bytes))abort();
  StageHeader* other_header=calloc(1,page+payload);memcpy(other_header,&h,sizeof h);
  Stage other={.header=other_header};if(stage_point_home(&other)!=(const uint16_t*)((uint8_t*)other_header+h.point_home))abort();free(other_header);
  for(unsigned i=65536;i<sizeof slab;i++)if(slab[i]!=0x5a)abort();checks+=9;
  g_compact_points_enabled=true;smg_game_set_home_cache(slab,65536);stage_sync_home_cache();
  if(!smg_home_cache_stats.active||!smg_home_cache_stats.compact||stage_point_home(&g_game.stage)!=canonical||stage_point_slots(&g_game.stage)!=(uint16_t*)slab)abort();
  uint16_t* slots=(uint16_t*)slab;
  for(unsigned i=0;i<h.point_count;i++)if(slots[i]!=UINT16_MAX)abort();
  for(unsigned i=0;i<16;i++)slots[i]=(i*5+3)%16;
  memset(a,0xa5,sizeof a);memcpy(b,a,sizeof a);memset(sa,0x75,sizeof sa);memcpy(sb,sa,sizeof sa);
  t.home=NULL;t.delta=NULL;t.dst=a;t.screen=sa;engine_transform_points(&t);
  if(mprotect(blob+page,payload,PROT_NONE))abort();
  t.home=slots;t.dst=b;t.screen=sb;engine_transform_points(&t);job.home=slots;job.delta=NULL;
  for(unsigned i=0;i<16;i++)if(memcmp(&a[i],&b[slots[i]],sizeof a[i])||sa[i]!=sb[slots[i]]||emit_slot(&job,i)!=slots[i])abort();
  if(mprotect(blob+page,payload,PROT_READ|PROT_WRITE))abort();
  smg_game_restore_home_cache();for(unsigned i=0;i<h.point_count;i++)if(slots[i]!=UINT16_MAX)abort();
  g_compact_points_enabled=false;checks++;
  smg_game_set_home_cache(NULL,0);stage_sync_home_cache();free(expected);free(deltas);munmap(blob,page+payload);
 }
 g_game.stage.header=NULL;stage_sync_home_cache();if(stage_point_home(&g_game.stage)||smg_home_cache_stats.active)abort();
 printf("%u real-stage home-cache checks; byte/sentinel preservation, protected-source transform/emission, stage/toggle/restore, exact/short capacities, null/misaligned buffers, overflow and no repeated bypass pass\n",checks);
}
'''
paths=[q for q in sorted((ROOT/'build/assets').glob('stage_*.bin')) if 'shared' not in q.name and q.stat().st_size>=204]
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='home-cache-',dir=ROOT/'tmp') as directory:
    p=Path(directory);(p/'test.c').write_text(prefix+types+projection+cache+transform+emit+suffix);args=[]
    for i,path in enumerate(paths):
        data=path.read_bytes();count=struct.unpack_from('<I',data,0)[0];offset=struct.unpack_from('<I',data,176)[0];size=count*2 if offset else 0
        assert not offset or offset+size<=len(data)
        (p/f'{i}.bin').write_bytes(data[:204]+data[offset:offset+size]);args.append(f'{i}.bin')
    try:
        subprocess.run(['docker','run','--rm','--name',p.name,'--user','1000:1000','-v',f'{ROOT}:/source:ro','-v',f'{p}:/test','-w','/test','-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c',
            'cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined -fno-sanitize-recover=all -I/source/src test.c -o test && exec ./test "$@"','test',*args],check=True,timeout=60)
    finally:subprocess.run(['docker','rm','-f',p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
