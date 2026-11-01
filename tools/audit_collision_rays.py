#!/usr/bin/env python3
"""Exercise production ray/candidate code over real compiled floor triangles.

Every tested ray has an independent double-precision intersection with its
source floor face. This separates missing compiled collision from runtime
candidate loss or intersection arithmetic. Dynamic geometry is excluded.
"""
from pathlib import Path
import subprocess
import tempfile
import sys
import shlex

ROOT=Path(__file__).resolve().parents[1]
source=(ROOT/'src/world/collision.c').read_text()
math=(ROOT/'src/math/vector_math.c').read_text()

def section(s,a,b):return s[s.index(a):s.index(b,s.index(a))]

prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include "assets/asset_format.h"
typedef int32_t fx;
typedef struct {fx x,y,z;} v3;
typedef struct {fx x,y;} v2;
#define __GNW__ 1
#define EWRAM_BSS
#define IWRAM_CODE
#define IWRAM_FN __attribute__((noinline))
#define N14_ONE 16384
#define FXC(x) ((int32_t)((x)*256))
#include "world/collision.h"
#define DYNCOL_BASE 0x7000
typedef struct {int unused;} DynColGroup;
static const DynColGroup* dyncol_deferred_part(uint32_t n){(void)n;return NULL;}
static bool dyncol_segment_meets(const DynColGroup* g,v3 a,v3 b){(void)g;(void)a;(void)b;return true;}
static int dyncol_gather(v3 a,v3 b,uint16_t* indices,int n,int max){(void)a;(void)b;(void)indices;(void)max;return n;}
static inline int32_t fx_mul(int32_t a,int32_t b){return (a*b)>>8;}
static inline int32_t mul14(int32_t a,int32_t b){return (a*b)>>14;}
static inline fx fx_min(fx a,fx b){return a<b?a:b;}
static inline fx fx_max(fx a,fx b){return a>b?a:b;}
'''
# Keep production cross/dot fixed-point arithmetic for diagnosis.
prefix+=section(math,'static inline v3 V3(','\nstatic inline int32_t mul_shift_exact(')
prefix+=section(math,'static inline int32_t v3_dot14(','\nstatic inline v3 v3_cross14(')
prefix+='static inline v3 v3_from_i16(const int16_t* p){return V3(p[0],p[1],p[2]);}\n'
prefix+=section(source,'typedef struct {\n    const int16_t* v[3];','__attribute__((noinline)) static void col_face_ref_dynamic(')
prefix+=r'''
static void col_face_ref(int i,ColFaceRef* r){
 if((unsigned)i>=g_collision.triangle_count)abort();
 const CollisionFace* f=&g_collision.faces[i];
 for(int k=0;k<3;k++)r->v[k]=g_collision.points+3*f->point[k];
 r->normal=f->normal;r->attr=f->attr;
}
static v3 col_ref_vertex(const ColFaceRef* r,int i,v3 origin){
 const int16_t* p=r->v[i];int scale=1<<g_collision.shift;
 return V3(p[0]*scale-origin.x,p[1]*scale-origin.y,p[2]*scale-origin.z);
}
'''
production=section(source,'static inline int32_t clamp16(','\nIWRAM_CODE static inline v3 col_vertex(')
production+=section(source,'IWRAM_CODE static inline int64_t dot64(','\nIWRAM_CODE static v3 closest_point_to_origin(')
production+=section(source,'IWRAM_FN static bool collision_raycast(','\nstatic bool collision_ray(')
if '--plane' in sys.argv:
    production=production.replace('fx d0 = -v3_dot14(a, n);', 'int64_t d0 = -dot64(a, n);').replace('fx d1 = d0 + v3_dot14(d, n);', 'int64_t d1 = d0 + dot64(d, n);').replace('if(d0 < 0 || d1 >= 0) continue;', 'if(d0 < 0 || d1 > 0 || d0 == d1) continue;')

if '--wide' in sys.argv:
    production = production.replace('v3_dot14(a, n)', 'wide_dot14(a, n)').replace('v3_dot14(d, n)', 'wide_dot14(d, n)')
    production = production.replace('dot64(v3_cross(pa, pb), n)', 'wide_edge(pa, pb, n)').replace('dot64(v3_cross(pb, pc), n)', 'wide_edge(pb, pc, n)').replace('dot64(v3_cross(pc, pa), n)', 'wide_edge(pc, pa, n)')
    prefix += r'''
