#!/usr/bin/env python3
"""Check guarded narrow cull intervals against independent wide arithmetic.

Docker ASan/UBSan covers full signed16 bounds, independent signed plane/radius
coefficients, coefficient-L1 boundaries and offset/tolerance headroom. Uses an
explicit floor oracle, not the production signed-shift expressions.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
s=(ROOT/'src/graphics/visibility.c').read_text()
production=s[s.index('static bool cull_narrow_planes('):s.index('int geometry_cull_chunks(')]
prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <limits.h>
#include <string.h>
typedef struct {int32_t planes[5][7];} CullJob;
'''
suffix=r'''
static uint32_t seed=0x5a3c79f1;
static uint32_t rnd(void){seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;return seed;}
static unsigned checked,rejected;
static int64_t floor2(int64_t x,unsigned bits){return x<0?-((-x+((INT64_C(1)<<bits)-1))>>bits):x>>bits;}
static void check(CullJob job,int16_t lo[3],int16_t hi[3]){
 bool expected=true;
 for(unsigned p=0;p<5;p++){
  int64_t a=0,b=0;
  for(unsigned k=0;k<3;k++){
   int64_t x=job.planes[p][k],y=job.planes[p][k+3];a+=x<0?-x:x;b+=y<0?-y:y;
  }
  if(a>32767||b>32767||job.planes[p][6]<(int64_t)INT32_MIN+8388672||job.planes[p][6]>(int64_t)INT32_MAX-8388672)expected=false;
 }
 if(cull_narrow_planes(&job)!=expected)abort();
 if(!expected){rejected++;return;}
 int32_t center[3],extent[3];for(unsigned k=0;k<3;k++){center[k]=(int32_t)lo[k]+hi[k];extent[k]=(int32_t)hi[k]-lo[k];}
 for(unsigned p=0;p<5;p++){
  int64_t m=0,r=0;for(unsigned k=0;k<3;k++){m+=(int64_t)center[k]*job.planes[p][k];r+=(int64_t)extent[k]*job.planes[p][k+3];}
  int64_t low=floor2(floor2(m,1)-floor2(r,1),8)+job.planes[p][6];
  int64_t high=floor2(floor2(m,1)+floor2(r,1),8)+job.planes[p][6];
  if(low-64<INT32_MIN||low+64>INT32_MAX||high-64<INT32_MIN||high+64>INT32_MAX)abort();
  int32_t actual_low,actual_high;cull_narrow_interval(center,extent,job.planes[p],&actual_low,&actual_high);
  if(actual_low!=low||actual_high!=high){fprintf(stderr,"interval mismatch %u\n",checked);exit(1);}checked++;
 }
}
int main(void){
 const int16_t edges[]={INT16_MIN,INT16_MIN+1,-1,0,1,INT16_MAX-1,INT16_MAX};
 const int32_t offsets[]={INT32_MIN,INT32_MIN+8388671,INT32_MIN+8388672,-65,-64,-1,0,1,64,65,INT32_MAX-8388672,INT32_MAX-8388671,INT32_MAX};
 for(unsigned n=0;n<1000000;n++){
  CullJob j={0};int16_t lo[3],hi[3];
  for(unsigned k=0;k<3;k++){lo[k]=n%3?(int16_t)rnd():edges[rnd()%7];hi[k]=n%3?(int16_t)rnd():edges[rnd()%7];}
  for(unsigned p=0;p<5;p++){
   for(unsigned group=0;group<2;group++){
    unsigned remaining=32767;
    for(unsigned k=0;k<3;k++){
     unsigned magnitude=k==2?remaining:rnd()%(remaining+1);remaining-=magnitude;
     j.planes[p][group*3+k]=rnd()&1?-(int32_t)magnitude:(int32_t)magnitude;
    }
   }
   j.planes[p][6]=offsets[rnd()%13];
  }
  if(n%11==0)j.planes[n%5][n%6]=(int32_t)rnd();
  check(j,lo,hi);
 }
 // Both signs at/beyond coefficient boundaries, including INT32_MIN abs.
 const int32_t coeff[]={INT32_MIN,-32769,-32768,-32767,32767,32768,32769,INT32_MAX};
 int16_t lo[3]={INT16_MIN,INT16_MIN,INT16_MIN},hi[3]={INT16_MAX,INT16_MAX,INT16_MAX};
 for(unsigned i=0;i<8;i++)for(unsigned k=0;k<6;k++){
  CullJob j={0};j.planes[4][k]=coeff[i];check(j,lo,hi);
 }
 if(!checked||!rejected)abort();
 printf("%u narrow intervals match independent floor/wide oracle; %u jobs rejected safely; signed coefficient/radius bounds and +/-64 tolerance headroom pass\n",checked,rejected);
}
'''
with tempfile.TemporaryDirectory(prefix='cull-interval-',dir=ROOT/'tmp') as directory:
 p=Path(directory);(p/'test.c').write_text(prefix+production+suffix)
 try:
  subprocess.run(['docker','run','--rm','--name',p.name,'--user','1000:1000','-v',f'{p}:/test','-w','/test','-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c','cc -O3 -no-pie -fsanitize=address,undefined test.c -o test && ./test'],check=True,timeout=60)
 finally:subprocess.run(['docker','rm','-f',p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
