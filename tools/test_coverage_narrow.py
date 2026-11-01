#!/usr/bin/env python3
"""Check narrow scanline bounds against an independent per-pixel oracle and wide shader."""
import ast
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
def literal(path,name):
 tree=ast.parse(path.read_text())
 return next(ast.literal_eval(n.value) for n in tree.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id==name for t in n.targets))
prefix=literal(ROOT/'tools/test_raster.py','prefix')+'#include <assert.h>\n#include <limits.h>\n'
source=(ROOT/'src/graphics/engine.c').read_text()
production=source[source.index('static int64_t raster_edge('):source.index('\nvoid engine_raster(const DrawEntry* entry)')]
begin=production.index('__attribute__((always_inline)) static inline void engine_raster_inner_impl(')
end=production.index('__attribute__((noinline)) static void engine_raster_inner_affine(',begin)
reference=production[begin:end].replace('engine_raster_inner_impl(','reference_impl(',1)
assert reference.count('const bool narrow_coverage = g_raster_coverage_narrow_enabled')==1
reference=reference.replace('const bool narrow_coverage = g_raster_coverage_narrow_enabled','const bool narrow_coverage = false')
math=r'''
static void check_bounds(void){
 const int64_t limits[]={INT32_MIN-INT64_C(1),INT32_MIN,INT32_MIN+INT64_C(1),-1,0,1,INT32_MAX-INT64_C(1),INT32_MAX,INT32_MAX+INT64_C(1)};
 const int64_t slopes[]={-1048560,-1,0,1,1048560};
 const unsigned heights[]={0,1,FRAME_HEIGHT-1};
 for(unsigned i=0;i<sizeof limits/sizeof *limits;i++)for(unsigned j=0;j<5;j++)
 for(unsigned k=0;k<3;k++)for(unsigned component=0;component<3;component++)for(unsigned bits=0;bits<8;bits++){
  int b0=bits&1,b1=(bits>>1)&1,b2=(bits>>2)&1;
  int64_t weights[3]={0};weights[component]=limits[i];
  int64_t dy1=component==0?slopes[j]:component==2?-slopes[j]:0;
  int64_t dy2=component==1?slopes[j]:0;
  int64_t end=limits[i]+slopes[j]*heights[k];
  bool expected=limits[i]>=INT32_MIN&&limits[i]<=INT32_MAX&&end>=INT32_MIN&&end<=INT32_MAX;
  int64_t w1=weights[0]+b1,w2=weights[1]+b2,area=weights[2]+w1+w2+b0;
  assert(raster_coverage_fits_s32(area,w1,w2,dy1,dy2,heights[k],b0,b1,b2)==expected);
 }
 unsigned narrow=0,wide=0;
 for(unsigned n=0;n<200000;n++){
  int32_t weight=(int32_t)next();int32_t step=(int)(next()%2097121)-1048560;
  if(n%17==0)step=0;
  if(n%23==0)weight=INT32_MIN;
  int first=next()%FRAME_WIDTH,last=first+next()%(FRAME_WIDTH-first);
  int actual_first=first,actual_last=last,expected_first=last+1,expected_last=-1;
  for(int x=first;x<=last;x++)if((int64_t)weight+(int64_t)x*step>=0){
   if(expected_first>x)expected_first=x;expected_last=x;
  }
  bool ok=raster_span_bound32(weight,step,&actual_first,&actual_last);
  assert(ok==(expected_last>=expected_first));
  if(ok)assert(actual_first==expected_first&&actual_last==expected_last);
  int old_first=first,old_last=last;
  bool old=raster_span_bound(weight,step,&old_first,&old_last);
  assert(old==ok);if(ok)assert(old_first==actual_first&&old_last==actual_last);
  int64_t area=next(),w1=(int32_t)next(),w2=(int32_t)next();
  int64_t dy1=(int)(next()%2097121)-1048560,dy2=(int)(next()%2097121)-1048560;
  unsigned rows=next()%FRAME_HEIGHT;int b0=n&1,b1=(n>>1)&1,b2=(n>>2)&1;
  bool fits=raster_coverage_fits_s32(area,w1,w2,dy1,dy2,rows,b0,b1,b2),oracle=true;
  uint32_t aw1=(uint32_t)w1-(unsigned)b1,aw2=(uint32_t)w2-(unsigned)b2;
  uint32_t aw0=(uint32_t)(area-w1-w2)-(unsigned)b0;
  for(unsigned y=0;y<=rows;y++){
   int64_t a=w1+dy1*y-b1,b=w2+dy2*y-b2,c=area-w1-dy1*y-w2-dy2*y-b0;
   if(a<INT32_MIN||a>INT32_MAX||b<INT32_MIN||b>INT32_MAX||c<INT32_MIN||c>INT32_MAX)oracle=false;
   if(fits)assert((int32_t)aw1==a&&(int32_t)aw2==b&&(int32_t)aw0==c);
   aw1+=(uint32_t)dy1;aw2+=(uint32_t)dy2;aw0-=(uint32_t)(dy1+dy2);
  }
  assert(fits==oracle);if(fits)narrow++;else wide++;
 }
 assert(narrow&&wide);
 printf("200000 signed32 scanline intervals match independent per-pixel oracle; row guards narrow%u wide%u exact\n",narrow,wide);
}
'''
suffix=literal(ROOT/'tools/test_texture_address.py','suffix').replace('int main(void){','int main(void){\n check_bounds();',1).replace('texture addressing differs','coverage differs').replace('Both mip layouts, every row/column','Narrow coverage; both mip layouts, every row/column')
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='coverage-narrow-',dir=ROOT/'tmp') as directory:
 work=Path(directory);(work/'test.c').write_text(prefix+production+reference+math+suffix)
 try:
  subprocess.run(['docker','run','--rm','--name',work.name,'--user','1000:1000','-v',f'{work}:/test','-w','/test','smg-runtime-tests','sh','-c','cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie test.c -o test && ./test && cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 test.c -o native && ./native'],check=True,timeout=180)
 finally:subprocess.run(['docker','rm','-f',work.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