static int32_t wide_dot14(v3 a,v3 n){return (int32_t)(((int64_t)a.x*n.x)>>14)+(int32_t)(((int64_t)a.y*n.y)>>14)+(int32_t)(((int64_t)a.z*n.z)>>14);}
static int64_t wide_edge(v3 a,v3 b,v3 n){return ((int64_t)a.y*b.z-(int64_t)a.z*b.y)*n.x+((int64_t)a.z*b.x-(int64_t)a.x*b.z)*n.y+((int64_t)a.x*b.y-(int64_t)a.y*b.x)*n.z;}
'''

suffix=r'''
static bool oracle_face(int tri,v3 from,v3 to){
 const CollisionFace* f=&g_collision.faces[tri];double p[3][3],d[3],normal[3],a[3],b[3];
 for(int k=0;k<3;k++)for(int c=0;c<3;c++)p[k][c]=g_collision.points[3*f->point[k]+c]*(1<<g_collision.shift);
 for(int c=0;c<3;c++){d[c]=(&to.x)[c]-(&from.x)[c];a[c]=p[1][c]-p[0][c];b[c]=p[2][c]-p[0][c];}
 normal[0]=a[1]*b[2]-a[2]*b[1];normal[1]=a[2]*b[0]-a[0]*b[2];normal[2]=a[0]*b[1]-a[1]*b[0];
 double den=0,num=0;
 for(int c=0;c<3;c++){den+=normal[c]*d[c];num+=normal[c]*(p[0][c]-(&from.x)[c]);}
 if(fabs(den)<1e-9)return false;double t=num/den;if(t<0||t>1)return false;
 double q[3];for(int c=0;c<3;c++)q[c]=(&from.x)[c]+t*d[c];
 for(int e=0;e<3;e++){
  double v[3],w[3];for(int c=0;c<3;c++){v[c]=p[(e+1)%3][c]-p[e][c];w[c]=q[c]-p[e][c];}
  double cross[3]={v[1]*w[2]-v[2]*w[1],v[2]*w[0]-v[0]*w[2],v[0]*w[1]-v[1]*w[0]};
  double dot=0;for(int c=0;c<3;c++)dot+=cross[c]*normal[c];if(dot < -1e-4)return false;
 }
 return true;
}
int main(int argc,char**argv){
 if(argc!=2)return 2;FILE*f=fopen(argv[1],"rb");if(!f)return 2;fseek(f,0,SEEK_END);long z=ftell(f);rewind(f);uint8_t* blob=malloc(z);if(!blob||fread(blob,1,z,f)!=(size_t)z)return 2;fclose(f);StageHeader*h=(void*)blob;
 g_collision=(CollisionWorld){.faces=(void*)(blob+h->collision_triangles),.points=(void*)(blob+h->points),.nodes=(void*)(blob+h->collision_nodes),.node_count=h->collision_node_count,.triangle_count=h->collision_triangle_count,.shift=h->point_shift};
 unsigned probes=0,misses=0,lost=0,cap=0;
 for(unsigned i=0;i<g_collision.triangle_count;i++){
  const CollisionFace*t=&g_collision.faces[i];if(t->normal[2]<4096)continue;
  for(int a=1;a<8;a++)for(int b=1;b<9-a;b++){
   int weights[3]={a,b,9-a-b};v3 centre={0};
   for(int k=0;k<3;k++)for(int c=0;c<3;c++)(&centre.x)[c]+=g_collision.points[3*t->point[k]+c]*weights[k]*(1<<g_collision.shift);
   for(int c=0;c<3;c++)(&centre.x)[c]/=9;
   v3 from=centre,to=centre;from.z+=80;to.z-=80;
   if(!oracle_face(i,from,to))continue;probes++;
   CollisionCandidates cand;collision_gather_around(from,to,13,&cand);bool found=false;
   for(int k=0;k<cand.count;k++)found|=cand.index[k]==i;
   RayHit hit;bool yes=collision_raycast(&cand,from,to,&hit,0);
   if(cand.count==COL_MAX_CANDIDATES)cap++;
   if(!yes){misses++;lost+=!found;if(misses<=12)printf("MISS tri=%u p=%d,%d,%d candidates=%d target_present=%d normal=%d,%d,%d\n",i,centre.x,centre.y,centre.z,cand.count,found,t->normal[0],t->normal[1],t->normal[2]);}
  }
 }
 printf("RESULT triangles=%u probes=%u misses=%u target_missing=%u capped_queries=%u\n",g_collision.triangle_count,probes,misses,lost,cap);free(blob);return misses?1:0;
}
'''
if '--incidents' in sys.argv:
    import json
    incident_path=Path(sys.argv[sys.argv.index('--incidents')+1])
    records=json.loads(incident_path.read_text())['incidents']
    selected=[x for x in records if x['stage']==(0 if '--observatory' in sys.argv else 1)]
    initializers=[]
    for x in selected:
        vec=lambda key:'{'+','.join(str(x[key][a]) for a in 'xyz')+'}'
        candidates=x['candidates'];indices=candidates['index'][:candidates['count']]
        initializers.append('{'+str(x['sequence'])+','+vec('from')+','+vec('to')+','+vec('up')+',{{'+','.join(map(str,indices))+'},'+str(len(indices))+'}}')
    prefix+='typedef struct {unsigned sequence;v3 from,to,up;CollisionCandidates candidates;} IncidentFixture;\nstatic IncidentFixture fixtures[]={'+','.join(initializers)+'};\n'
    replay=r'''
 for(unsigned k=0;k<sizeof fixtures/sizeof *fixtures;k++){
  IncidentFixture*x=&fixtures[k];RayHit hit;
  bool yes=collision_raycast(&x->candidates,x->from,x->to,&hit,0);
  printf("INCIDENT sequence=%u production_hit=%d tri=%d geometric_floor_hits=",x->sequence,yes,yes?hit.tri:-1);
  unsigned count=0;
  for(unsigned i=0;i<g_collision.triangle_count;i++){
   const CollisionFace*t=&g_collision.faces[i];
   if((int64_t)t->normal[0]*x->up.x+(int64_t)t->normal[1]*x->up.y+(int64_t)t->normal[2]*x->up.z<((int64_t)2785*16384))continue;
   if(oracle_face(i,x->from,x->to)){printf("%s%u",count?",":"",i);count++;}
  }
  if(!count)printf("none");puts("");
 }
 free(blob);return 0;
