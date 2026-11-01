#!/usr/bin/env python3
"""Check production point claiming against an atomic per-triangle capacity oracle."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
h = (ROOT / 'src/graphics/visibility.h').read_text()
v = (ROOT / 'src/graphics/visibility.c').read_text()
a = h.index('typedef struct {\n    const uint16_t* visible;')
types = h[a:h.index('} ClaimJob;', a)] + '} ClaimJob;\n'
a = v.index('int geometry_claim_points(ClaimJob* job){')
function = v[a:v.index('\n#endif', a)]
prefix = r'''
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "assets/asset_format.h"
#define __GNW__ 1
typedef struct {int32_t x,y,z;} v3;
typedef struct {v3 position;uint8_t clip,mark;uint16_t pad;} FramePoint;
static unsigned drops,peak;
#define SMG_COUNT(field) (++drops)
#define SMG_PEAK(field,value) do{if((unsigned)(value)>peak)peak=(value);}while(0)
'''
suffix = r'''
static uint32_t rng=0x71b42c9d;
static uint32_t next(void){rng^=rng<<13;rng^=rng>>17;rng^=rng<<5;return rng;}
static unsigned reference(ClaimJob* j){
 unsigned n=0;
 for(;n<(unsigned)j->n;n++){
  const StageTriangle*t=&j->tris[j->visible[n]];uint8_t pending[16]={0};unsigned need=0;
  for(unsigned k=0;k<3;k++)pending[j->home[t->v[k].point]]=1;
  for(unsigned leaf=0;leaf<16;leaf++)if(pending[leaf]&&j->leaf_stamps[leaf]!=(uint8_t)j->stamp)
   need+=j->leaf_first[leaf+1]-j->leaf_first[leaf];
  if(j->next_slot+need>j->slot_limit)break;
  for(unsigned k=0;k<3;k++){
   unsigned p=t->v[k].point,leaf=j->home[p];
   if(j->leaf_stamps[leaf]!=(uint8_t)j->stamp){
    j->leaf_stamps[leaf]=(uint8_t)j->stamp;j->leaf_delta[leaf]=(int16_t)(j->next_slot-j->leaf_first[leaf]);
    j->next_slot+=j->leaf_first[leaf+1]-j->leaf_first[leaf];
   }
   FramePoint*slot=&j->pool[(int32_t)j->leaf_delta[leaf]+p];
   if(slot->mark!=(uint8_t)j->stamp){slot->mark=(uint8_t)j->stamp;*j->plist++=p;}
  }
 }
 return n;
}
int main(int argc,char**argv){
 unsigned full_reuse=0,atomic_stops=0,empty=0;
 for(unsigned test=0;test<40000;test++){
  uint16_t first[17],home[256],visible[256],pa[512],pb[512];
  uint8_t stamps_a[16]={0},stamps_b[16]={0};int16_t delta_a[16],delta_b[16];
  FramePoint pool_a[512],pool_b[512];StageTriangle tris[256];unsigned count=0,maxneed=0;
  for(unsigned leaf=0;leaf<16;leaf++){
   first[leaf]=count;unsigned size=1+next()%16;
   for(unsigned i=0;i<size;i++)home[count++]=leaf;
  }first[16]=count;
  for(unsigned t=0;t<256;t++){
   visible[t]=t;unsigned need=0;uint8_t seen[16]={0};
   for(unsigned k=0;k<3;k++){unsigned p=next()%count;tris[t].v[k].point=p;seen[home[p]]=1;}
   for(unsigned l=0;l<16;l++)if(seen[l])need+=first[l+1]-first[l];
   if(need>maxneed)maxneed=need;
  }
  // The last repeated face must remain admissible at an exactly full pool.
  if(test%7==0)for(unsigned t=1;t<256;t++)tris[t]=tris[0];
  unsigned cap=test%7==0?0:next()%(count+1);
  if(test%7==0){uint8_t seen[16]={0};for(unsigned k=0;k<3;k++)seen[home[tris[0].v[k].point]]=1;
   for(unsigned l=0;l<16;l++)if(seen[l])cap+=first[l+1]-first[l];}
  memset(pool_a,0,sizeof pool_a);memcpy(pool_b,pool_a,sizeof pool_a);
  memset(delta_a,0x4a,sizeof delta_a);memcpy(delta_b,delta_a,sizeof delta_a);
  memset(pa,0x5c,sizeof pa);memcpy(pb,pa,sizeof pa);
  unsigned stamp=test%2?255:1,n=test%11?256:0;
  ClaimJob a={visible,n,tris,home,first,stamps_a,delta_a,pool_a,stamp,pa,0,cap,cap>maxneed?cap-maxneed:0};
  ClaimJob b=a;b.leaf_stamps=stamps_b;b.leaf_delta=delta_b;b.pool=pool_b;b.plist=pb;
  drops=peak=0;unsigned got=geometry_claim_points(&a),want=reference(&b);
  if(got!=want||a.next_slot!=b.next_slot||a.plist-pa!=b.plist-pb||
     memcmp(stamps_a,stamps_b,sizeof stamps_a)||memcmp(delta_a,delta_b,sizeof delta_a)||
     memcmp(pool_a,pool_b,sizeof pool_a)||memcmp(pa,pb,sizeof pa)||drops!=(got<n)||peak!=a.next_slot||a.next_slot>cap)abort();
  if(got==256&&a.next_slot==cap)full_reuse++;if(got<n)atomic_stops++;if(!n)empty++;
 }
 if(!full_reuse||!atomic_stops||!empty)abort();
 printf("40000 atomic-claim oracle jobs: %u full-pool reuse, %u capacity stops, %u empty jobs; exact marks/maps/lists/counters and untouched guards pass\n",full_reuse,atomic_stops,empty);
 unsigned checked=0;
 for(int file=1;file<argc;file++){
  FILE*f=fopen(argv[file],"rb");if(!f)abort();fseek(f,0,SEEK_END);long bytes=ftell(f);rewind(f);
  uint8_t*data=malloc(bytes);if(fread(data,1,bytes,f)!=(size_t)bytes)abort();fclose(f);
  const StageHeader*h=(const StageHeader*)data;if(!h->point_home){free(data);continue;}
  const uint16_t*home=(const uint16_t*)(data+h->point_home),*first=(const uint16_t*)(data+h->leaf_point_first);
  const StageTriangle*t=(const StageTriangle*)(data+h->triangles);
  for(unsigned i=0;i<h->triangle_count;i++){
   uint16_t leaf[3];unsigned need=0;for(unsigned k=0;k<3;k++)leaf[k]=home[t[i].v[k].point];
   if(leaf[0]==UINT16_MAX||leaf[1]==UINT16_MAX||leaf[2]==UINT16_MAX)continue;
   for(unsigned k=0;k<3;k++)if(!(k>0&&leaf[k]==leaf[0])&&!(k>1&&leaf[k]==leaf[1]))need+=first[leaf[k]+1]-first[leaf[k]];
   if(need>h->leaf_slot_max){fprintf(stderr,"Bad fast margin: %s tri%u need%u bound%u\n",argv[file],i,need,h->leaf_slot_max);abort();}checked++;
  }free(data);
 }
 printf("%u real-stage triangle leaf margins satisfy the production fast-path bound\n",checked);
}
'''
paths = [q for q in sorted((ROOT / 'build/assets').glob('stage_*.bin'))
         if 'shared' not in q.name and q.stat().st_size >= 204]
with tempfile.TemporaryDirectory(prefix='point-claim-', dir=ROOT / 'tmp') as directory:
    p = Path(directory)
    (p / 'test.c').write_text(prefix + types + function + suffix)
    try:
        subprocess.run(['docker', 'run', '--rm', '--name', p.name, '--user', '1000:1000',
                        '-v', f'{ROOT}:/source:ro', '-v', f'{p}:/test', '-w', '/test',
                        'smg-runtime-tests', 'sh', '-c',
                        'cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined -fno-sanitize-recover=all -I/source/src test.c -o test && exec ./test "$@"',
                        'test', *[f'/source/{q.relative_to(ROOT)}' for q in paths]], check=True, timeout=90)
    finally:
        subprocess.run(['docker', 'rm', '-f', p.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
