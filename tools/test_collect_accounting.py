#!/usr/bin/env python3
"""Compare batched collection totals against frozen immediate-update accounting.

This checks accounting/early-exit equivalence, not GBA rendering correctness.
Separate independent reference and arithmetic fixtures establish that contract.
"""
import ast
from pathlib import Path
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
def section(s,start,end):
    i=s.index(start)
    return s[i:s.index(end,i)]
def literal(path,name):
    for node in ast.parse(path.read_text()).body:
        if isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id==name for t in node.targets):return ast.literal_eval(node.value)
    raise ValueError(name)

prefix=literal(ROOT/'tools/test_visibility.py','prefix')
header=(ROOT/'src/graphics/visibility.h').read_text()
types=section(header,'typedef struct {\n    const uint16_t* leaves;','} CollectJob;')+'} CollectJob;\n'
source=(ROOT/'src/graphics/visibility.c').read_text()
production=section(source,'static volatile bool g_collect_narrow_enabled','\nvoid geometry_collect_leaves(')
accounting=r'''
#undef SMG_COUNT
#undef SMG_ADD
static struct {uint32_t leaf_triangles_tested,leaf_triangles_rejected,point_list_capacity_hits;} smg_geometry_stats;
#define SMG_COUNT(field) (++smg_geometry_stats.field)
#define SMG_ADD(field,count) (smg_geometry_stats.field+=(uint32_t)(count))
'''
suffix=r'''
int main(void){
 unsigned wraps[3]={0};
 for(unsigned test=0;test<20000;test++){
  uint16_t leaves[8],offsets[9],ranks[128],plist1[256],plist2[256],home[128]={0};
  uint32_t bits1[8],bits2[8];uint8_t marks1[128],marks2[128];
  StageTriangle triangles[128];LeafTriangle records[128];int16_t points[128][3];
  for(unsigned i=0;i<8;i++){leaves[i]=next()%8;bits1[i]=next();}
  for(unsigned i=0;i<9;i++)offsets[i]=i*16;
  for(unsigned i=0;i<256;i++)plist1[i]=(uint16_t)next();
  for(unsigned i=0;i<128;i++){
   marks1[i]=(uint8_t)next();ranks[i]=next()%256;
   for(unsigned k=0;k<3;k++){
    points[i][k]=(int16_t)next();
    triangles[i].v[k].point=records[i].points[k]=next()%128;
    triangles[i].normal[k]=(int8_t)next();records[i].normal[k]=(int16_t)next();records[i].a[k]=(int16_t)next();
   }
  }
  memcpy(plist2,plist1,sizeof plist1);memcpy(bits2,bits1,sizeof bits1);memcpy(marks2,marks1,sizeof marks1);
  CollectJob a={.leaves=leaves,.leaf_count=test%17?8:0,.leaf_triangle_offsets=offsets,.recs=records,.ranks=ranks,
   .limit=(int16_t)next(),.marks=marks1,.stamp=(uint8_t)next(),.bits=bits1,.lo=UINT32_MAX,.hi=0,
   .plist=plist1,.plist_end=plist1+test%130,.home=test%3?NULL:home,.points=points,.triangles=triangles,
   .axis_coord=(test%3)*2,.axis_sign=test%2?-1:0};
  const int32_t camera[]={-1000000,-32769,-32768,-1,0,32767,32768,1000000};
  for(unsigned k=0;k<3;k++)a.cam[k]=camera[(test+k)%8];
  CollectJob b=a;b.marks=marks2;b.bits=bits2;b.plist=plist2;b.plist_end=plist2+(a.plist_end-plist1);
  // Test modulo32 accumulation, initial nonzero totals, disabled narrow mode,
  // empty jobs and an already full list. Last admitted triangles may add three
  // points as in the original; backing arrays exceed the configured list span.
  uint32_t initial[3]={test%4?next():UINT32_MAX-test%3,
   test%4?next():UINT32_MAX-test%3,test%11?next():UINT32_MAX},expected[3];
  memcpy(&smg_geometry_stats,initial,sizeof initial);
  reference_g_collect_narrow_enabled=g_collect_narrow_enabled=test%5!=0;
  reference_geometry_collect_leaves_c(&a,test%2);memcpy(expected,&smg_geometry_stats,sizeof expected);
  for(unsigned k=0;k<3;k++)if(expected[k]<initial[k])wraps[k]++;
  memcpy(&smg_geometry_stats,initial,sizeof initial);geometry_collect_leaves_c(&b,test%2);
  if(memcmp(expected,&smg_geometry_stats,sizeof expected)||a.plist-plist1!=b.plist-plist2||a.lo!=b.lo||a.hi!=b.hi||
   memcmp(marks1,marks2,sizeof marks1)||memcmp(bits1,bits2,sizeof bits1)||memcmp(plist1,plist2,sizeof plist1)){
   fprintf(stderr,"collection accounting/output mismatch at job %u\n",test);return 1;
  }
 }
 if(!wraps[0]||!wraps[1]||!wraps[2])abort();
 printf("Counter wrap cases: tested=%u rejected=%u capacity=%u\n",wraps[0],wraps[1],wraps[2]);
 puts("20000 exact immediate/batched collection counters and outputs; modulo32, empty/full jobs, early capacity exits, indexed/home and guarded/wide modes pass");return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='collect-accounting-',dir=ROOT/'tmp') as directory:
    path=Path(directory);(path/'test.c').write_text(prefix+types+accounting+(ROOT/'tools/tests/collect_accounting_reference.inc').read_text()+production+suffix)
    try:
        subprocess.run(['docker','run','--rm','--name',path.name,'--user','1000:1000','-v',f'{ROOT}:/source:ro','-v',f'{path}:/test','-w','/test','smg-runtime-tests','sh','-c',
            'cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined -fno-sanitize-recover=all -I/source/src test.c -o test && ./test'],check=True,timeout=60)
    finally:
        subprocess.run(['docker','rm','-f',path.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