'''
    if '--regather' in sys.argv:
        replay=replay.replace('IncidentFixture*x=&fixtures[k];RayHit hit;',
                              'IncidentFixture*x=&fixtures[k];RayHit hit;collision_gather_around(x->from,x->to,13,&x->candidates);')
    if '--strict-incidents' in sys.argv:
        replay=' unsigned incident_failures=0;\n'+replay
        replay=replay.replace('if(!count)printf("none");puts("");',r'''if(!count)printf("none");puts("");
  bool floor_hit=yes&&((int64_t)hit.normal.x*x->up.x+(int64_t)hit.normal.y*x->up.y+(int64_t)hit.normal.z*x->up.z>=((int64_t)2785*16384));
  if((count>0)!=floor_hit){fprintf(stderr,"incident %u disagrees with floor oracle\n",x->sequence);incident_failures++;}''')
        replay=replay.replace('free(blob);return 0;', 'free(blob);return incident_failures?1:0;')
    elif '--require-hit' in sys.argv:
        replay=' unsigned incident_failures=0;\n'+replay
        replay=replay.replace('if(!count)printf("none");puts("");',
                              'if(!count)printf("none");puts("");if(!yes)incident_failures++;')
        replay=replay.replace('free(blob);return 0;', 'free(blob);return incident_failures?1:0;')
    if '--debug-edges' in sys.argv:
        replay=replay.replace('if(oracle_face(i,x->from,x->to)){printf(',r'''if(oracle_face(i,x->from,x->to)){
         ColFaceRef r;col_face_ref(i,&r);v3 n=v3_from_i16(r.normal),a=col_ref_vertex(&r,0,x->from),b=col_ref_vertex(&r,1,x->from),c=col_ref_vertex(&r,2,x->from),d=v3_sub(x->to,x->from);
         int64_t d0=-dot64(a,n),d1=d0+dot64(d,n);int tt=ratio14(d0,d0-d1);
         v3 p=V3((int32_t)(((int64_t)d.x*tt+8192)>>14),(int32_t)(((int64_t)d.y*tt+8192)>>14),(int32_t)(((int64_t)d.z*tt+8192)>>14));
         v3 pa=v3_sub(a,p),pb=v3_sub(b,p),pc=v3_sub(c,p);
         printf("\n DETAIL seq=%u tri=%u a=%d,%d,%d b=%d,%d,%d c=%d,%d,%d n=%d,%d,%d d0=%lld d1=%lld tt=%d p=%d,%d,%d w=%lld,%lld,%lld\n",x->sequence,i,a.x,a.y,a.z,b.x,b.y,b.z,c.x,c.y,c.z,n.x,n.y,n.z,(long long)d0,(long long)d1,tt,p.x,p.y,p.z,(long long)dot64(v3_cross(pa,pb),n),(long long)dot64(v3_cross(pb,pc),n),(long long)dot64(v3_cross(pc,pa),n));
         printf(''')
    suffix=suffix.replace(' unsigned probes=0,misses=0,lost=0,cap=0;',replay+'\n unsigned probes=0,misses=0,lost=0,cap=0;')

if '--contract' in sys.argv:
    contract=r'''
 int16_t points[]={-100,-100,0,100,-100,0,-100,100,0};
 CollisionFace floor={.point={0,1,2},.normal={0,0,16384}};
 g_collision.faces=&floor;g_collision.points=points;g_collision.triangle_count=1;g_collision.shift=0;
 CollisionCandidates candidates={.index={0},.count=1};RayHit hit;
 #define REQUIRE(x) do{if(!(x)){fprintf(stderr,"contract failed: %s\n",#x);return 1;}}while(0)
 REQUIRE(collision_raycast(&candidates,V3(0,0,20),V3(0,0,0),&hit,0)&&hit.t==16384);
 REQUIRE(!collision_raycast(&candidates,V3(0,0,0),V3(10,0,0),&hit,0));
 REQUIRE(!collision_raycast(&candidates,V3(0,0,20),V3(0,0,1),&hit,0));
 REQUIRE(collision_raycast(&candidates,V3(0,0,0),V3(0,0,-20),&hit,0)&&hit.t==0);
 REQUIRE(!collision_raycast(&candidates,V3(0,0,-20),V3(0,0,20),&hit,0));
 REQUIRE(!collision_raycast(&candidates,V3(200,200,20),V3(200,200,-20),&hit,0));
 /* A slightly quantized stored normal must not turn a coplanar line into
  * an artificial edge contact. Inclusion uses the actual triangle plane. */
 floor.normal[0]=1;
 REQUIRE(!collision_raycast(&candidates,V3(20,0,0),V3(-200,0,0),&hit,0));
 int16_t thin[]={0,0,0,30000,1,1,30000,2,1};
 g_collision.points=thin;floor.normal[0]=-1;
 REQUIRE(collision_raycast(&candidates,V3(15000,1,80),V3(15000,1,-80),&hit,0));
 REQUIRE(!collision_raycast(&candidates,V3(15000,2,80),V3(15000,2,-80),&hit,0));

 free(blob);puts("Ray contracts passed: endpoint, coplanar/quantized normal, no crossing, start contact, one-sided, thin edge contact, outside triangle");return 0;
'''
    suffix=suffix.replace(' unsigned probes=0,misses=0,lost=0,cap=0;',contract+'\n unsigned probes=0,misses=0,lost=0,cap=0;')

if '--body-candidates' in sys.argv:
    suffix=suffix.replace('collision_gather_around(from,to,13,&cand);', 'v3 body=centre;body.z+=70;collision_gather_around(body,body,180,&cand);')

with tempfile.TemporaryDirectory(prefix='collision-audit-',dir=ROOT/'tmp') as temporary:
 p=Path(temporary);(p/'test.c').write_text(prefix+production+suffix)
 stage='observatory' if '--observatory' in sys.argv else 'honeyhive'
 stage_file=Path(sys.argv[sys.argv.index('--stage-file')+1]).resolve() if '--stage-file' in sys.argv else ROOT/f'build/assets/stage_{stage}.bin'
 stage_arg=shlex.quote(str(stage_file.relative_to(ROOT)))
 result=subprocess.run(['docker','run','--rm','--user','1000:1000','-v',f'{ROOT}:/work','-w','/work','smg-runtime-tests','sh','-c',f'cc -O2 -fwrapv -I src {p.relative_to(ROOT)}/test.c -lm -o {p.relative_to(ROOT)}/test && {p.relative_to(ROOT)}/test {stage_arg}'],check=False,timeout=60)
 raise SystemExit(result.returncode)
