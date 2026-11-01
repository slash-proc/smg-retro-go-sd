#!/usr/bin/env python3
"""Check exact shared-reciprocal setup against signed division in Docker.

The independent oracle uses native signed division and unsigned __int128
products. Test near integer boundaries and full numerator range, not merely
random rendered triangles. The renderer's smaller input bounds are covered.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
source=(ROOT/'src/graphics/engine.c').read_text()
start=source.index('static uint64_t raster_abs64(')
end=source.index('static bool raster_span_bound(',start)
production=source[start:end]
prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <limits.h>
'''
suffix=r'''
static uint64_t seed=UINT64_C(0x1032547698badcfe);
static uint64_t rnd(void){seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;return seed;}
static unsigned long cases,corrections;
static void check(int64_t n,int64_t d,uint64_t* reciprocal){
 int64_t expected=n/d;
 int64_t actual=raster_setup_div(n,d,reciprocal,true);
 if(actual!=expected){fprintf(stderr,"ratio %lld/%lld: got %lld vs %lld\n",(long long)n,(long long)d,(long long)actual,(long long)expected);exit(1);}
 uint64_t original=*reciprocal;
 if(raster_setup_div(n,d,reciprocal,false)!=expected||*reciprocal!=original)abort();
 if(n){
  uint64_t a=raster_abs64(n),den=raster_abs64(d);
  if(original!=(UINT64_C(1)<<63)/den)abort();
  uint64_t raw=(uint64_t)(((unsigned __int128)a*original)>>63);
  if((a/den)-raw>1)abort();corrections+=a/den!=raw;
 }
 cases++;
}
int main(void){
 uint64_t inverse=0;if(raster_setup_div(0,17,&inverse,true)!=0||inverse)abort();
 const uint64_t ns[]={0,1,2,3,UINT32_MAX,UINT64_C(1)<<32,(UINT64_C(1)<<53)-1,INT64_MAX};
 const uint64_t ds[]={1,2,3,7,16,17,255,256,257,65535,65536,65537,UINT32_MAX,(UINT64_C(1)<<32)+1,(UINT64_C(1)<<33)-1,INT64_MAX};
 for(unsigned i=0;i<sizeof ds/sizeof *ds;i++){
  inverse=0;for(unsigned j=0;j<sizeof ns/sizeof *ns;j++)for(unsigned sign=0;sign<4;sign++)
   check(sign&1?-(int64_t)ns[j]:(int64_t)ns[j],sign&2?-(int64_t)ds[i]:(int64_t)ds[i],&inverse);
 }
 for(unsigned group=0;group<200000;group++){
  int64_t d=1+(rnd()&UINT64_C(0x1ffffffff));inverse=0;
  uint64_t n=rnd()&INT64_MAX;
  check(n,d,&inverse);check(-n,d,&inverse);check(n,-d,&inverse);check(-n,-d,&inverse);
  // Exact multiples and adjacent values exercise the one-step correction.
  uint64_t q=n/(uint64_t)d,near=q*(uint64_t)d;
  check(near,d,&inverse);if(near)check(near-1,d,&inverse);if(near<INT64_MAX)check(near+1,d,&inverse);
 }
 if(!corrections)abort();
 printf("%lu exact signed ratios vs independent division; %lu Q63 correction cases, wide-product oracle and shared/lazy inverse pass\n",cases,corrections);
 return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='setup-div-',dir=ROOT/'tmp') as directory:
    path=Path(directory);(path/'test.c').write_text(prefix+production+suffix)
    try:
        subprocess.run(['docker','run','--rm','--name',path.name,'--user','1000:1000',
            '-v',f'{path}:/test','-w','/test','-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0',
            'smg-runtime-tests','sh','-c',
            'cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined test.c -o test && ./test'],check=True,timeout=60)
    finally:
        subprocess.run(['docker','rm','-f',path.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
