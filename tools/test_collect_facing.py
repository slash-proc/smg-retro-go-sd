#!/usr/bin/env python3
"""Prove guarded signed32 triangle facing against independently rounded products.

Extracts the production camera guard and narrow facing helper. Docker UBSan
checks overflow at signed16 coordinate/normal endpoints and random interiors;
the int64 oracle uses explicit floor division for negative values.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
s=(ROOT/'src/graphics/visibility.c').read_text()
production=s[s.index('static bool collect_narrow_camera('):s.index('/* End collect facing helpers. */')]
prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <limits.h>
typedef struct {int32_t cam[3];} CollectJob;
'''
suffix=r'''
static uint32_t seed=0x76f87a19;
static uint32_t rnd(void){seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;return seed;}
static unsigned checks,guard_checks;
static int64_t floor256(int64_t x){return x<0?-((-x+255)/256):x/256;}
static void check(int32_t a[3],int32_t n[3],CollectJob j){
 if(!collect_narrow_camera(&j))abort();
 int64_t expected=0;
 for(unsigned k=0;k<3;k++){
  int64_t product=((int64_t)a[k]-j.cam[k])*n[k];
  if(product<INT32_MIN||product>INT32_MAX)abort();
  expected+=floor256(product);
 }
 if(expected<INT32_MIN||expected>INT32_MAX||collect_facing_narrow(a,n,j.cam)!=expected)abort();
 checks++;
}
int main(void){
 const int32_t values[]={INT16_MIN,INT16_MIN+1,-257,-256,-255,-1,0,1,255,256,257,INT16_MAX-1,INT16_MAX};
 for(unsigned ai=0;ai<13;ai++)for(unsigned ni=0;ni<13;ni++)for(unsigned ci=0;ci<13;ci++){
  // Include sums of three extreme products and mixed signs, not only dots
  // with a single nonzero component. Each product rounds independently.
  for(unsigned offset=0;offset<13;offset++){
   int32_t a[3],n[3];CollectJob j;
   for(unsigned k=0;k<3;k++){
    a[k]=values[(ai+k*offset)%13];n[k]=values[(ni+k*offset)%13];j.cam[k]=values[(ci+k*offset)%13];
   }
   check(a,n,j);
  }
 }
 for(unsigned i=0;i<2000000;i++){
  int32_t a[3],n[3];CollectJob j;
  for(unsigned k=0;k<3;k++){a[k]=(int16_t)rnd();n[k]=i%2?(int16_t)rnd():(int8_t)rnd()*2;j.cam[k]=(int16_t)rnd();}
  check(a,n,j);
 }
 const int32_t camera[]={INT32_MIN,-1000000,-32769,-32768,-32767,0,32766,32767,32768,1000000,INT32_MAX};
 for(unsigned x=0;x<11;x++)for(unsigned y=0;y<11;y++)for(unsigned z=0;z<11;z++){
  CollectJob j={.cam={camera[x],camera[y],camera[z]}};bool expected=true;
  for(unsigned k=0;k<3;k++)if(j.cam[k]<-32768||j.cam[k]>32767)expected=false;
  if(collect_narrow_camera(&j)!=expected)abort();guard_checks++;
 }
 printf("%u exact signed32 facing cases against int64 floor oracle; %u full-int32 camera guards pass\n",checks,guard_checks);
 return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='collect-facing-',dir=ROOT/'tmp') as directory:
    path=Path(directory);(path/'test.c').write_text(prefix+production+suffix)
    try:
        subprocess.run(['docker','run','--rm','--name',path.name,'--user','1000:1000',
            '-v',f'{path}:/test','-w','/test','smg-runtime-tests','sh','-c',
            'cc -O3 -no-pie -fsanitize=address,undefined -fno-sanitize-recover=all test.c -o test && ./test'],check=True,timeout=60)
    finally:
        subprocess.run(['docker','rm','-f',path.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
