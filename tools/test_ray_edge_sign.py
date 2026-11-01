#!/usr/bin/env python3
"""Check the production ray edge sign against independent __int128 arithmetic."""
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
s=(ROOT/'src/world/collision.c').read_text();s=s[s.index('IWRAM_CODE static int col_ray_edge_sign('):s.index('\n#endif',s.index('IWRAM_CODE static int col_ray_edge_sign('))]
prefix=r'''
#include <stdint.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#define IWRAM_CODE
typedef struct {int32_t x,y,z;} v3;
'''
suffix=r'''
static uint64_t seed=UINT64_C(0x18427659baefdd23);
static uint32_t rnd(void){seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;return seed;}
static unsigned count;
static void check(v3 a,v3 b,v3 n){
 __int128 cx=(__int128)a.y*b.z-(__int128)a.z*b.y;
 __int128 cy=(__int128)a.z*b.x-(__int128)a.x*b.z;
 __int128 cz=(__int128)a.x*b.y-(__int128)a.y*b.x;
 __int128 value=cx*n.x+cy*n.y+cz*n.z;int expected=(value>0)-(value<0);
 int actual=col_ray_edge_sign(a,b,n);if(actual!=expected){fprintf(stderr,"edge mismatch %u expected%d got%d\n",count,expected,actual);abort();}count++;
}
int main(void){
 v3 pa={180,324,4},pb={-231,-416,-6},n={-414,9,16379};
 check(pa,pb,n);if(col_ray_edge_sign(pa,pb,n)!=-1)abort();
 const int32_t limits[]={INT_MIN,INT_MIN+1,-1048576,-1,0,1,1048576,INT_MAX-1,INT_MAX};
 for(unsigned i=0;i<9;i++)for(unsigned j=0;j<9;j++)for(unsigned k=0;k<9;k++){
  v3 a={limits[i],limits[j],limits[k]},b={limits[k],limits[i],limits[j]};
  check(a,b,(v3){INT_MIN,INT_MAX,INT_MIN});check(a,b,(v3){32767,-32768,32767});check(a,b,(v3){0,0,16384});check(a,a,(v3){-32768,32767,0});
 }
 for(unsigned i=0;i<400000;i++){
  v3 a={(int32_t)rnd(),(int32_t)rnd(),(int32_t)rnd()},b={(int32_t)rnd(),(int32_t)rnd(),(int32_t)rnd()},n={(int32_t)rnd(),(int32_t)rnd(),(int32_t)rnd()};
  if(i%2){a.x>>=12;a.y>>=12;a.z>>=12;b.x>>=12;b.y>>=12;b.z>>=12;}
  check(a,b,n);check(b,a,n);
  check(a,b,(v3){(int16_t)n.x,(int16_t)n.y,(int16_t)n.z});
  if(i%19==0)check(a,b,(v3){0,0,0});
 }
 printf("%u exact edge signs vs __int128: floor55, full int32 boundaries, full signed32 directions, fast/fallback, cancellation and winding pass\n",count);return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='ray-edge-',dir=ROOT/'tmp') as directory:
 p=Path(directory);(p/'test.c').write_text(prefix+s+suffix)
 try:
  subprocess.run(['docker','run','--rm','--name',p.name,'--user','1000:1000','-v',f'{p}:/test','-w','/test','-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c','cc -O3 -no-pie -fsanitize=address,undefined test.c -o test && ./test'],check=True,timeout=60)
 finally:subprocess.run(['docker','rm','-f',p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
